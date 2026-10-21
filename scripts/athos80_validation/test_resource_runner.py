#!/usr/bin/env python3
"""Offline regressions: mocked HTTP/clocks only; never launch a service or load."""
import argparse
import copy
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import resource_runner as rr


def response(variant_id=11):
    return {"contract": "test", "lexical_profile": "bm25", "result_mode": "products",
            "facet_count_mode": "products", "num_found": 1, "facets": [],
            "metrics": {name: 1 for name in rr.COUNT_METRICS},
            "hits": [{"product_id": 1, "selected_variant_id": variant_id}]}


def thresholds(**changes):
    values = dict(slo_p95_ms=50.0, slo_p99_ms=100.0, max_error_rate=.001,
                  min_completion_ratio=.98, min_success_ratio=.98,
                  max_generator_lag_p99_ms=10.0, max_generator_lag_max_ms=100.0,
                  max_client_cpu_cores=.8)
    values.update(changes)
    return values


def measured_summary():
    return {"response_validation_pass": True, "elapsed_including_drain_seconds": 10.,
            "valid_zero_error_zero_drop": True,
            "successful_scheduled_latency_ms": {"p95": 49., "p99": 99.},
            "generator_schedule_lag_ms": {"p99": 10., "max": 100.},
            "error_rate": 0., "completion_ratio": .98, "success_rate": .98,
            "all_cases_scheduled": True}


def resources():
    return dict(stable_process_tree=True, pss_complete=True, ownership_complete=True, sampler_ok=True)


def golden_fixture(cases, expected=None):
    expected = expected or response()
    return {"schema": "athos80-http-goldens-v1", "workload_sha256": "a" * 64,
            "catalog_sha256": "b" * 64, "binary_sha256": "c" * 64,
            "oracle_summary_sha256": "d" * 64, "normalization": "remove metrics.elapsed_micros only",
            "entries": {arm: {case["id"]: {"request_sha256": rr.digest(rr.request_for(case, arm)),
                                            "stable_response_sha256": rr.stable_response_digest(expected)}
                              for case in cases} for arm in rr.ARMS}}


