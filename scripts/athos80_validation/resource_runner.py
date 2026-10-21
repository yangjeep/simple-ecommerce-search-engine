#!/usr/bin/env python3
"""Gated, loopback-only four-arm fixed-QPS experiment; standard library only.

Preparation does not launch anything. Execution requires --run-approved. This
flag records a operator-reviewed gate, not an assertion of product parity.
No build, remote operation, production endpoint, or global process kill occurs.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import copy
import hashlib
import http.client
import ipaddress
import json
import math
import os
from pathlib import Path
import signal
import shutil
import socket
import subprocess
import sys
import threading
import time
from typing import Any
from urllib.parse import urlsplit
import uuid


VERSION = "athos80-resource-runner-v4-ownership-observability"
ARMS = ("base", "merch", "personal", "both")
COUNT_METRICS = (
    "catalog_variants", "lexical_candidates", "structural_candidates",
    "verified_variants", "eligible_variants", "eligible_products",
    "facet_verified_variants", "ranked_variants",
)
OWNERSHIP_ENV = "ATHOS80_RESOURCE_OWNER"
SLO_REQUIRED = ("slo_p95_ms", "slo_p99_ms", "max_error_rate", "min_completion_ratio",
                "min_success_ratio", "max_generator_lag_p99_ms", "max_generator_lag_max_ms", "max_client_cpu_cores")


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(value: Any) -> str:
    return hashlib.sha256(canonical(value)).hexdigest()


def file_digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for part in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(part)
    return h.hexdigest()


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n")


def quantiles(values: list[float]) -> dict[str, float | None]:
    values = sorted(values)
    def q(p: float) -> float | None:
        return values[max(0, math.ceil(p * len(values)) - 1)] if values else None
    return {"p50": q(.50), "p95": q(.95), "p99": q(.99), "max": values[-1] if values else None}


def endpoint_parts(endpoint: str) -> tuple[str, int, str]:
    url = urlsplit(endpoint)
    if url.scheme != "http" or url.username or url.password or url.query or url.fragment:
        raise ValueError("endpoint must be plain http://literal-loopback:port/path without credentials/query")
    try:
        address = ipaddress.ip_address(url.hostname or "")
    except ValueError as error:
        raise ValueError("endpoint host must be a literal loopback address, not DNS") from error
    if not address.is_loopback:
        raise ValueError("non-loopback endpoints are forbidden")
    if url.port is None:
        raise ValueError("endpoint requires an explicit experiment port")
    return str(address), url.port, url.path or "/search"


def port_open(host: str, port: int) -> bool:
    try:
        with socket.create_connection((host, port), timeout=.2):
            return True
    except OSError:
        return False


def proc_stat(pid: int) -> dict[str, Any] | None:
    try:
        text = Path(f"/proc/{pid}/stat").read_text()
        # comm may contain spaces and parentheses; split only after the final ')'.
        end = text.rfind(")")
        fields = text[end + 2:].split()
        return {"pid": pid, "comm": text[text.find("(") + 1:end],
                "state": fields[0], "ppid": int(fields[1]), "session": int(fields[3]),
                "ticks": int(fields[11]) + int(fields[12]), "starttime": int(fields[19]),
                "rss_kib": int(fields[21]) * os.sysconf("SC_PAGE_SIZE") / 1024}
    except (OSError, ValueError, IndexError):
        return None


def pss_kib(pid: int) -> float | None:
    try:
        for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
            if line.startswith("Pss:"):
                return float(line.split()[1])
    except (OSError, ValueError, IndexError):
        pass
    return None


class OwnedService:
    """Own one new process session plus nonce-bearing descendants; PID-safe cleanup."""
    def __init__(self, command: list[str], cwd: str, output: Path, sample_ms: int):
        self.nonce = uuid.uuid4().hex
        self.command, self.cwd, self.output = command, cwd, output
        self.sample_seconds = sample_ms / 1000
        self.proc: subprocess.Popen[bytes] | None = None
        self.log = None
        self.owned: dict[tuple[int, int], dict[str, Any]] = {}
        self.last_ticks: dict[tuple[int, int], int] = {}
        self.cpu_ticks = 0
        self.samples: list[dict[str, Any]] = []
        self.exits: set[tuple[int, int]] = set()
        self.last_alive: set[tuple[int, int]] = set()
        self.lock = threading.RLock()
        self.stop = threading.Event()
        self.thread: threading.Thread | None = None
        self.measuring = False
        self.started_during_measure: set[tuple[int, int]] = set()
        self.ownership_violations: set[tuple[int, int]] = set()
        self.ownership_violation_events: list[dict[str, Any]] = []
        self.nonce_read_failures: list[dict[str, Any]] = []
        self.last_nonce_failure: dict[int, dict[str, Any]] = {}
        self.nonce_read_counts: dict[int, dict[str, int]] = {}
        self.lifecycle = "created"
        self.sampler_error: str | None = None

    def start(self) -> None:
        self.lifecycle = "starting"
        env = dict(os.environ)
        env[OWNERSHIP_ENV] = self.nonce
        self.log = (self.output / "service.log").open("wb")
        self.proc = subprocess.Popen(self.command, cwd=self.cwd, env=env,
                                     stdin=subprocess.DEVNULL, stdout=self.log,
                                     stderr=subprocess.STDOUT, start_new_session=True)
        self.sample()
        self.lifecycle = "started"
        self.thread = threading.Thread(target=self._sample_loop, name="resource-sampler", daemon=True)
        self.thread.start()

    def _has_nonce(self, pid: int) -> bool:
        error_number, error_type = None, None
        try:
            matches = f"{OWNERSHIP_ENV}={self.nonce}".encode() in Path(f"/proc/{pid}/environ").read_bytes().split(b"\0")
            status = "matching_marker" if matches else "marker_absent"
        except OSError as error:
            matches, status = False, "read_oserror"
            error_number, error_type = error.errno, type(error).__name__
        counts = self.nonce_read_counts.setdefault(pid, {})
        counts[status] = counts.get(status, 0) + 1
        if not matches:
            # Observation only: preserve the exact fail-closed return value and
            # discovery's historical latch, even if this newer stat shows exit.
            # Never persist environment bytes/values, matching or otherwise.
            observed = proc_stat(pid)
            event = {"monotonic": time.monotonic(), "lifecycle": self.lifecycle,
                     "measuring": self.measuring, "pid": pid,
                     "popen_root_pid": self.proc.pid if self.proc else None,
                     "read_status": status, "error_type": error_type, "errno": error_number,
                     "post_read_identity": {field: observed[field] for field in ("pid", "starttime", "state")} if observed else None}
            self.nonce_read_failures.append(event)
            self.last_nonce_failure[pid] = event
            with (self.output / "ownership_read_failures.jsonl").open("a") as stream:
                stream.write(json.dumps(event, sort_keys=True, allow_nan=False) + "\n")
        return matches

    def _discover(self) -> dict[tuple[int, int], dict[str, Any]]:
        with self.lock:
            return self._discover_unlocked()

    def _discover_unlocked(self) -> dict[tuple[int, int], dict[str, Any]]:
        if self.proc is None:
            return {}
        stats = {}
        for path in Path("/proc").iterdir():
            if path.name.isdecimal():
                stat = proc_stat(int(path.name))
                if stat:
                    stats[stat["pid"]] = stat
        alive = {}
        # Build an additional ancestry/session set to detect missing ownership
        # markers instead of silently omitting an engine/adapter's resource use.
        expected_pids = {self.proc.pid}
        expected_pids.update(pid for pid, stat in stats.items() if stat["session"] == self.proc.pid)
        expected_pids.update(pid for pid, start in self.owned if pid in stats and stats[pid]["starttime"] == start)
        previous_size = -1
        while previous_size != len(expected_pids):
            previous_size = len(expected_pids)
            expected_pids.update(pid for pid, stat in stats.items() if stat["ppid"] in expected_pids)
        # Read environments only for our session/ancestry/tracked identities.
        # Deliberately do not scan unrelated host processes for this nonce.
        # Rapid double-fork + setsid escape before sampling is unsupported;
        # the reviewed Rust service is stable and thread-based, not daemonized.
        for pid in expected_pids:
            stat = stats.get(pid)
            if stat is None:
                continue
            key = (pid, stat["starttime"])
            has_nonce = self._has_nonce(pid)
            if has_nonce:
                alive[key] = stat
            elif stat["state"] != "Z":
                if key not in self.ownership_violations:
                    event = {"monotonic": time.monotonic(), "lifecycle": self.lifecycle,
                             "measuring": self.measuring,
                             "snapshot_identity": {field: stat[field] for field in ("pid", "starttime", "state")},
                             "nonce_read": self.last_nonce_failure.get(pid)}
                    self.ownership_violation_events.append(event)
                    with (self.output / "ownership_violations.jsonl").open("a") as stream:
                        stream.write(json.dumps(event, sort_keys=True, allow_nan=False) + "\n")
                self.ownership_violations.add(key)
        return alive

    def sample(self) -> dict[str, Any]:
        with self.lock:
            alive = self._discover()
            for key, stat in alive.items():
                if key not in self.owned:
                    try:
                        cmdline = Path(f"/proc/{key[0]}/cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace").strip()
                    except OSError:
                        cmdline = "<unavailable>"
                    self.owned[key] = {**stat, "cmdline": cmdline, "first_seen_monotonic": time.monotonic()}
                    if self.measuring:
                        self.started_during_measure.add(key)
                before = self.last_ticks.get(key, stat["ticks"])
                self.cpu_ticks += max(0, stat["ticks"] - before)
                self.last_ticks[key] = stat["ticks"]
            if self.measuring:
                self.exits.update(self.last_alive - alive.keys())
            self.last_alive = set(alive)
            pss = [pss_kib(key[0]) for key in alive]
            sample = {"monotonic": time.monotonic(), "cpu_ticks_cumulative": self.cpu_ticks,
                      "rss_kib": sum(stat["rss_kib"] for stat in alive.values()),
                      "pss_kib": sum(x for x in pss if x is not None) if all(x is not None for x in pss) else None,
                      "pids": [key[0] for key in sorted(alive)],
                      "pss_missing_pids": [key[0] for key, amount in zip(alive, pss) if amount is None]}
            if self.measuring:
                self.samples.append(sample)
            return sample

    def _sample_loop(self) -> None:
        while not self.stop.wait(self.sample_seconds):
            try:
                self.sample()
            except BaseException as error:
                self.sampler_error = f"{type(error).__name__}: {error}"
                self.stop.set()

    def begin_measurement(self) -> dict[str, Any]:
        if self.sampler_error:
            raise RuntimeError(f"resource sampler failed: {self.sampler_error}")
        baseline = self.sample()
        with self.lock:
            self.samples = []
            self.exits = set()
            self.started_during_measure = set()
            self.measuring = True
        return baseline

    def end_measurement(self, baseline: dict[str, Any]) -> dict[str, Any]:
        end = self.sample()
        with self.lock:
            self.measuring = False
            samples = list(self.samples)
        write_json(self.output / "resource_samples.json", samples)
        write_json(self.output / "process_inventory.json", list(self.owned.values()))
        pss_missing = any(s["pss_kib"] is None for s in samples)
        return {"cpu_seconds": (end["cpu_ticks_cumulative"] - baseline["cpu_ticks_cumulative"]) / os.sysconf("SC_CLK_TCK"),
                "sample_window_seconds": end["monotonic"] - baseline["monotonic"],
                "aggregate_rss_kib": quantiles([s["rss_kib"] for s in samples]),
                "aggregate_pss_kib": quantiles([s["pss_kib"] for s in samples if s["pss_kib"] is not None]),
                "pss_complete": not pss_missing,
                "processes_seen": len(self.owned),
                "processes_started_during_measurement": [list(key) for key in sorted(self.started_during_measure)],
                "processes_exited_during_measurement": [list(key) for key in sorted(self.exits)],
                "stable_process_tree": not self.started_during_measure and not self.exits,
                "ownership_complete": not self.ownership_violations,
                "ownership_violations": [list(key) for key in sorted(self.ownership_violations)],
                "sampler_ok": self.sampler_error is None and bool(samples),
                "sampler_error": self.sampler_error,
                "sample_count": len(samples)}

    def owns_listener(self, host: str, port: int) -> bool:
        inodes = set()
        for table in ("/proc/net/tcp", "/proc/net/tcp6"):
            try:
                lines = Path(table).read_text().splitlines()[1:]
            except OSError:
                continue
            for line in lines:
                fields = line.split()
                if len(fields) <= 9 or fields[3] != "0A":
                    continue
                encoded_address, encoded_port = fields[1].split(":")
                if int(encoded_port, 16) != port:
                    continue
                raw = bytes.fromhex(encoded_address)
                # Linux /proc encodes addresses as native-endian 32-bit words.
                if sys.byteorder == "little":
                    raw = b"".join(raw[index:index + 4][::-1] for index in range(0, len(raw), 4))
                address = ipaddress.ip_address(raw)
                if address.is_unspecified or str(address) == host or str(getattr(address, "ipv4_mapped", None)) == host:
                    inodes.add(fields[9])
        for key in self._discover():
            try:
                for fd in Path(f"/proc/{key[0]}/fd").iterdir():
                    try:
                        target = os.readlink(fd)
                    except OSError:
                        continue
                    if target.startswith("socket:[") and target[8:-1] in inodes:
                        return True
            except OSError:
                continue
        return False

    def close(self) -> None:
        # No pkill, killall, process-name match, or negative/host-wide PID.
        self.lifecycle = "cleanup"
        self.stop.set()
        if self.thread:
            self.thread.join(timeout=5)
        self.sample()
        for sig, wait_seconds in ((signal.SIGTERM, 3.0), (signal.SIGKILL, 1.0)):
            self.sample()  # Include children created during prior shutdown phase.
            for key in sorted(self.owned, reverse=True):
                now = proc_stat(key[0])
                if now and now["starttime"] == key[1] and self._has_nonce(key[0]):
                    try:
                        os.kill(key[0], sig)
                    except ProcessLookupError:
                        pass
            until = time.monotonic() + wait_seconds
            while time.monotonic() < until:
                if not self._discover():
                    break
                time.sleep(.05)
        if self.proc:
            try:
                self.proc.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass
        remaining = self._discover()
        unmarked_survivors = []
        for key in self.ownership_violations:
            stat = proc_stat(key[0])
            if stat and stat["starttime"] == key[1] and stat["state"] != "Z":
                unmarked_survivors.append(list(key))
        write_json(self.output / "cleanup.json", {"owned_processes_remaining": [list(k) for k in remaining],
                                                 "unmarked_descendants_not_signaled": unmarked_survivors,
                                                 "historical_ownership_violations": [list(k) for k in sorted(self.ownership_violations)],
                                                 "ownership_violation_events": self.ownership_violation_events,
                                                 "nonce_read_failures": self.nonce_read_failures,
                                                 "nonce_read_counts": self.nonce_read_counts,
                                                 "owned_process_identities": [list(k) for k in sorted(self.owned)],
                                                 "popen_root_pid": self.proc.pid if self.proc else None,
                                                 "complete": not remaining and not unmarked_survivors})
        self.lifecycle = "closed"
        if self.log:
            self.log.close()
        if remaining or unmarked_survivors:
            raise RuntimeError("owned service processes remain; inspect cleanup.json; no unrelated process was signaled")


def http_json(host: str, port: int, path: str, payload: dict[str, Any] | None,
              timeout: float) -> tuple[int, Any, int]:
    connection = http.client.HTTPConnection(host, port, timeout=timeout)
    try:
        body = canonical(payload) if payload is not None else None
        connection.request("POST" if payload is not None else "GET", path, body=body,
                           headers={"Content-Type": "application/json", "Connection": "close"})
        response = connection.getresponse()
        raw = response.read()
        try:
            parsed = json.loads(raw)
        except json.JSONDecodeError:
            parsed = {"non_json_body_prefix": raw[:1024].decode(errors="replace")}
        return response.status, parsed, len(raw)
    finally:
        connection.close()


def request_for(case: dict[str, Any], arm: str) -> dict[str, Any]:
    request = copy.deepcopy(case["request"])
    request["rules"] = copy.deepcopy(case.get("merch_rules", [])) if arm in ("merch", "both") else []
    request["personal_signals"] = copy.deepcopy(case.get("personal_signals", [])) if arm in ("personal", "both") else []
    return request


def parity_signature(response: dict[str, Any]) -> dict[str, Any]:
    required = ("contract", "lexical_profile", "result_mode", "facet_count_mode", "num_found", "facets", "metrics", "hits")
    for field in required:
        if field not in response:
            raise ValueError(f"success response missing required field {field}")
    for field in COUNT_METRICS:
        if field not in response["metrics"]:
            raise ValueError(f"success response missing metric {field}")
    return {field: response[field] for field in required if field not in ("metrics", "hits")} | {
        "metrics": {field: response["metrics"][field] for field in COUNT_METRICS}}


def stable_response_digest(response: dict[str, Any]) -> str:
    """Golden normalization: remove exactly metrics.elapsed_micros, nothing else."""
    stable = copy.deepcopy(response)
    if isinstance(stable.get("metrics"), dict):
        stable["metrics"].pop("elapsed_micros", None)
    return digest(stable)


def valid_sha256(value: Any) -> bool:
    return isinstance(value, str) and len(value) == 64 and all(char in "0123456789abcdef" for char in value)


def unique_json_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate golden JSON key: {key}")
        result[key] = value
    return result


def load_goldens(path: Path, cases: list[dict[str, Any]], workload_sha256: str,
                 binary_sha256: str, evidence_hashes: set[str]) -> dict[str, Any]:
    goldens = json.loads(path.read_text(), object_pairs_hook=unique_json_object)
    if goldens.get("schema") != "athos80-http-goldens-v1":
        raise ValueError("unsupported golden schema")
    if goldens.get("normalization") != "remove metrics.elapsed_micros only":
        raise ValueError("unsupported golden normalization")
    for field in ("workload_sha256", "catalog_sha256", "binary_sha256", "oracle_summary_sha256"):
        if not valid_sha256(goldens.get(field)):
            raise ValueError(f"missing/invalid golden {field}")
    if goldens["workload_sha256"] != workload_sha256 or goldens["binary_sha256"] != binary_sha256:
        raise ValueError("goldens do not match frozen workload/binary")
    if goldens["catalog_sha256"] not in evidence_hashes:
        raise ValueError("golden catalog hash must match a supplied --evidence-file")
    if goldens["oracle_summary_sha256"] not in evidence_hashes:
        raise ValueError("golden oracle summary hash must match a supplied --evidence-file")
    entries = goldens.get("entries")
    if not isinstance(entries, dict):
        raise ValueError("golden entries must map every arm and case")
    if set(entries) != set(ARMS):
        raise ValueError("golden arm coverage must be exactly base/merch/personal/both")
    expected_case_ids = {case["id"] for case in cases}
    for arm in ARMS:
        if not isinstance(entries.get(arm), dict):
            raise ValueError(f"golden missing arm/case {arm}/*")
        if set(entries[arm]) != expected_case_ids:
            raise ValueError(f"golden case coverage mismatch for {arm}")
        for case in cases:
            entry = entries.get(arm, {}).get(case["id"])
            if not isinstance(entry, dict):
                raise ValueError(f"golden missing arm/case {arm}/{case['id']}")
            if not valid_sha256(entry.get("request_sha256")) or not valid_sha256(entry.get("stable_response_sha256")):
                raise ValueError(f"invalid golden digest for {arm}/{case['id']}")
            if entry["request_sha256"] != digest(request_for(case, arm)):
                raise ValueError(f"golden request binding mismatch for {arm}/{case['id']}")
    return goldens


def load_workload(path: Path) -> dict[str, Any]:
    workload = json.loads(path.read_text())
    cases = workload.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ValueError("workload must contain a nonempty cases list")
    ids = set()
    for case in cases:
        if not isinstance(case.get("id"), str) or case["id"] in ids:
            raise ValueError("case IDs must be unique strings")
        ids.add(case["id"])
        if not isinstance(case.get("request"), dict):
            raise ValueError("every case requires request object")
        if case["request"].get("rules") or case["request"].get("personal_signals"):
            raise ValueError("base request rules/signals must be empty; use case merch_rules/personal_signals")
        for rule in case.get("merch_rules", []):
            if rule.get("action", {}).get("type") not in ("group", "pin"):
                raise ValueError("equal-candidate resource experiment accepts only group/pin rules, never hide")
        canonical(case)  # Fail on NaN/Infinity or unserializable values before launch.
    return workload


def wait_ready(service: OwnedService, host: str, port: int, path: str, timeout: float) -> None:
    end = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < end:
        if service.proc and service.proc.poll() is not None:
            raise RuntimeError(f"service exited before ready: {service.proc.returncode}; see service.log")
        try:
            status, _, _ = http_json(host, port, path, None, min(2.0, timeout))
            if status == 200 and service.owns_listener(host, port):
                if service.ownership_violations:
                    raise RuntimeError("service child lacks ownership marker; complete resource accounting is not established")
                return
            last_error = f"health HTTP {status}; owned_listener={service.owns_listener(host, port)}"
        except (OSError, http.client.HTTPException) as error:
            last_error = repr(error)
        time.sleep(.1)
    raise TimeoutError(f"owned loopback service not ready: {last_error}")


def preflight(cases: list[dict[str, Any]], arm: str, host: str, port: int, path: str,
              timeout: float, references: dict[str, dict[str, Any]],
              ranking_references: dict[str, dict[str, str]], output: Path,
              goldens: dict[str, Any] | None = None) -> None:
    with (output / "preflight.jsonl").open("w") as stream:
        for case in cases:
            request = request_for(case, arm)
            golden = None
            if goldens is not None:
                golden = goldens.get("entries", {}).get(arm, {}).get(case["id"])
                if not isinstance(golden, dict):
                    raise ValueError(f"golden missing arm/case {arm}/{case['id']}")
                if golden.get("request_sha256") != digest(request):
                    raise ValueError(f"golden request binding mismatch for {arm}/{case['id']}")
            status, response, _ = http_json(host, port, path, request, timeout)
            stream.write(json.dumps({"id": case["id"], "arm": arm, "request": request,
                                     "status": status, "response": response}, allow_nan=False) + "\n")
            stream.flush()
            if status != 200:
                raise RuntimeError(f"preflight {case['id']} HTTP {status}")
            if golden is not None and stable_response_digest(response) != golden["stable_response_sha256"]:
                raise RuntimeError(f"golden full-response mismatch for {arm}/{case['id']}")
            signature = parity_signature(response)
            if case["id"] not in references:
                if arm != "base":
                    raise RuntimeError("first arm must establish base reference")
                references[case["id"]] = signature
            elif signature != references[case["id"]]:
                raise RuntimeError(f"equal-candidate/count/facet parity failed for {case['id']} in {arm}")
            ranking = digest(response["hits"])
            same_arm = ranking_references.setdefault(arm, {})
            if case["id"] in same_arm and ranking != same_arm[case["id"]]:
                raise RuntimeError(f"same-arm ranking changed across preflights: {case['id']} in {arm}")
            same_arm[case["id"]] = ranking


def preflight_arm(cases: list[dict[str, Any]], arm: str, host: str, port: int, path: str,
                  timeout: float, references: dict[str, dict[str, Any]],
                  ranking_references: dict[str, dict[str, str]], output: Path,
                  goldens: dict[str, Any] | None = None) -> None:
    # Uniform prewarming, including separately invoked rotated cycles: every
    # fresh arm receives the same two full passes before the same warmup phase.
    # For the base arm this intentionally means two base passes. Never introduce
    # an extra baseline-only pass on just the first rotated arm.
    baseline_output = output / "base_preflight"
    arm_output = output / "arm_preflight"
    baseline_output.mkdir(exist_ok=False)
    arm_output.mkdir(exist_ok=False)
    preflight(cases, "base", host, port, path, timeout, references, ranking_references, baseline_output, goldens)
    preflight(cases, arm, host, port, path, timeout, references, ranking_references, arm_output, goldens)


def perform_request(case_id: str, request: dict[str, Any], arm: str, index: int,
                    scheduled: float, submitted: float, phase_start: float,
                    host: str, port: int, path: str, timeout: float,
                    reference: dict[str, Any], ranking_reference: str) -> dict[str, Any]:
    """Time response completion before any parity/hash work (independently tested)."""
    began = time.monotonic()
    response_completed = None
    validation_began = None
    record: dict[str, Any] = {"sequence": index, "id": case_id, "arm": arm,
                              "scheduled_offset_seconds": scheduled - phase_start,
                              "queue_delay_ms": (began - scheduled) * 1000,
                              "worker_queue_delay_ms": (began - submitted) * 1000,
                              "generator_schedule_lag_ms": max(0, submitted - scheduled) * 1000,
                              "status": None, "outcome": "error", "parity_ok": False,
                              "same_arm_ranking_ok": False}
    try:
        status, response, size = http_json(host, port, path, request, timeout)
        response_completed = time.monotonic()  # Do not move below validation.
        validation_began = response_completed
        record.update(status=status, response_bytes=size)
        if status != 200:
            record["error"] = f"HTTP {status}"
        else:
            try:
                signature = parity_signature(response)
                record["parity_ok"] = signature == reference
                record["parity_digest"] = digest(signature)
                record["ranking_digest"] = digest(response["hits"])
                record["same_arm_ranking_ok"] = record["ranking_digest"] == ranking_reference
                if not record["parity_ok"]:
                    record["outcome"] = "parity_failure"
                elif not record["same_arm_ranking_ok"]:
                    record["outcome"] = "ranking_failure"
                else:
                    record["outcome"] = "ok"
            except (ValueError, TypeError, KeyError) as error:
                record.update(outcome="validation_failure", error=repr(error))
    except (TimeoutError, socket.timeout) as error:
        record.update(outcome="timeout", error=repr(error))
    except (OSError, http.client.HTTPException, ValueError, TypeError, KeyError) as error:
        record["error"] = repr(error)
    processing_completed = time.monotonic()
    if response_completed is None:
        response_completed = processing_completed
    record["http_latency_ms"] = (response_completed - began) * 1000
    record["scheduled_latency_ms"] = (response_completed - scheduled) * 1000
    record["response_completed_offset_seconds"] = response_completed - phase_start
    record["validation_ms"] = (processing_completed - validation_began) * 1000 if validation_began is not None else 0.0
    return record


def fixed_qps(cases: list[dict[str, Any]], arm: str, host: str, port: int, path: str,
              qps: float, seconds: float, timeout: float, workers: int, max_inflight: int,
              references: dict[str, dict[str, Any]], ranking_references: dict[str, dict[str, str]],
              raw_path: Path) -> dict[str, Any]:
    offered = math.floor(qps * seconds)
    if offered < 1:
        raise ValueError("phase must offer at least one request")
    outstanding = set()
    results: list[dict[str, Any]] = []
    requests = {case["id"]: request_for(case, arm) for case in cases}

    with raw_path.open("w") as raw, concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        def harvest(done: set[concurrent.futures.Future]) -> None:
            for future in done:
                record = future.result()
                results.append(record)
                raw.write(json.dumps(record, sort_keys=True, allow_nan=False) + "\n")
            raw.flush()

        # ThreadPoolExecutor starts workers lazily. Block one readiness task per
        # worker so every thread exists before the schedule epoch, without a
        # CPU-spin loop. Request preparation, file/pool setup, and readiness are
        # not dispatch lag; the measured schedule begins only after all finish.
        ready = threading.Barrier(workers + 1)
        ready_futures = [pool.submit(ready.wait, timeout=30.) for _ in range(workers)]
        ready.wait(timeout=30.)
        for future in ready_futures:
            future.result()
        start = time.monotonic()

        for index in range(offered):
            scheduled = start + index / qps
            delay = scheduled - time.monotonic()
            if delay > 0:
                time.sleep(delay)
            done = {future for future in outstanding if future.done()}
            harvest(done)
            outstanding -= done
            case = cases[index % len(cases)]
            submitted = time.monotonic()
            if len(outstanding) >= max_inflight:
                record = {"sequence": index, "id": case["id"], "arm": arm,
                          "scheduled_offset_seconds": index / qps, "outcome": "admission_drop",
                          "scheduled_latency_ms": None, "http_latency_ms": None,
                          "generator_schedule_lag_ms": max(0, submitted - scheduled) * 1000,
                          "validation_ms": 0.0,
                          "status": None, "parity_ok": False}
                results.append(record)
                raw.write(json.dumps(record, sort_keys=True) + "\n")
            else:
                outstanding.add(pool.submit(perform_request, case["id"], requests[case["id"]],
                                            arm, index, scheduled, submitted, start,
                                            host, port, path, timeout, references[case["id"]],
                                            ranking_references[arm][case["id"]]))
        harvest(outstanding)
    end = time.monotonic()
    successes = [r for r in results if r["outcome"] == "ok"]
    attempted = [r for r in results if r["outcome"] != "admission_drop"]
    completed = [r for r in attempted if r["status"] is not None]
    counts = {key: sum(r["outcome"] == key for r in results)
              for key in ("ok", "error", "timeout", "parity_failure", "ranking_failure", "validation_failure", "admission_drop")}
    last_attempt = max((r["response_completed_offset_seconds"] for r in attempted), default=0.0)
    last_response = max((r["response_completed_offset_seconds"] for r in completed), default=0.0)
    return {"offered_qps": qps, "offer_window_seconds": seconds,
            "elapsed_including_drain_seconds": end - start, "offered_requests": offered,
            "completed_requests": len(completed), "successful_requests": len(successes),
            "finished_attempts": len(attempted),
            "completion_ratio": len(completed) / offered,
            "outcomes": counts, "success_rate": len(successes) / offered,
            "error_rate": (counts["error"] + counts["timeout"] + counts["validation_failure"] + counts["parity_failure"] + counts["ranking_failure"]) / offered,
            "response_validation_pass": not any(counts[k] for k in ("parity_failure", "ranking_failure", "validation_failure")),
            "achieved_success_qps_including_drain": len(successes) / (end - start),
            "response_completion_window_seconds": last_response,
            "attempt_completion_window_seconds": last_attempt,
            "post_attempt_client_drain_seconds": max(0, end - start - last_attempt),
            "successful_scheduled_latency_ms": quantiles([r["scheduled_latency_ms"] for r in successes]),
            "all_completed_scheduled_latency_ms": quantiles([r["scheduled_latency_ms"] for r in completed]),
            "all_finished_attempts_scheduled_latency_ms": quantiles([r["scheduled_latency_ms"] for r in attempted]),
            "successful_http_latency_ms": quantiles([r["http_latency_ms"] for r in successes]),
            "queue_delay_ms": quantiles([r["queue_delay_ms"] for r in attempted]),
            "worker_queue_delay_ms": quantiles([r["worker_queue_delay_ms"] for r in attempted]),
            "generator_schedule_lag_ms": quantiles([r["generator_schedule_lag_ms"] for r in results]),
            "validation_ms": quantiles([r["validation_ms"] for r in attempted]),
            "distinct_scheduled_cases": len({cases[index % len(cases)]["id"] for index in range(offered)}),
            "total_workload_cases": len(cases),
            "all_cases_scheduled": offered >= len(cases),
            "case_schedule_sha256": digest([cases[index % len(cases)]["id"] for index in range(offered)]),
            "valid_zero_error_zero_drop": len(successes) == offered}


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-approved", action="store_true", help="required explicit post-review execution gate")
    p.add_argument("--approval-note", required=True, help="who approved this exact local run and scope")
    p.add_argument("--workload", type=Path, required=True)
    p.add_argument("--golden-file", type=Path, required=True,
                   help="mandatory independently validated release goldens; no nongolden cost/capacity run")
    p.add_argument("--server-command", required=True, help="JSON argv array; shell syntax is never interpreted")
    p.add_argument("--cwd", default=str(Path.cwd()))
    p.add_argument("--endpoint", required=True, help="e.g. http://127.0.0.1:18080/search")
    p.add_argument("--ready-path", default="/health")
    p.add_argument("--ready-timeout", type=float, default=120)
    p.add_argument("--output", type=Path, required=True, help="new directory; existing output is not overwritten")
    p.add_argument("--qps", type=float, required=True, help="reviewed fixed offered QPS; do not infer capacity from a low-rate pilot")
    p.add_argument("--seconds", type=float, default=30)
    p.add_argument("--warmup-seconds", type=float, default=5)
    p.add_argument("--timeout", type=float, default=10)
    p.add_argument("--workers", type=int, default=16)
    p.add_argument("--max-inflight", type=int, default=64)
    p.add_argument("--sample-ms", type=int, default=200)
    p.add_argument("--rounds", type=int, default=1,
                   help="four rounds rotate arm order; each arm restarts the same owned service")
    p.add_argument("--start-round-index", type=int, default=0,
                   help="nonnegative zero-based rotation index; use 1 for a separately invoked second cycle")
    p.add_argument("--evidence-file", action="append", type=Path, default=[],
                   help="additional catalog/config/binary/source file to hash into manifest")
    p.add_argument("--slo-p95-ms", type=float, help="strict upper bound on successful scheduled p95")
    p.add_argument("--slo-p99-ms", type=float, help="strict upper bound on successful scheduled p99")
    p.add_argument("--max-error-rate", type=float, help="strict upper bound, errors/offered; drops additionally fail completion ratio")
    p.add_argument("--min-completion-ratio", type=float, help="inclusive minimum completed/offered, admission drops stay in denominator")
    p.add_argument("--min-success-ratio", type=float, help="inclusive successful/offered threshold; required with other SLO gates")
    p.add_argument("--max-generator-lag-p99-ms", type=float, help="inclusive p99 scheduling-lag limit")
    p.add_argument("--max-generator-lag-max-ms", type=float, help="inclusive worst scheduling-lag limit")
    p.add_argument("--max-client-cpu-cores", type=float, help="strict client+sampler CPU seconds / elapsed seconds limit")
    return p


def arm_order(round_index: int) -> tuple[str, ...]:
    if round_index < 0:
        raise ValueError("round index must be nonnegative")
    return ARMS[round_index % 4:] + ARMS[:round_index % 4]


def freeze_thresholds(args: argparse.Namespace) -> dict[str, float | None] | None:
    names = SLO_REQUIRED
    values = {name: getattr(args, name, None) for name in names}
    if all(value is None for value in values.values()):
        return None  # Accounting-only run: never infer an SLO/capacity pass.
    missing = [name for name in SLO_REQUIRED if values[name] is None]
    if missing:
        raise ValueError(f"partial SLO gate: missing {', '.join(missing)}")
    if any(value is not None and not math.isfinite(value) for value in values.values()):
        raise ValueError("SLO thresholds must be finite")
    if values["slo_p95_ms"] <= 0 or values["slo_p99_ms"] < values["slo_p95_ms"]:
        raise ValueError("SLO latency thresholds require 0 < p95 <= p99")
    if not 0 < values["max_error_rate"] <= 1:
        raise ValueError("strict max-error-rate must be in (0, 1]")
    for name in ("min_completion_ratio", "min_success_ratio"):
        if values[name] is not None and not 0 < values[name] <= 1:
            raise ValueError(f"{name} must be in (0, 1]")
    if values["max_generator_lag_p99_ms"] < 0 or values["max_generator_lag_max_ms"] < values["max_generator_lag_p99_ms"]:
        raise ValueError("generator lag thresholds require 0 <= p99 <= max")
    if values["max_client_cpu_cores"] <= 0:
        raise ValueError("max-client-cpu-cores must be positive")
    return values


def assess_run(summary: dict[str, Any], resources: dict[str, Any], client_cpu_seconds: float,
               thresholds: dict[str, float | None] | None) -> dict[str, Any]:
    accounting_valid = all(resources[key] for key in
                           ("stable_process_tree", "pss_complete", "ownership_complete", "sampler_ok"))
    response_validation_pass = summary["response_validation_pass"]
    strict_zero_failure_pass = summary["valid_zero_error_zero_drop"]
    client_cpu_cores = client_cpu_seconds / summary["elapsed_including_drain_seconds"]
    result: dict[str, Any] = {
        "accounting_valid": accounting_valid,
        "response_validation_pass": response_validation_pass,
        "resource_comparison_valid": accounting_valid and response_validation_pass and strict_zero_failure_pass,
        "strict_zero_error_timeout_drop_pass": strict_zero_failure_pass,
        "slo_evaluated": thresholds is not None,
        "slo_pass": None, "generator_scheduling_pass": None, "client_cpu_pass": None,
        "capacity_valid": False,
        "client_and_sampler_cpu_cores": client_cpu_cores,
        "capacity_scope": "this frozen workload and offered QPS only; not maximum or production capacity",
        "capacity_blockers": [],
    }
    if thresholds is None:
        result["capacity_blockers"].append("SLO/generator thresholds not supplied before execution")
    else:
        latency = summary["successful_scheduled_latency_ms"]
        lag = summary["generator_schedule_lag_ms"]
        checks = {
            "p95_strict": latency["p95"] is not None and latency["p95"] < thresholds["slo_p95_ms"],
            "p99_strict": latency["p99"] is not None and latency["p99"] < thresholds["slo_p99_ms"],
            "error_rate_strict": summary["error_rate"] < thresholds["max_error_rate"],
            "completion_ratio_inclusive": summary["completion_ratio"] >= thresholds["min_completion_ratio"],
            "success_ratio_inclusive": thresholds["min_success_ratio"] is None or summary["success_rate"] >= thresholds["min_success_ratio"],
            "zero_correctness_mismatches": response_validation_pass,
        }
        result["slo_checks"] = checks
        result["slo_pass"] = all(checks.values())
        result["generator_scheduling_pass"] = lag["p99"] is not None and lag["max"] is not None and lag["p99"] <= thresholds["max_generator_lag_p99_ms"] and lag["max"] <= thresholds["max_generator_lag_max_ms"]
        result["client_cpu_pass"] = client_cpu_cores < thresholds["max_client_cpu_cores"]
        if not result["slo_pass"]:
            result["capacity_blockers"].append("frozen SLO failed")
        if not result["generator_scheduling_pass"]:
            result["capacity_blockers"].append("load-generator scheduling lag exceeded frozen limits")
        if not result["client_cpu_pass"]:
            result["capacity_blockers"].append("load-generator/sampler CPU saturation flag")
    if not accounting_valid:
        result["capacity_blockers"].append("resource accounting invalid")
    if not response_validation_pass:
        result["capacity_blockers"].append("response correctness/determinism mismatch")
    if not strict_zero_failure_pass:
        result["capacity_blockers"].append("strict cost/drain guard failed: error, timeout, or drop may leave unaccounted work")
    if not summary["all_cases_scheduled"]:
        result["capacity_blockers"].append("measured schedule did not cover all frozen workload cases")
    result["capacity_valid"] = not result["capacity_blockers"]
    if thresholds is None:
        classification = "not_evaluated_no_frozen_thresholds"
    elif not result["generator_scheduling_pass"] or not result["client_cpu_pass"]:
        classification = "load_generator_or_environment_limited_not_engine_capacity_failure"
    elif not accounting_valid:
        classification = "resource_accounting_invalid"
    elif not response_validation_pass:
        classification = "correctness_or_determinism_failure"
    elif not strict_zero_failure_pass:
        classification = "strict_zero_error_resource_guard_failed"
    elif not summary["all_cases_scheduled"]:
        classification = "workload_coverage_incomplete"
    elif not result["slo_pass"]:
        classification = "slo_failed_at_this_offered_qps"
    else:
        classification = "accepted_at_this_offered_qps_only"
    result["capacity_classification"] = classification
    return result


def comparisons(runs: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Within-round deltas only; never turn incremental ratios into total cost."""
    result = []
    for number in sorted({run["round"] for run in runs}):
        arms = {run["arm"]: run for run in runs if run["round"] == number}
        if set(arms) != set(ARMS):
            continue
        def measures(run: dict[str, Any]) -> dict[str, float | None]:
            return {
                "cpu_ms_per_successful_request": run["resources"]["cpu_ms_per_successful_request"],
                "rss_peak_kib": run["resources"]["aggregate_rss_kib"]["max"],
                "pss_peak_kib": run["resources"]["aggregate_pss_kib"]["max"],
                "scheduled_p95_ms": run["successful_scheduled_latency_ms"]["p95"],
                "scheduled_p99_ms": run["successful_scheduled_latency_ms"]["p99"],
            }
        base = measures(arms["base"])
        for arm in ARMS[1:]:
            arm_values = measures(arms[arm])
            result.append({"round": number, "arm": arm, "baseline_arm": "base",
                           "accounting_valid": arms[arm]["accounting_valid"] and arms["base"]["accounting_valid"],
                           "same_slo_capacity_valid": arms[arm]["capacity_valid"] and arms["base"]["capacity_valid"],
                           "metrics": {name: {"base": base[name], "feature": value,
                                               "delta": value - base[name] if value is not None and base[name] is not None else None,
                                               "ratio": value / base[name] if value is not None and base[name] else None}
                                       for name, value in arm_values.items()},
                           "interpretation": "incremental same-engine serving comparison, not Athos total-cost ratio"})
    return result


