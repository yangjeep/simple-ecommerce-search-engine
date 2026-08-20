#!/usr/bin/env python3
"""Issue #9: reconcile the three independent brand-adjudication labeling
passes into final ground truth, per the protocol in
docs/research/brand-adjudication-rubric.md's "Labeling protocol" section:

- 3/3 agree  -> that label, high confidence.
- 2/3 agree  -> the majority label, flagged lower-confidence.
- 0/3 agree (three distinct labels) -> ground truth is itself
  `ambiguous_insufficient_evidence`, flagged no_majority -- genuine
  adjudication difficulty is evidence, not noise to be resolved by fiat.

Deterministic: pure aggregation over the three fixed input files, no
randomness, no model calls. Reads the three independent pass outputs plus
the original corpus (for bucket/occurrence_count), writes one reconciled
JSONL ground-truth file.

Usage: python3 scripts/phase2/reconcile_brand_adjudication.py \\
    <pass1.json> <pass2.json> <pass3.json> <corpus.jsonl> <output.jsonl>
"""
import json
import sys
from collections import Counter


def load_pass(path):
    with open(path) as f:
        data = json.load(f)
    by_brand = {}
    for row in data:
        by_brand[row["brand_normalized"]] = row
    return by_brand


def reconcile(label1, label2, label3):
    counts = Counter([label1, label2, label3])
    most_common, count = counts.most_common(1)[0]
    if count == 3:
        return most_common, "unanimous", 3
    if count == 2:
        return most_common, "majority", 2
    return "ambiguous_insufficient_evidence", "no_majority", 1


def main():
    pass1_path, pass2_path, pass3_path, corpus_path, output_path = sys.argv[1:6]

    pass1 = load_pass(pass1_path)
    pass2 = load_pass(pass2_path)
    pass3 = load_pass(pass3_path)

    corpus = {}
    with open(corpus_path) as f:
        for line in f:
            row = json.loads(line)
            corpus[row["brand_normalized"]] = row

    brands = list(corpus.keys())
    assert set(brands) == set(pass1.keys()) == set(pass2.keys()) == set(pass3.keys()), (
        "the three passes and the corpus must cover exactly the same 209 candidates"
    )

    confidence_counts = Counter()
    label_counts = Counter()
    out_rows = []
    for brand in brands:
        c = corpus[brand]
        l1 = pass1[brand]["label"]
        l2 = pass2[brand]["label"]
        l3 = pass3[brand]["label"]
        final_label, confidence, agreement = reconcile(l1, l2, l3)
        confidence_counts[confidence] += 1
        label_counts[final_label] += 1
        out_rows.append(
            {
                "brand_normalized": brand,
                "bucket": c["bucket"],
                "real_occurrence_count": c["real_occurrence_count"],
                "final_label": final_label,
                "confidence": confidence,
                "agreement_count": agreement,
                "pass_labels": {"pass1": l1, "pass2": l2, "pass3": l3},
            }
        )

    out_rows.sort(key=lambda r: (r["bucket"], -r["real_occurrence_count"], r["brand_normalized"]))
    with open(output_path, "w") as f:
        for row in out_rows:
            f.write(json.dumps(row) + "\n")

    total = len(out_rows)
    print(f"reconciled {total} candidates -> {output_path}")
    print("\nconfidence tier breakdown:")
    for tier, n in confidence_counts.most_common():
        print(f"  {tier}: {n} ({100 * n / total:.1f}%)")
    print("\nfinal label breakdown:")
    for label, n in label_counts.most_common():
        print(f"  {label}: {n} ({100 * n / total:.1f}%)")

    # Pairwise raw agreement (Cohen's-kappa-adjacent, but reported as plain
    # percent agreement -- simplest honest number, not dressed up as a
    # formal statistic this small a sample doesn't support well).
    pairs = [("pass1", "pass2", pass1, pass2), ("pass1", "pass3", pass1, pass3), ("pass2", "pass3", pass2, pass3)]
    print("\npairwise raw agreement:")
    for name_a, name_b, a, b in pairs:
        agree = sum(1 for brand in brands if a[brand]["label"] == b[brand]["label"])
        print(f"  {name_a} vs {name_b}: {agree}/{total} ({100 * agree / total:.1f}%)")


if __name__ == "__main__":
    main()
