#!/usr/bin/env python3
"""Phase 6B (Issue #23 follow-on, Issue #21 Phase 6 scale ladder):
generate a CONTROLLED STRESS CATALOG by replicating the real WANDS
catalog.jsonl N times.

This is explicitly NOT a fabricated/organic-growth catalog. Every record
in the output is a byte-identical copy of a real WANDS record (same
title, category, attributes, ratings -- see
docs/research/artifacts/p6a_dataset_acquisition/manifest.json for what
WANDS' 42,994 real products actually contain), except for `id`, which is
suffixed per replica (`-r0`, `-r1`, ...) so Solr's unique-key requirement
and the Rust ingestion's per-line identity both hold.

What this DOES hold constant, by construction: facet CARDINALITY (the
number of distinct category/color/style/... values) is unchanged by
replication -- a real 10x-larger organic catalog would very likely have
*more* distinct categories/colors, not just 10x deeper buckets of the
same ones. This script isolates one axis only: candidate-set SIZE per
existing real bucket, holding per-candidate attribute-map complexity and
facet cardinality fixed. See PHASE6B_LOG.md for why this isolation is the
point (falsifying/confirming Phase 6A's "per-candidate attribute
complexity, not raw scale" explanation of the facet-scan crossover
shift) rather than a claim about real catalog growth.

Usage: python3 scripts/datasets/replicate_wands_scale.py <multiplier> [out_path]
  multiplier: positive integer K (e.g. 2, 5, 10, 20)
  out_path: defaults to dataset_cache/wands_scale/catalog_{K}x.jsonl
"""
import json
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parents[2] / "dataset_cache" / "wands" / "catalog.jsonl"


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    k = int(sys.argv[1])
    assert k >= 1, "multiplier must be >= 1"
    out_path = (
        Path(sys.argv[2])
        if len(sys.argv) > 2
        else Path(__file__).resolve().parents[2]
        / "dataset_cache"
        / "wands_scale"
        / f"catalog_{k}x.jsonl"
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)

    with open(SRC) as f:
        lines = f.readlines()
    real_n = len(lines)

    written = 0
    with open(out_path, "w") as out:
        for replica in range(k):
            for line in lines:
                record = json.loads(line)
                record["id"] = f"{record['id']}-r{replica}"
                out.write(json.dumps(record) + "\n")
                written += 1

    print(
        f"replicated {real_n} real WANDS products x{k} = {written} rows -> {out_path}\n"
        f"CONTROLLED STRESS CATALOG, not organic data: facet cardinality is "
        f"unchanged from the real {real_n}-product catalog, only per-bucket "
        f"candidate-set size scales."
    )


if __name__ == "__main__":
    main()
