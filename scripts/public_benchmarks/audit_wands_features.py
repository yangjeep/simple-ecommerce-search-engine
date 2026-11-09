#!/usr/bin/env python3
"""Audit observed public-probe losses; never change labels, catalogs or filters."""
import argparse
import collections
import csv
import hashlib
import json
from pathlib import Path
import re
import sys

FOCUS = {"1508", "3473", "3594", "3650", "5728", "12994", "38821"}


def norm(value):
    return re.sub(r"[^a-z0-9 ]", " ", str(value).lower()).strip()


def features(raw):
    """Retain every original key/value pair, including repeated keys."""
    return [(k.strip(), v.strip()) for item in raw.split("|") if ":" in item
            for k, v in [item.split(":", 1)]]


def audit(raw_path, prepared_path, probe_path, native_path):
    csv.field_size_limit(sys.maxsize)
    with raw_path.open(newline="") as stream:
        raw = {r["product_id"]: r for r in csv.DictReader(stream, delimiter="\t")}
    prepared = {r["id"]: r for line in prepared_path.read_text().splitlines()
                if (r := json.loads(line))}
    probe = {r["query"]: r for line in probe_path.read_text().splitlines()
             if (r := json.loads(line))}
    native = json.loads(native_path.read_text())
    groups, focus = [], []
    for row in native["rows"]:
        if row["family"] != "B_attr":
            continue
        ref = probe[row["query"]]
        relevant = {str(pid) for pid in ref["relevant_pids"]}
        retained = set(row["retained_relevant_pids"])
        if len(retained) != row["retained_relevant_products"] or not retained <= relevant:
            raise ValueError("native retained IDs/count mismatch")
        enum_constraints = row["hard_enum_constraints"]
        reasons = collections.Counter()
        exact_prediction_disagreements = []
        for pid in sorted(relevant, key=int):
            source, product = raw[pid], prepared[pid]
            pairs = features(source.get("product_features", ""))
            failed = [c for c in enum_constraints if product.get(c["attribute"]) != c["value"]]
            # This predicts only enum predicates; native decisions remain the authority.
            if (not failed) != (pid in retained):
                exact_prediction_disagreements.append(pid)
            product_reasons = []
            for c in failed:
                vals = [v for k, v in pairs if k.lower() == c["attribute"].lower()]
                if product.get(c["attribute"]) is None:
                    reason = "prepared_attribute_absent"
                elif any(norm(v) == norm(c["value"]) for v in vals):
                    reason = "matching_raw_value_not_prepared_exact_value"
                elif any(norm(c["value"]) in norm(v).split() for v in vals):
                    reason = "raw_token_membership_not_exact_enum"
                else:
                    reason = "other_enum_mismatch"
                product_reasons.append(reason)
            if pid not in retained:
                reasons.update(set(product_reasons) or ["not_explained_by_enum_predicates"])
            if pid in FOCUS and row["query"].endswith(" doormats"):
                focus.append({
                    "product_id": pid, "query": row["query"],
                    "title": source["product_name"],
                    "raw_product_class": source["product_class"],
                    "raw_color_features": [[k, v] for k, v in pairs if k.lower() == "color"],
                    "prepared_color": product.get("color"),
                    "actual_hard_enum_constraints": enum_constraints,
                    "actual_native_retained": pid in retained,
                    "published_relevant": True,
                    "failed_enum_predicates": failed,
                    "exclusion_reasons": product_reasons,
                })
        groups.append({
            "query": row["query"], "reference_count": len(relevant),
            "actual_retained_count": len(retained),
            "exclusion_reason_counts": dict(reasons),
            "enum_only_prediction_disagreement_ids": exact_prediction_disagreements,
        })
    return {
        "status": "observational_audit", "native_summary": native["summary"],
        "input_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                         for p in (raw_path, prepared_path, probe_path, native_path)},
        "queries": groups, "focus_products": focus,
        "limits": [
            "Feature bags are product-level metadata, not verified variant identities.",
            "Reasons describe representation differences, not corrections to ground truth.",
            "Enum-only agreement is tested only within published relevant sets.",
            "Native decisions come from real compiler/executor, not this Python predictor.",
        ],
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("raw", type=Path)
    p.add_argument("prepared", type=Path)
    p.add_argument("probe", type=Path)
    p.add_argument("native", type=Path)
    p.add_argument("output", type=Path)
    a = p.parse_args()
    result = audit(a.raw, a.prepared, a.probe, a.native)
    a.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"queries": result["queries"], "focus_row_count": len(result["focus_products"])}))


if __name__ == "__main__":
    main()
