#!/usr/bin/env python3
"""Issue #9 follow-up: build a real, tractably-sized sample of brand
vocabulary that could actually change a real query's outcome, for an
end-to-end FIB/precision/recall test of the model-assisted canonicalizer
arm -- something P2-E08 did for FrequencyOnlyCanonicalizer/
HeuristicCanonicalizer (both deterministic, cheap to run over the FULL
real ~206K-distinct-brand vocabulary) but the model-assisted arm cannot
do at that scale: classifying all ~206,227 distinct real brands would mean
~206,227 individual agent judgments, which CLAUDE.md's own cold-start
discipline ("do not perform one LLM call per SKU/value at scale") and this
environment's lack of a live, cheap model API both rule out.

The insight this script acts on: a brand string that is below the
canonicalization threshold AND never appears anywhere in the real
22,458-query judged set cannot possibly change any measured FIB/precision/
recall number, no matter how it's classified -- so restricting the
end-to-end test to brands that *could* matter is not cherry-picking, it is
the exact set relevant to the measurement, at a scale (~7,532 candidates
at threshold=25, verified separately) still too large for full agent
coverage but tractable to *sample* from, exactly like the original
209-candidate adjudication corpus sampled from a larger real population.

Deterministic (seed=7, matching every other sampling script in this
project). Reads dataset_cache/export/catalog.jsonl and
dataset_cache/export/queries.jsonl (both gitignored, real). Writes
dataset_cache/export/brand_query_relevant_sample.jsonl (gitignored).
"""
import json
import random
import sys
from collections import defaultdict
from pathlib import Path

SEED = 7
CATALOG_PATH = Path("dataset_cache/export/catalog.jsonl")
QUERIES_PATH = Path("dataset_cache/export/queries.jsonl")
OUT_PATH = Path("dataset_cache/export/brand_query_relevant_sample.jsonl")

THRESHOLD = 25  # P2-E05's measured real recall-peak frontier
SAMPLE_SIZE = 500
MAX_REPRESENTATIVE_PRODUCTS = 5


def normalize(s: str) -> str:
    return s.strip().lower()


def main() -> None:
    if not CATALOG_PATH.exists() or not QUERIES_PATH.exists():
        print("missing real dataset files", file=sys.stderr)
        sys.exit(1)

    print("loading real catalog, grouping by normalized brand...", file=sys.stderr)
    by_brand = defaultdict(list)
    with CATALOG_PATH.open() as f:
        for line in f:
            rec = json.loads(line)
            brand = rec.get("brand")
            if brand:
                by_brand[normalize(brand)].append(rec)
    frequency = {b: len(prods) for b, prods in by_brand.items()}
    print(f"{len(frequency)} distinct real brands", file=sys.stderr)

    print("loading real queries...", file=sys.stderr)
    query_texts = set()
    with QUERIES_PATH.open() as f:
        for line in f:
            d = json.loads(line)
            query_texts.add(d["query"].lower())
    all_query_text = "\x01".join(query_texts)
    print(f"{len(query_texts)} distinct real queries", file=sys.stderr)

    below_threshold = [b for b, f in frequency.items() if f < THRESHOLD]
    print(f"below threshold={THRESHOLD}: {len(below_threshold)} brands", file=sys.stderr)

    query_relevant = sorted(b for b in below_threshold if len(b) >= 3 and b in all_query_text)
    print(
        f"below-threshold brands whose exact string appears in some real query: {len(query_relevant)}",
        file=sys.stderr,
    )

    rng = random.Random(SEED)
    sample = rng.sample(query_relevant, min(SAMPLE_SIZE, len(query_relevant)))
    sample.sort()
    print(f"sampled {len(sample)} for end-to-end model-assisted evaluation", file=sys.stderr)

    corpus = []
    for brand in sample:
        products = by_brand[brand]
        rng_local = random.Random(f"{SEED}:{brand}")
        reps = rng_local.sample(products, min(MAX_REPRESENTATIVE_PRODUCTS, len(products)))
        representative_products = [
            {
                "asin": p["id"],
                "title": p["title"],
                "bullets_snippet": (p.get("bullets") or "")[:300],
                "color": p.get("color"),
            }
            for p in reps
        ]
        corpus.append(
            {
                "brand_normalized": brand,
                "real_occurrence_count": frequency[brand],
                "representative_products": representative_products,
                "note_missing_evidence": "no real product_type/category/seller field in this dataset",
            }
        )

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with OUT_PATH.open("w") as f:
        for entry in corpus:
            f.write(json.dumps(entry) + "\n")
    print(f"wrote {len(corpus)} candidates to {OUT_PATH}", file=sys.stderr)


if __name__ == "__main__":
    main()
