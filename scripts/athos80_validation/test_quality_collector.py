"""Offline only: never starts a service, binds a port, or sends HTTP."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

import quality_collector as qc


def cases():
    return [{"id": f"wands-{i}", "query_id": str(i),
             "request": {"q": f'exact query {i} with "quote"', "lexical": {"profile": "bm25_literal"},
                         "rules": [], "personal_signals": []}}
            for i in range(480)]


class QualityCollectorTests(unittest.TestCase):
    def test_noncontiguous_official_id_487_retained_without_renumbering(self):
        fixture = cases()
        fixture[-1]["id"] = "wands-487"
        fixture[-1]["query_id"] = "487"
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "cases.json"
            path.write_text(json.dumps({"cases": fixture}))
            _, observed = qc.load_cases(path)
        self.assertEqual(observed, fixture)
        self.assertEqual(observed[-1]["id"], "wands-487")
        self.assertNotIn("wands-479", {case["id"] for case in observed})

    def test_actual_frozen_quality_cases_preserve_all_exact_noncontiguous_ids(self):
        path = Path(__file__).resolve().parent / "quality" / "quality_cases.json"
        self.assertTrue(path.is_file(), "bundled public quality cases are required")
        source = json.loads(path.read_text())["cases"]
        _, observed = qc.load_cases(path)
        self.assertEqual(observed, source)
        self.assertEqual(len(observed), 480)
        self.assertEqual(len({case["id"] for case in observed}), 480)
        self.assertIn("wands-487", {case["id"] for case in observed})

    def test_all_480_exact_ids_and_errors_retained_without_retry(self):
        original = cases()
        service = MagicMock()
        service.owns_listener.return_value = True
        seen = []
        def fake_http(_host, _port, _path, request, _timeout):
            seen.append(copy.deepcopy(request))
            index = len(seen) - 1
            if index == 208:
                return 400, {"error": "retained HTTP error"}
            if index == 285:
                raise TimeoutError("retained transport error")
            return 200, {"hits": [], "query": request["q"]}
        with tempfile.TemporaryDirectory() as temp, patch.object(qc, "http_call", side_effect=fake_http):
            output = Path(temp) / "responses.jsonl"
            summary = qc.collect(original, service, 18782, 1., output)
            rows = [json.loads(line) for line in output.read_text().splitlines()]
        self.assertEqual(len(seen), 480)
        self.assertEqual([r["id"] for r in rows], [c["id"] for c in original])
        self.assertEqual([r["request"] for r in rows], [c["request"] for c in original])
        self.assertEqual(rows[208]["status"], 400)
        self.assertIsNone(rows[285]["status"])
        self.assertEqual(summary["http_success_count"], 478)
        self.assertEqual(summary["transport_error_count"], 1)
        self.assertTrue(summary["all_480_unique_ids_retained"])

    def test_listener_loss_does_not_contact_replacement_and_retains_ids(self):
        service = MagicMock()
        service.owns_listener.return_value = False
        with tempfile.TemporaryDirectory() as temp, patch.object(qc, "http_call") as http:
            summary = qc.collect(cases(), service, 18782, 1., Path(temp) / "responses.jsonl")
        http.assert_not_called()
        self.assertEqual(summary["transport_error_count"], 480)
        self.assertTrue(summary["all_480_unique_ids_retained"])

    def test_cases_reject_missing_duplicate_wrong_id_or_nonbase(self):
        for fault in ("missing", "duplicate", "wrong_query_id", "merch", "negative", "leading_zero", "noncanonical_prefix"):
            fixture = cases()
            if fault == "missing": fixture.pop()
            elif fault == "duplicate": fixture[1]["id"] = fixture[0]["id"]
            elif fault == "wrong_query_id": fixture[1]["query_id"] = "0"
            elif fault == "merch": fixture[1]["request"]["rules"] = [{"id": "not-base"}]
            elif fault == "negative": fixture[1].update(id="wands--1", query_id="-1")
            elif fault == "leading_zero": fixture[1].update(id="wands-01", query_id="01")
            else: fixture[1].update(id="1", query_id="1")
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as temp:
                path = Path(temp) / "cases.json"
                path.write_text(json.dumps({"cases": fixture}))
                with self.assertRaises(ValueError): qc.load_cases(path)

    def test_required_expected_hashes_reject_change(self):
        with tempfile.TemporaryDirectory() as temp:
            paths = [Path(temp) / name for name in ("binary", "catalog", "cases")]
            for path in paths: path.write_text(path.name)
            hashes = [qc.reviewed.file_digest(path) for path in paths]
            self.assertEqual(qc.verify_expected_inputs(*paths, *hashes)["cases_sha256"], hashes[2])
            paths[1].write_text("changed")
            with self.assertRaisesRegex(ValueError, "catalog SHA256 mismatch"):
                qc.verify_expected_inputs(*paths, *hashes)

    def test_redirect_returned_once_and_full_nonjson_error_retained(self):
        conn = MagicMock()
        reply = conn.getresponse.return_value
        reply.status = 302
        reply.read.return_value = b'{"location":"https://must-not-follow.example/"}'
        with patch.object(qc.http.client, "HTTPConnection", return_value=conn) as make:
            status, result = qc.http_call("127.0.0.1", 18782, "/search", {"q": "same"}, 1.)
        self.assertEqual(status, 302)
        self.assertIn("location", result)
        make.assert_called_once_with("127.0.0.1", 18782, timeout=1.)
        conn.request.assert_called_once()
        conn.close.assert_called_once()

    def test_nonloopback_rejected_before_connection(self):
        with patch.object(qc.http.client, "HTTPConnection") as make, self.assertRaises(ValueError):
            qc.http_call("192.0.2.1", 18782, "/search", {}, 1.)
        make.assert_not_called()

    def test_execution_gate_before_process_or_socket(self):
        argv = ["quality_collector.py", "--approval-note", "offline test", "--binary", "missing",
                "--catalog", "missing", "--cases", "missing", "--expect-binary-sha256", "a" * 64,
                "--expect-catalog-sha256", "b" * 64, "--expect-cases-sha256", "c" * 64,
                "--port", "18782", "--output", "unused"]
        with patch.object(sys, "argv", argv), patch.object(qc.reviewed, "OwnedService") as service, \
             patch.object(qc.socket, "socket") as sock, self.assertRaisesRegex(SystemExit, "NOT RUN"):
            qc.main()
        service.assert_not_called()
        sock.assert_not_called()


if __name__ == "__main__":
    unittest.main()