class RunnerTests(unittest.TestCase):
    def test_nonce_observability_distinguishes_missing_error_and_positive_counts(self):
        with tempfile.TemporaryDirectory() as temp:
            service = rr.OwnedService(["unused"], temp, Path(temp), 200)
            service.proc = SimpleNamespace(pid=123)
            stat = {"pid": 123, "starttime": 456, "state": "S"}
            with patch.object(rr, "proc_stat", return_value=stat):
                with patch.object(Path, "read_bytes", return_value=f"{rr.OWNERSHIP_ENV}={service.nonce}\0".encode()):
                    self.assertTrue(service._has_nonce(123))
                with patch.object(Path, "read_bytes", return_value=b"UNRELATED_SECRET=never-log-this\0"):
                    self.assertFalse(service._has_nonce(123))
                with patch.object(Path, "read_bytes", side_effect=PermissionError(13, "mock")):
                    self.assertFalse(service._has_nonce(123))
            self.assertEqual(service.nonce_read_counts[123],
                             {"matching_marker": 1, "marker_absent": 1, "read_oserror": 1})
            events = [json.loads(line) for line in (Path(temp) / "ownership_read_failures.jsonl").read_text().splitlines()]
            self.assertEqual([event["read_status"] for event in events], ["marker_absent", "read_oserror"])
            self.assertEqual(events[1]["errno"], 13)
            self.assertEqual(events[1]["error_type"], "PermissionError")
            self.assertEqual(events[0]["post_read_identity"], stat)
            raw = json.dumps(events)
            self.assertNotIn("never-log-this", raw)
            self.assertNotIn(service.nonce, raw)

    def test_observability_preserves_failclosed_even_after_exit_or_pid_reuse(self):
        snapshot = {"pid": 123, "ppid": 10, "session": 123, "starttime": 456, "state": "S"}
        for newer in (None, snapshot | {"state": "Z"}, snapshot | {"starttime": 789}, snapshot):
            with self.subTest(newer=newer), tempfile.TemporaryDirectory() as temp:
                service = rr.OwnedService(["unused"], temp, Path(temp), 200)
                service.proc = SimpleNamespace(pid=123)
                with patch.object(Path, "iterdir", return_value=iter([Path("/proc/123")])), \
                     patch.object(rr, "proc_stat", side_effect=[snapshot, newer]), \
                     patch.object(Path, "read_bytes", side_effect=PermissionError(13, "mock")):
                    self.assertEqual(service._discover(), {})
                # OBSERVABILITY ONLY: no stale-snapshot ignore/grace/clearing.
                self.assertEqual(service.ownership_violations, {(123, 456)})
                event = json.loads((Path(temp) / "ownership_violations.jsonl").read_text())
                self.assertEqual(event["snapshot_identity"], {"pid": 123, "starttime": 456, "state": "S"})
                self.assertEqual(event["nonce_read"]["post_read_identity"],
                                 {field: newer[field] for field in ("pid", "starttime", "state")} if newer else None)
                service.proc = None  # Mock exited lifecycle; never created a process.
                with patch.object(rr, "proc_stat", return_value=None):
                    service.close()
                cleanup = json.loads((Path(temp) / "cleanup.json").read_text())
                self.assertTrue(cleanup["complete"])
                self.assertEqual(cleanup["historical_ownership_violations"], [[123, 456]])
                self.assertEqual(len(cleanup["ownership_violation_events"]), 1)

    def test_live_wrong_nonce_permanently_fails_even_after_matching_read(self):
        stat = {"pid": 123, "ppid": 10, "session": 123, "starttime": 456, "state": "S"}
        with tempfile.TemporaryDirectory() as temp:
            service = rr.OwnedService(["unused"], temp, Path(temp), 200)
            service.proc = SimpleNamespace(pid=123)
            with patch.object(Path, "iterdir", side_effect=lambda: iter([Path("/proc/123")])), \
                 patch.object(rr, "proc_stat", return_value=stat):
                with patch.object(Path, "read_bytes", return_value=b"ATHOS80_RESOURCE_OWNER=wrong\0"):
                    self.assertEqual(service._discover(), {})
                with patch.object(Path, "read_bytes", return_value=f"{rr.OWNERSHIP_ENV}={service.nonce}\0".encode()):
                    self.assertIn((123, 456), service._discover())
            self.assertEqual(service.ownership_violations, {(123, 456)})
            self.assertEqual(service.ownership_violation_events[0]["nonce_read"]["read_status"], "marker_absent")

    def test_schedule_epoch_excludes_slow_preparation_and_waits_for_all_workers(self):
        clock = [100.]
        clock_reads = []
        ready_threads = set()
        ready_lock = rr.threading.Lock()
        expected = response()
        cases = [{"id": "q", "request": {"q": "chair"}}]
        original_request = rr.request_for
        original_open = Path.open
        original_pool = rr.concurrent.futures.ThreadPoolExecutor
        original_barrier = rr.threading.Barrier

        def slow_request(case, arm):
            clock[0] += 10.  # Mocked setup cost: no real sleep.
            return original_request(case, arm)

        def slow_open(path, *args, **kwargs):
            clock[0] += 20.
            return original_open(path, *args, **kwargs)

        def slow_pool(*args, **kwargs):
            clock[0] += 30.
            return original_pool(*args, **kwargs)

        def observed_barrier(parties):
            clock[0] += 40.
            barrier = original_barrier(parties)
            original_wait = barrier.wait

            def wait(timeout=None):
                with ready_lock:
                    ready_threads.add(rr.threading.get_ident())
                return original_wait(timeout)

            barrier.wait = wait
            return barrier

        def monotonic():
            clock_reads.append(clock[0])
            return clock[0]

        def fake_http(*_args):
            clock[0] += .02
            return 200, copy.deepcopy(expected), 100

        with tempfile.TemporaryDirectory() as temp:
            raw_path = Path(temp) / "raw.jsonl"
            with patch.object(rr, "request_for", side_effect=slow_request), \
                 patch.object(Path, "open", new=slow_open), \
                 patch.object(rr.concurrent.futures, "ThreadPoolExecutor", side_effect=slow_pool), \
                 patch.object(rr.threading, "Barrier", side_effect=observed_barrier), \
                 patch.object(rr.time, "monotonic", side_effect=monotonic), \
                 patch.object(rr, "http_json", side_effect=fake_http):
                result = rr.fixed_qps(cases, "base", "127.0.0.1", 1, "/search", 1., 1.,
                                      1., 4, 4, {"q": rr.parity_signature(expected)},
                                      {"base": {"q": rr.digest(expected["hits"])}}, raw_path)
            raw = json.loads(raw_path.read_text())
        self.assertEqual(clock_reads[0], 200.)
        self.assertEqual(len(ready_threads), 5)  # Four distinct workers plus scheduler.
        self.assertEqual(raw["generator_schedule_lag_ms"], 0.)
        self.assertEqual(raw["worker_queue_delay_ms"], 0.)
        self.assertAlmostEqual(raw["scheduled_latency_ms"], 20.)
        self.assertAlmostEqual(result["elapsed_including_drain_seconds"], .02)
        self.assertTrue(result["valid_zero_error_zero_drop"])

    def test_full_response_golden_rejects_wrong_active_ids_despite_same_counts_and_hits(self):
        cases = [{"id": "q", "request": {}}]
        expected = response()
        expected["active_rule_ids"] = ["expected"]
        wrong = copy.deepcopy(expected)
        wrong["active_rule_ids"] = ["wrong"]
        self.assertEqual(rr.parity_signature(expected), rr.parity_signature(wrong))
        self.assertEqual(rr.digest(expected["hits"]), rr.digest(wrong["hits"]))
        with tempfile.TemporaryDirectory() as temp, \
             patch.object(rr, "http_json", return_value=(200, wrong, 100)), \
             self.assertRaisesRegex(RuntimeError, "golden full-response mismatch"):
            rr.preflight(cases, "base", "127.0.0.1", 1, "/search", 1., {}, {}, Path(temp),
                         golden_fixture(cases, expected))

    def test_golden_missing_case_or_request_mismatch_rejected_before_http(self):
        cases = [{"id": "q", "request": {}}]
        for fault in ("missing_case", "request_mismatch"):
            goldens = golden_fixture(cases)
            if fault == "missing_case":
                del goldens["entries"]["base"]["q"]
            else:
                goldens["entries"]["base"]["q"]["request_sha256"] = "e" * 64
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as temp, \
                 patch.object(rr, "http_json") as http, self.assertRaises(ValueError):
                rr.preflight(cases, "base", "127.0.0.1", 1, "/search", 1., {}, {}, Path(temp), goldens)
            http.assert_not_called()

    def test_golden_normalization_excludes_only_elapsed_and_includes_profile(self):
        before = response()
        before["metrics"]["elapsed_micros"] = 123
        after = copy.deepcopy(before)
        after["metrics"]["elapsed_micros"] = 456
        self.assertEqual(rr.stable_response_digest(before), rr.stable_response_digest(after))
        after["lexical_profile"] = "exact_token_and"
        self.assertNotEqual(rr.stable_response_digest(before), rr.stable_response_digest(after))
        after = copy.deepcopy(before)
        after["metrics"]["verified_variants"] += 1
        self.assertNotEqual(rr.stable_response_digest(before), rr.stable_response_digest(after))

    def test_golden_loader_binds_inputs_complete_arms_and_requests(self):
        cases = [{"id": "q", "request": {}}]
        good = golden_fixture(cases)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "goldens.json"
            path.write_text(json.dumps(good))
            self.assertEqual(rr.load_goldens(path, cases, "a" * 64, "c" * 64, {"b" * 64, "d" * 64}), good)
            for fault in ("workload", "binary", "catalog", "oracle_summary", "missing_arm", "request", "extra_case"):
                with self.subTest(fault=fault):
                    bad = copy.deepcopy(good)
                    if fault in ("workload", "binary", "catalog", "oracle_summary"):
                        bad[f"{fault}_sha256"] = "e" * 64
                    elif fault == "missing_arm":
                        del bad["entries"]["personal"]
                    elif fault == "extra_case":
                        bad["entries"]["base"]["stale_case"] = copy.deepcopy(bad["entries"]["base"]["q"])
                    else:
                        bad["entries"]["both"]["q"]["request_sha256"] = "e" * 64
                    path.write_text(json.dumps(bad))
                    with self.assertRaises(ValueError):
                        rr.load_goldens(path, cases, "a" * 64, "c" * 64, {"b" * 64, "d" * 64})

    def test_golden_duplicate_json_keys_rejected_not_silently_overwritten(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "goldens.json"
            path.write_text('{"schema":"athos80-http-goldens-v1","schema":"athos80-http-goldens-v1"}')
            with self.assertRaisesRegex(ValueError, "duplicate golden JSON key"):
                rr.load_goldens(path, [{"id": "q", "request": {}}], "a" * 64, "c" * 64, {"b" * 64, "d" * 64})

    def test_separate_cycle_rotation_start_zero_and_one(self):
        self.assertEqual(rr.arm_order(0), ("base", "merch", "personal", "both"))
        self.assertEqual(rr.arm_order(1), ("merch", "personal", "both", "base"))
        self.assertEqual(rr.arm_order(4), rr.arm_order(0))
        with self.assertRaises(ValueError):
            rr.arm_order(-1)

    def test_rotated_first_arm_uses_uniform_base_then_arm_preflight(self):
        cases = [{"id": "q", "request": {}, "merch_rules": [{"action": {"type": "group"}}]}]
        refs, rankings = {}, {}
        calls = []

        def fake_http(_host, _port, _path, request, _timeout):
            calls.append(copy.deepcopy(request))
            return 200, response(12 if request["rules"] else 11), 100

        with tempfile.TemporaryDirectory() as temp, patch.object(rr, "http_json", side_effect=fake_http):
            rr.preflight_arm(cases, "merch", "127.0.0.1", 1, "/search", 1., refs, rankings, Path(temp))
            self.assertTrue((Path(temp) / "base_preflight" / "preflight.jsonl").exists())
            self.assertTrue((Path(temp) / "arm_preflight" / "preflight.jsonl").exists())
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[0]["rules"], [])
        self.assertTrue(calls[1]["rules"])
        self.assertIn("q", refs)
        self.assertNotEqual(rankings["base"]["q"], rankings["merch"]["q"])

    def test_base_arm_also_receives_exactly_two_full_preflight_passes(self):
        cases = [{"id": "q1", "request": {"q": "one"}}, {"id": "q2", "request": {"q": "two"}}]
        refs, rankings = {}, {}
        with tempfile.TemporaryDirectory() as temp, \
             patch.object(rr, "http_json", return_value=(200, response(), 100)) as http:
            rr.preflight_arm(cases, "base", "127.0.0.1", 1, "/search", 1., refs, rankings, Path(temp))
        self.assertEqual([call.args[3]["q"] for call in http.call_args_list], ["one", "two", "one", "two"])
        self.assertTrue(all(call.args[3]["rules"] == [] for call in http.call_args_list))

    def test_latency_excludes_slow_validation_with_mock_clock_and_sleep(self):
        clock = [100.]
        expected = response()
        signature = rr.parity_signature(expected)
        original = rr.parity_signature

        def fake_sleep(seconds):
            clock[0] += seconds

        def fake_http(*_args):
            fake_sleep(.02)
            return 200, copy.deepcopy(expected), 100

        def slow_validation(actual):
            rr.time.sleep(2.0)  # Mocked: advance clock; never really sleep.
            return original(actual)

        with patch.object(rr.time, "monotonic", side_effect=lambda: clock[0]), \
             patch.object(rr.time, "sleep", side_effect=fake_sleep), \
             patch.object(rr, "http_json", side_effect=fake_http), \
             patch.object(rr, "parity_signature", side_effect=slow_validation):
            result = rr.perform_request("q", {}, "base", 0, 100., 100., 100.,
                                        "127.0.0.1", 1, "/search", 1., signature,
                                        rr.digest(expected["hits"]))
        self.assertEqual(result["outcome"], "ok")
        self.assertAlmostEqual(result["http_latency_ms"], 20.)
        self.assertAlmostEqual(result["scheduled_latency_ms"], 20.)
        self.assertAlmostEqual(result["validation_ms"], 2000.)

    def test_equal_counts_but_changed_hit_fails_same_arm(self):
        expected = response(11)
        changed = response(12)
        self.assertEqual(rr.parity_signature(expected), rr.parity_signature(changed))
        cases = [{"id": "q", "request": {"q": "chair", "lexical": {"profile": "bm25"}}}]
        with tempfile.TemporaryDirectory() as temp, \
             patch.object(rr, "http_json", return_value=(200, changed, 100)), \
             patch.object(rr.time, "sleep", return_value=None):
            result = rr.fixed_qps(cases, "base", "127.0.0.1", 1, "/search", 1., 1.,
                                  1., 1, 1, {"q": rr.parity_signature(expected)},
                                  {"base": {"q": rr.digest(expected["hits"])}}, Path(temp) / "raw.jsonl")
            raw = json.loads((Path(temp) / "raw.jsonl").read_text())
        self.assertTrue(raw["parity_ok"])
        self.assertFalse(raw["same_arm_ranking_ok"])
        self.assertEqual(result["outcomes"]["ranking_failure"], 1)
        self.assertFalse(result["response_validation_pass"])
        self.assertFalse(result["valid_zero_error_zero_drop"])

    def test_cross_arm_rankings_allowed_but_same_arm_frozen(self):
        cases = [{"id": "q", "request": {}}]
        refs, rankings = {}, {}
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp) / "base"
            merch = Path(temp) / "merch"
            base.mkdir()
            merch.mkdir()
            with patch.object(rr, "http_json", return_value=(200, response(11), 100)):
                rr.preflight(cases, "base", "127.0.0.1", 1, "/search", 1., refs, rankings, base)
            with patch.object(rr, "http_json", return_value=(200, response(12), 100)):
                rr.preflight(cases, "merch", "127.0.0.1", 1, "/search", 1., refs, rankings, merch)
                with self.assertRaisesRegex(RuntimeError, "same-arm ranking changed"):
                    rr.preflight(cases, "base", "127.0.0.1", 1, "/search", 1., refs, rankings, base)
        self.assertNotEqual(rankings["base"]["q"], rankings["merch"]["q"])

    def test_timeout_counts_against_offered_error_and_completion_ratios(self):
        expected = response()
        cases = [{"id": "q", "request": {}}]
        with tempfile.TemporaryDirectory() as temp, \
             patch.object(rr, "http_json", side_effect=TimeoutError("mock timeout")), \
             patch.object(rr.time, "sleep", return_value=None):
            result = rr.fixed_qps(cases, "base", "127.0.0.1", 1, "/search", 1., 1.,
                                  1., 1, 1, {"q": rr.parity_signature(expected)},
                                  {"base": {"q": rr.digest(expected["hits"])}}, Path(temp) / "raw.jsonl")
        self.assertEqual(result["offered_requests"], 1)
        self.assertEqual(result["finished_attempts"], 1)
        self.assertEqual(result["completed_requests"], 0)
        self.assertEqual(result["completion_ratio"], 0.)
        self.assertEqual(result["success_rate"], 0.)
        self.assertEqual(result["error_rate"], 1.)
        self.assertFalse(rr.assess_run(result, resources(), 0., thresholds())["resource_comparison_valid"])

    def test_no_thresholds_never_implies_capacity_pass(self):
        self.assertIsNone(rr.freeze_thresholds(argparse.Namespace()))
        result = rr.assess_run(measured_summary(), resources(), 1., None)
        self.assertTrue(result["accounting_valid"])
        self.assertTrue(result["resource_comparison_valid"])
        self.assertIsNone(result["slo_pass"])
        self.assertFalse(result["capacity_valid"])

    def test_threshold_validation_rejects_partial_nonfinite_and_invalid_ranges(self):
        with self.assertRaisesRegex(ValueError, "partial SLO"):
            rr.freeze_thresholds(argparse.Namespace(slo_p95_ms=50.))
        for changes in ({"slo_p99_ms": 40.}, {"max_error_rate": float("nan")},
                        {"min_success_ratio": 1.01}, {"max_error_rate": 0.},
                        {"max_generator_lag_max_ms": 1.}, {"max_client_cpu_cores": 0.}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                rr.freeze_thresholds(argparse.Namespace(**thresholds(**changes)))
        self.assertEqual(rr.freeze_thresholds(argparse.Namespace(**thresholds())), thresholds())

    def test_strict_slo_boundaries_and_inclusive_ratio_lag_boundaries(self):
        summary = measured_summary()
        result = rr.assess_run(summary, resources(), 1., thresholds())
        self.assertTrue(result["capacity_valid"])
        for field, value in (("p95", 50.), ("p99", 100.)):
            changed = copy.deepcopy(summary)
            changed["successful_scheduled_latency_ms"][field] = value
            result = rr.assess_run(changed, resources(), 1., thresholds())
            self.assertTrue(result["accounting_valid"])
            self.assertFalse(result["slo_pass"])
            self.assertFalse(result["capacity_valid"])
        changed = copy.deepcopy(summary)
        changed["error_rate"] = .001
        self.assertFalse(rr.assess_run(changed, resources(), 1., thresholds())["slo_pass"])
        for ratio in ("completion_ratio", "success_rate"):
            changed = copy.deepcopy(summary)
            changed[ratio] = .9799
            self.assertFalse(rr.assess_run(changed, resources(), 1., thresholds())["slo_pass"])

    def test_generator_failure_not_engine_capacity_failure(self):
        summary = measured_summary()
        summary["generator_schedule_lag_ms"]["p99"] = 10.01
        result = rr.assess_run(summary, resources(), 1., thresholds())
        self.assertTrue(result["slo_pass"])
        self.assertFalse(result["capacity_valid"])
        self.assertIn("not_engine_capacity_failure", result["capacity_classification"])
        result = rr.assess_run(measured_summary(), resources(), 8., thresholds())
        self.assertFalse(result["client_cpu_pass"])  # Strict < .8, not <= .8.
        self.assertFalse(result["capacity_valid"])

    def test_any_error_timeout_or_drop_invalidates_cost_even_if_slo_allows_rate(self):
        for outcome in ("error", "timeout", "admission_drop"):
            with self.subTest(outcome=outcome):
                summary = measured_summary()
                summary["valid_zero_error_zero_drop"] = False
                summary["outcomes"] = {outcome: 1}
                summary["error_rate"] = .0005 if outcome != "admission_drop" else 0.
                summary["completion_ratio"] = .9995
                summary["success_rate"] = .9995
                result = rr.assess_run(summary, resources(), 1., thresholds())
                self.assertTrue(result["accounting_valid"])
                self.assertTrue(result["slo_pass"])
                self.assertFalse(result["resource_comparison_valid"])
                self.assertFalse(result["capacity_valid"])
    def test_worker_queue_delay_not_mislabelled_dispatch_lag(self):
        expected = response()
        with patch.object(rr.time, "monotonic", return_value=101.), \
             patch.object(rr, "http_json", return_value=(200, expected, 100)):
            result = rr.perform_request("q", {}, "base", 0, 100., 100.01, 100.,
                                        "127.0.0.1", 1, "/search", 1., rr.parity_signature(expected),
                                        rr.digest(expected["hits"]))
        self.assertAlmostEqual(result["generator_schedule_lag_ms"], 10.)
        self.assertAlmostEqual(result["worker_queue_delay_ms"], 990.)
        self.assertAlmostEqual(result["queue_delay_ms"], 1000.)

    def test_existing_output_rejected_without_service_or_socket(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            workload = root / "workload.json"
            workload.write_text(json.dumps({"cases": [{"id": "q", "request": {}}]}))
            output = root / "existing"
            output.mkdir()
            marker = output / "keep.txt"
            marker.write_text("unchanged")
            argv = ["resource_runner.py", "--run-approved", "--approval-note", "offline guard test",
                    "--workload", str(workload), "--server-command", json.dumps([sys.executable]),
                    "--golden-file", str(root / "not-read-because-output-exists.json"),
                    "--endpoint", "http://127.0.0.1:18780/search", "--qps", "10", "--output", str(output)]
            with patch.object(sys, "argv", argv), patch.object(rr, "OwnedService") as service, \
                 patch.object(rr, "port_open") as connect, self.assertRaises(FileExistsError):
                rr.main()
            service.assert_not_called()
            connect.assert_not_called()
            self.assertEqual(marker.read_text(), "unchanged")

    def test_missing_execution_gate_rejects_before_any_service(self):
        argv = ["resource_runner.py", "--approval-note", "offline guard test", "--workload", "missing",
                "--golden-file", "not-read-without-execution-gate.json",
                "--server-command", json.dumps([sys.executable]), "--endpoint", "http://127.0.0.1:18780/search",
                "--qps", "10", "--output", "unused"]
        with patch.object(sys, "argv", argv), patch.object(rr, "OwnedService") as service, \
             self.assertRaisesRegex(SystemExit, "NOT RUN"):
            rr.main()
        service.assert_not_called()

    def test_missing_mandatory_goldens_rejected_before_service_or_socket(self):
        argv = ["resource_runner.py", "--run-approved", "--approval-note", "offline guard test",
                "--workload", "missing", "--server-command", json.dumps([sys.executable]),
                "--endpoint", "http://127.0.0.1:18780/search", "--qps", "10", "--output", "unused"]
        with patch.object(sys, "argv", argv), patch.object(rr, "OwnedService") as service, \
             patch.object(rr, "port_open") as connect, patch.object(sys, "stderr"), \
             self.assertRaises(SystemExit) as error:
            rr.main()
        self.assertEqual(error.exception.code, 2)
        service.assert_not_called()
        connect.assert_not_called()


if __name__ == "__main__":
    unittest.main()
