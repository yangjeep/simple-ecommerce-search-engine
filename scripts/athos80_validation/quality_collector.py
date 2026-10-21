#!/usr/bin/env python3
"""Collect 480 pure-query HTTP outputs after performance; no oracle or benchmark.

Requires a fresh owned loopback service and frozen binary/catalog/cases hashes.
No build, production call, retry, proxy, redirect follow, or query rewrite occurs.
"""
from __future__ import annotations

import argparse
from collections import Counter
import copy
import http.client
import ipaddress
import json
import math
from pathlib import Path
import shutil
import socket
import sys
import time
from typing import Any

import resource_runner as reviewed


VERSION = "athos80-quality-collector-v1"
HOST = "127.0.0.1"
EXPECTED_CASE_COUNT = 480


def load_cases(path: Path) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    package = json.loads(path.read_text(), object_pairs_hook=reviewed.unique_json_object)
    cases = package.get("cases")
    if not isinstance(cases, list) or len(cases) != EXPECTED_CASE_COUNT:
        raise ValueError("quality input must contain exactly 480 cases")
    ids = [case.get("id") for case in cases]
    if any(not isinstance(case_id, str) for case_id in ids) or len(set(ids)) != EXPECTED_CASE_COUNT:
        raise ValueError("quality IDs must contain exactly 480 unique strings")
    for case in cases:
        case_id = case["id"]
        query_id = case_id[len("wands-"):] if case_id.startswith("wands-") else ""
        # Official WANDS IDs are noncontiguous (including 487). The pinned
        # cases-file SHA, not a fabricated 0..479 range, fixes the exact set.
        if not query_id or not query_id.isascii() or not query_id.isdigit() or str(int(query_id)) != query_id:
            raise ValueError(f"noncanonical nonnegative WANDS query ID: {case_id}")
        if case.get("query_id") != query_id:
            raise ValueError(f"query_id binding mismatch for {case['id']}")
        request = case.get("request")
        if not isinstance(request, dict) or not isinstance(request.get("q"), str):
            raise ValueError(f"missing exact query request for {case['id']}")
        if any((case.get("merch_rules"), case.get("personal_signals"),
                request.get("rules"), request.get("personal_signals"))):
            raise ValueError("quality collection accepts pure base requests only")
        reviewed.canonical(request)  # Validate without normalizing or rewriting.
    return package.get("metadata", {}), cases


def frozen_hashes(paths: list[Path]) -> dict[str, str]:
    return {str(path.resolve()): reviewed.file_digest(path) for path in paths}


def verify_expected_inputs(binary: Path, catalog: Path, cases: Path,
                           expected_binary: str, expected_catalog: str,
                           expected_cases: str) -> dict[str, str]:
    result = {}
    for name, path, expected in (("binary", binary, expected_binary),
                                 ("catalog", catalog, expected_catalog),
                                 ("cases", cases, expected_cases)):
        if not reviewed.valid_sha256(expected):
            raise ValueError(f"invalid expected {name} SHA256")
        observed = reviewed.file_digest(path)
        if observed != expected:
            raise ValueError(f"frozen {name} SHA256 mismatch")
        result[f"{name}_sha256"] = observed
    return result


def http_call(host: str, port: int, path: str, request: dict[str, Any] | None,
              timeout: float) -> tuple[int, Any]:
    if not ipaddress.ip_address(host).is_loopback or not 1 <= port <= 65535:
        raise ValueError("only literal loopback HTTP is allowed")
    # HTTPConnection ignores ambient proxy settings and does not follow 3xx.
    connection = http.client.HTTPConnection(host, port, timeout=timeout)
    try:
        body = reviewed.canonical(request) if request is not None else None
        connection.request("POST" if request is not None else "GET", path, body=body,
                           headers={"Content-Type": "application/json", "Connection": "close"})
        reply = connection.getresponse()
        raw = reply.read()
        try:
            def reject_nonfinite(token: str) -> None:
                raise ValueError(f"nonfinite JSON token {token}")
            response = json.loads(raw, parse_constant=reject_nonfinite)
        except (ValueError, UnicodeDecodeError):
            response = {"_non_json_body": raw.decode(errors="replace")}
        return reply.status, response
    finally:
        connection.close()


def wait_owned_ready(service: reviewed.OwnedService, port: int, timeout: float) -> Any:
    deadline = time.monotonic() + timeout
    last_error = "owned listener not ready"
    while time.monotonic() < deadline:
        if service.proc is not None and service.proc.poll() is not None:
            raise RuntimeError(f"owned server exited {service.proc.returncode}")
        if service.owns_listener(HOST, port):
            if service.ownership_violations:
                raise RuntimeError("owned service has an unmarked descendant")
            try:
                status, body = http_call(HOST, port, "/health", None, 1.)
                if status == 200:
                    return body
                last_error = f"health HTTP {status}"
            except (OSError, http.client.HTTPException) as error:
                last_error = repr(error)
        time.sleep(.1)
    raise TimeoutError(last_error)


