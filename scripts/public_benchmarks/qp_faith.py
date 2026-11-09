#!/usr/bin/env python3
"""Pinned, offline QP-Faith R1.2 replay. Never imports a planner or calls an LLM."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "benchmarks/manifests/qp_faith_public.json"
SLOTS = ("product_type", "color", "material", "style", "brand", "price_range")


def blob_sha(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def read_rows(path):
    rows = []
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except ValueError as exc:
            raise ValueError(f"{path}:{number}: invalid JSON") from exc
        if not isinstance(row, dict):
            raise ValueError(f"{path}:{number}: row must be an object")
        rows.append(row)
    return rows


def validate_rows(rows, probe):
    expected = {r["query"]: r for r in probe}
    if len(expected) != len(probe) or not expected:
        raise ValueError("probe has duplicate queries or is empty")
    seen = set()
    for row in rows:
        query = row.get("query")
        if query not in expected or query in seen:
            raise ValueError(f"unknown or duplicate query: {query!r}")
        seen.add(query)
        for field in ("family", "known_slots"):
            if row.get(field) != expected[query].get(field):
                raise ValueError(f"{query!r}: mismatched {field}")
        if "plan" not in row:
            raise ValueError(f"{query!r}: missing plan")
    if seen != set(expected):
        raise ValueError(f"incomplete query coverage: {len(seen)}/{len(expected)}")


def score(rows, emitted):
    entity = [r for r in rows if r["family"] == "A_entity"]
    if not entity:
        raise ValueError("no entity rows")
    emissions = {s: round(sum(emitted(r["plan"], s) for r in entity) / len(entity), 3)
                 for s in SLOTS}
    return {"category_routing_rate": emissions["product_type"],
            "emission": emissions, "n_entity": len(entity)}


def verify(source, lock):
    for path, sha in lock["files"].items():
        if blob_sha((source / path).read_bytes()) != sha:
            raise ValueError(f"source checksum mismatch: {path}")


def fetch(source, lock):
    # Only immutable public GitHub files in the checked-in lock. No model runners.
    for path, sha in lock["files"].items():
        target = source / path
        if target.exists():
            if blob_sha(target.read_bytes()) != sha:
                raise ValueError(f"refusing to replace modified file: {path}")
            continue
        url = f'https://raw.githubusercontent.com/{lock["repository"]}/{lock["revision"]}/{path}'
        with urllib.request.urlopen(url, timeout=60) as response:
            data = response.read()
        if blob_sha(data) != sha:
            raise ValueError(f"download checksum mismatch: {path}")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)


def replay(source, lock):
    verify(source, lock)
    spec = importlib.util.spec_from_file_location("qp_faith_official", source / "scripts/score_conditions.py")
    official = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(official)
    probe = read_rows(source / "data/probe/probe.jsonl")
    expected = json.loads((source / "results/score_conditions.json").read_text())
    cells = {}
    invalid_plans = {}
    for condition, tags in expected["by_condition"].items():
        cells[condition] = {}
        for tag in tags:
            path = f"results/cond-{condition}-{tag}__probe_plans.jsonl"
            rows = read_rows(source / path)
            validate_rows(rows, probe)
            if any(r.get("condition") != condition for r in rows):
                raise ValueError(f"condition mismatch: {path}")
            cells[condition][tag] = score(rows, official.emitted)
            invalid_plans[path] = sum(not isinstance(r["plan"], dict) for r in rows)
    if cells != expected["by_condition"]:
        raise ValueError("replayed cells differ from published scores")
    # Also execute the complete unmodified official scorer in a temporary output dir.
    with tempfile.TemporaryDirectory() as tmp:
        result_dir = Path(tmp)
        for path in lock["files"]:
            if path.startswith("results/cond-"):
                (result_dir / Path(path).name).write_bytes((source / path).read_bytes())
        official.RES = str(result_dir)
        official.main()
        if json.loads((result_dir / "score_conditions.json").read_text()) != expected:
            raise ValueError("official scorer differs from published scores")
    return {
        "status": "cached_public_replay_passed",
        "upstream_revision": lock["revision"],
        "query_count": len(probe),
        "entity_query_count": sum(r["family"] == "A_entity" for r in probe),
        "cell_count": sum(len(t) for t in cells.values()),
        "all_cells_equal_published": True,
        "non_object_plans_by_file": invalid_plans,
        "by_condition": cells,
        "limits": [
            "Cached generations only; not a fresh behavioral replication.",
            "R1.2 prompt ladder only; not the full paper or aggressive expansion arm.",
            "Non-object plans follow upstream emission semantics, not proof of safe behavior.",
            "Procedural motif routing is not retrieval quality or organic-query prevalence.",
            "No current commerce-core, WANDS or ESCI retrieval results produced."
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--fetch", action="store_true", help="fetch pinned public files; otherwise fully offline")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    lock = json.loads(LOCK.read_text())
    if args.fetch:
        fetch(args.source, lock)
    report = replay(args.source, lock)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"PASS: {report['cell_count']} cached cells; {report['query_count']} queries per cell")


if __name__ == "__main__":
    main()