def main() -> int:
    args = parser().parse_args()
    if not args.run_approved:
        raise SystemExit("NOT RUN: review required; --run-approved intentionally absent")
    if args.output.exists():
        raise FileExistsError(f"refusing existing output directory: {args.output}")
    for name in ("qps", "seconds", "warmup_seconds", "timeout", "ready_timeout"):
        value = getattr(args, name)
        if not math.isfinite(value) or value <= 0:
            raise SystemExit(f"--{name.replace('_', '-')} must be positive and finite")
    if args.workers < 1 or args.max_inflight < args.workers or args.sample_ms < 50 or args.rounds < 1 or args.start_round_index < 0:
        raise SystemExit("workers>=1, max-inflight>=workers, sample-ms>=50, rounds>=1, start-round-index>=0 required")
    try:
        thresholds = freeze_thresholds(args)
    except ValueError as error:
        raise SystemExit(str(error)) from error
    command = json.loads(args.server_command)
    if not isinstance(command, list) or not command or not all(isinstance(x, str) and x for x in command):
        raise SystemExit("--server-command must be a nonempty JSON array of nonempty strings")
    host, port, path = endpoint_parts(args.endpoint)
    if not args.ready_path.startswith("/") or args.ready_path.startswith("//"):
        raise SystemExit("--ready-path must be an absolute path on the same loopback server")
    workload = load_workload(args.workload)
    executable = Path(command[0])
    if not executable.is_absolute():
        executable = Path(args.cwd) / executable if "/" in command[0] else Path(shutil.which(command[0]) or command[0])
    if not executable.is_file():
        raise SystemExit("server executable is missing; runner never builds it")
    evidence = {str(f.resolve()): file_digest(f) for f in [args.workload, Path(__file__), executable, *args.evidence_file]}
    goldens = None
    if args.golden_file is not None:
        goldens = load_goldens(args.golden_file, workload["cases"], file_digest(args.workload),
                               file_digest(executable), {file_digest(f) for f in args.evidence_file})
        evidence[str(args.golden_file.resolve())] = file_digest(args.golden_file)
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = {"runner_version": VERSION, "approval_note": args.approval_note,
                "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "endpoint": args.endpoint, "command": command, "cwd": args.cwd,
                "workload_metadata": workload.get("metadata", {}), "case_count": len(workload["cases"]),
                "evidence_sha256": evidence, "python_version": sys.version,
                "parameters": {key: getattr(args, key) for key in ("qps", "seconds", "warmup_seconds", "workers", "max_inflight", "timeout", "sample_ms", "rounds", "start_round_index")},
                "frozen_slo_and_generator_thresholds": thresholds,
                "golden_file": str(args.golden_file.resolve()) if args.golden_file else None,
                "independently_validated_release_goldens_used": goldens is not None,
                "golden_oracle_summary_sha256": goldens["oracle_summary_sha256"] if goldens else None,
                "scope": "same engine/analyzer/catalog HTTP feature-off vs feature-on incremental serving cost",
                "not_claimed": ["80% Athos capability coverage", "20% Athos total cost", "production traffic representativeness", "search relevance parity"],
                "excluded_costs": ["ETL/catalog ingestion preparation", "personal factor/model/profile generation", "event tracking", "merchant UI/control plane", "deployment operations", "build/index construction outside measured serving window", "load generator and sampler CPU"],
                "status": "running", "runs": []}
    write_json(args.output / "manifest.json", manifest)
    references: dict[str, dict[str, Any]] = {}
    ranking_references: dict[str, dict[str, str]] = {}
    try:
        for round_index in range(args.start_round_index, args.start_round_index + args.rounds):
            # Balanced rotation after a base-first establishing round.
            order = arm_order(round_index)
            for arm in order:
                out = args.output / f"round_{round_index + 1:02d}_{arm}"
                out.mkdir()
                for evidence_path, expected_hash in evidence.items():
                    if file_digest(Path(evidence_path)) != expected_hash:
                        raise RuntimeError(f"frozen input changed before {arm}: {evidence_path}")
                if port_open(host, port):
                    raise RuntimeError("experiment port is already occupied; refusing to contact or stop existing service")
                service = OwnedService(command, args.cwd, out, args.sample_ms)
                try:
                    service.start()
                    wait_ready(service, host, port, args.ready_path, args.ready_timeout)
                    preflight_arm(workload["cases"], arm, host, port, path, args.timeout, references, ranking_references, out, goldens)
                    write_json(args.output / "arm_ranking_digests.json", ranking_references)
                    warmup = fixed_qps(workload["cases"], arm, host, port, path, args.qps,
                                       args.warmup_seconds, args.timeout, args.workers,
                                       args.max_inflight, references, ranking_references, out / "warmup_requests.jsonl")
                    write_json(out / "warmup_summary.json", warmup)
                    if not warmup["valid_zero_error_zero_drop"]:
                        raise RuntimeError("warmup error/drop/parity failure; measured load not started")
                    base_resources = service.begin_measurement()
                    client_cpu_start = time.process_time()
                    summary = fixed_qps(workload["cases"], arm, host, port, path, args.qps,
                                        args.seconds, args.timeout, args.workers,
                                        args.max_inflight, references, ranking_references, out / "requests.jsonl")
                    client_cpu_seconds = time.process_time() - client_cpu_start
                    resources = service.end_measurement(base_resources)
                    resources["cpu_ms_per_offered_request"] = resources["cpu_seconds"] * 1000 / summary["offered_requests"]
                    resources["cpu_ms_per_successful_request"] = resources["cpu_seconds"] * 1000 / summary["successful_requests"] if summary["successful_requests"] else None
                    summary.update(arm=arm, round=round_index + 1, resources=resources,
                                   load_generator_and_sampler_cpu_seconds_excluded=client_cpu_seconds)
                    summary.update(assess_run(summary, resources, client_cpu_seconds, thresholds))
                    write_json(out / "summary.json", summary)
                    manifest["runs"].append(summary)
                    write_json(args.output / "manifest.json", manifest)
                    if not summary["resource_comparison_valid"]:
                        raise RuntimeError("cost measurement invalid: error/timeout/drop may be undrained, correctness mismatch, or incomplete accounting; see summary")
                finally:
                    service.close()
        write_json(args.output / "baseline_parity_signatures.json", references)
        write_json(args.output / "within_round_comparisons.json", comparisons(manifest["runs"]))
        manifest["status"] = "completed"
        manifest["all_runs_accounting_valid"] = all(run["accounting_valid"] for run in manifest["runs"])
        manifest["all_runs_capacity_valid"] = bool(manifest["runs"]) and all(run["capacity_valid"] for run in manifest["runs"])
        manifest["completed_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        write_json(args.output / "manifest.json", manifest)
        print(json.dumps({"status": "completed", "manifest": str(args.output / "manifest.json"),
                          "valid_runs": len(manifest["runs"])}))
        return 0
    except BaseException as error:
        manifest["status"] = "failed"
        manifest["error"] = f"{type(error).__name__}: {error}"
        write_json(args.output / "manifest.json", manifest)
        raise


if __name__ == "__main__":
    raise SystemExit(main())