def collect(cases: list[dict[str, Any]], service: reviewed.OwnedService, port: int,
            timeout: float, output: Path) -> dict[str, Any]:
    statuses: Counter[str] = Counter()
    recorded_ids = []
    with output.open("x") as stream:
        for case in cases:
            request = copy.deepcopy(case["request"])
            status = None
            try:
                # Recheck before every request; never contact a replacement listener.
                if not service.owns_listener(HOST, port):
                    raise RuntimeError("owned listener unavailable; no request sent")
                status, response = http_call(HOST, port, "/search", request, timeout)
            except (OSError, http.client.HTTPException, ValueError, RuntimeError) as error:
                response = {"_collector_error": type(error).__name__, "message": str(error)}
            record = {"id": case["id"], "arm": "base", "request": request,
                      "status": status, "response": response}
            stream.write(json.dumps(record, sort_keys=True, allow_nan=False) + "\n")
            stream.flush()
            recorded_ids.append(case["id"])
            statuses[str(status) if status is not None else "transport_error"] += 1
    expected_ids = [case["id"] for case in cases]
    return {"record_count": len(recorded_ids), "recorded_ids": recorded_ids,
            "exact_input_order_and_ids_retained": recorded_ids == expected_ids,
            "all_480_unique_ids_retained": len(recorded_ids) == EXPECTED_CASE_COUNT and len(set(recorded_ids)) == EXPECTED_CASE_COUNT,
            "http_status_counts": dict(statuses), "http_success_count": statuses.get("200", 0),
            "http_error_count": sum(count for status, count in statuses.items() if status not in ("200", "transport_error")),
            "transport_error_count": statuses.get("transport_error", 0)}


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-approved", action="store_true")
    p.add_argument("--approval-note", required=True, help="specific operator approval for this local collection")
    p.add_argument("--binary", type=Path, required=True)
    p.add_argument("--catalog", type=Path, required=True)
    p.add_argument("--cases", type=Path, required=True)
    p.add_argument("--expect-binary-sha256", required=True)
    p.add_argument("--expect-catalog-sha256", required=True)
    p.add_argument("--expect-cases-sha256", required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--timeout", type=float, default=30.)
    p.add_argument("--ready-timeout", type=float, default=180.)
    return p


def main() -> int:
    args = parser().parse_args()
    if not args.run_approved:
        raise SystemExit("NOT RUN: explicit local collection approval required")
    if args.output.exists():
        raise FileExistsError("fresh output directory required; prior evidence is never overwritten")
    if not 1 <= args.port <= 65535 or not all(math.isfinite(x) and x > 0 for x in (args.timeout, args.ready_timeout)):
        raise ValueError("valid port and positive finite timeouts required")
    verified = verify_expected_inputs(args.binary, args.catalog, args.cases,
                                      args.expect_binary_sha256, args.expect_catalog_sha256,
                                      args.expect_cases_sha256)
    metadata, cases = load_cases(args.cases)
    relevant = [args.binary, args.catalog, args.cases, Path(__file__), Path(reviewed.__file__)]
    pre_hashes = frozen_hashes(relevant)
    # Refuse occupied ports without sending even a health request to that process.
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind((HOST, args.port))
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / "sources").mkdir()
    for source in (Path(__file__), Path(reviewed.__file__)):
        shutil.copy2(source, args.output / "sources" / source.name)
    shutil.copy2(args.cases, args.output / "quality_cases.json")
    command = [str(args.binary.resolve()), str(args.catalog.resolve()), "--addr", f"{HOST}:{args.port}",
               "--workers", "1", "--bm25"]
    manifest: dict[str, Any] = {"collector_version": VERSION, **verified,
                               "approval_note": args.approval_note, "command": command,
                               "purpose": "480 pure-query quality outputs only; no Python oracle/performance claim",
                               "metadata": metadata, "pre_hashes": pre_hashes, "status": "running",
                               "responses_sha256": None, "frozen_inputs_unchanged": False,
                               "owned_listener_remaining": None, "collection_complete": False}
    reviewed.write_json(args.output / "manifest.json", manifest)
    service = reviewed.OwnedService(command, str(args.binary.resolve().parent), args.output, 1000)
    fatal_errors = []
    try:
        service.start()
        manifest["pid"] = service.proc.pid if service.proc else None
        manifest["health"] = wait_owned_ready(service, args.port, args.ready_timeout)
        manifest.update(collect(cases, service, args.port, args.timeout, args.output / "responses.jsonl"))
        manifest["collection_complete"] = manifest["all_480_unique_ids_retained"] and manifest["exact_input_order_and_ids_retained"]
    except BaseException as error:
        fatal_errors.append(f"{type(error).__name__}: {error}")
    finally:
        try:
            service.close()
        except BaseException as error:
            fatal_errors.append(f"cleanup {type(error).__name__}: {error}")
        manifest["owned_listener_remaining"] = service.owns_listener(HOST, args.port)
        try:
            manifest["post_hashes"] = frozen_hashes(relevant)
            manifest["frozen_inputs_unchanged"] = manifest["post_hashes"] == pre_hashes
        except OSError as error:
            fatal_errors.append(f"post-hash {type(error).__name__}: {error}")
        response_path = args.output / "responses.jsonl"
        if response_path.exists():
            manifest["responses_sha256"] = reviewed.file_digest(response_path)
        if not manifest["frozen_inputs_unchanged"]:
            fatal_errors.append("frozen source/input/binary changed")
        if manifest["owned_listener_remaining"]:
            fatal_errors.append("owned listener remains after cleanup")
        if not manifest["collection_complete"]:
            fatal_errors.append("not all 480 exact case IDs were collected")
        manifest["fatal_errors"] = fatal_errors
        manifest["status"] = "completed" if not fatal_errors else "failed"
        manifest["http_errors_are_retained_not_retried_or_dropped"] = True
        reviewed.write_json(args.output / "manifest.json", manifest)
    print(json.dumps({"status": manifest["status"], "manifest": str(args.output / "manifest.json"),
                      "record_count": manifest.get("record_count", 0),
                      "http_status_counts": manifest.get("http_status_counts", {})}))
    return 0 if not fatal_errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
