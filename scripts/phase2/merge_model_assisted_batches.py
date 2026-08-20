#!/usr/bin/env python3
"""Issue #9, P2-E10: merge the 5 independent, parallel model-assisted
labeling batches (each 100 real candidates from
dataset_cache/export/brand_query_relevant_sample.jsonl, split via
`split -l 100 -d --additional-suffix=.jsonl`) into one ordered JSON array,
verifying coverage and order against the original sample file before
writing anything.

This is pure aggregation (concatenate + verify), no judgment of its own --
the actual classification work was five separate agent runs, each reading
its own 100-candidate batch file and a shared rubric
(docs/research/brand-adjudication-rubric.md), independently.

Usage: python3 scripts/phase2/merge_model_assisted_batches.py \\
    <sample.jsonl> <batch0.json> <batch1.json> ... <output.json>
"""
import json
import sys


def main() -> None:
    if len(sys.argv) < 4:
        print(
            "usage: merge_model_assisted_batches.py <sample.jsonl> <batch...> <output.json>",
            file=sys.stderr,
        )
        sys.exit(1)

    sample_path = sys.argv[1]
    output_path = sys.argv[-1]
    batch_paths = sys.argv[2:-1]

    with open(sample_path) as f:
        sample_brands = [json.loads(line)["brand_normalized"] for line in f]

    merged = []
    for path in batch_paths:
        with open(path) as f:
            merged.extend(json.load(f))

    merged_brands = [row["brand_normalized"] for row in merged]
    assert len(merged) == len(sample_brands), (
        f"merged {len(merged)} candidates, sample has {len(sample_brands)}"
    )
    assert merged_brands == sample_brands, (
        "merged batch order does not match the original sample file order"
    )
    allowed_labels = {
        "canonical_known_entity_or_alias",
        "legitimate_new_entity",
        "lexical_only_not_structural",
        "ambiguous_insufficient_evidence",
        "junk_malformed_wrong_field",
    }
    for row in merged:
        assert row["label"] in allowed_labels, f"invalid label: {row['label']!r}"

    with open(output_path, "w") as f:
        json.dump(merged, f, indent=2)

    print(f"merged {len(merged)} candidates from {len(batch_paths)} batches -> {output_path}")
    from collections import Counter

    for label, count in Counter(row["label"] for row in merged).most_common():
        print(f"  {label}: {count}")


if __name__ == "__main__":
    main()
