#!/usr/bin/env python3
"""Issue #9: build a durable, provenance-recorded adjudication corpus from
the real 1.2M-product ESCI catalog's brand vocabulary -- specifically the
long tail P2-E05's frequency-only canonicalization (commerce_core::cold_start
::compile_lexicon's min_enum_frequency, now applied to brand too) excludes
or sits near the frontier of.

Deterministic (fixed seed, matching R1-E04's own random.Random(seed=7)
precedent). Reads only dataset_cache/export/catalog.jsonl (gitignored, real
data, produced by scripts/round1/export_esci.py -- see docs/experiments/
ROUND1_LOG.md for provenance). Writes dataset_cache/export/
brand_adjudication_corpus.jsonl (gitignored, reproducible from this script
-- the script itself, not the corpus file, is the committed artifact).

Normalization matches round1_eval::catalog::build_catalog exactly
(`s.trim().to_lowercase()`) so occurrence counts here are identical to what
commerce_core::cold_start::CatalogProfile computes at index-build time --
otherwise this corpus would not actually reflect the real canonicalization
frontier it's meant to probe.

Bucket definitions (frequency = number of real products carrying this exact
normalized brand string, over the full 1,215,854-product catalog):
  - singleton:       frequency == 1     (P2-E05: 49.4% of all distinct brands)
  - low:             2 <= frequency <= 5
  - mid:             6 <= frequency <= 25
  - near_threshold:  20 <= frequency <= 30  (P2-E05's measured real recall
                      peak was at min_enum_frequency=25; this bucket
                      deliberately straddles that exact frontier)
  - calibration_high_frequency: a small, fixed set of well-known real brand
    names already trusted at every threshold P2-E05 tested (frequency >=100)
    -- NOT part of the frontier being probed, included only so the
    adjudication rubric and any classifier can be sanity-checked against
    obviously-easy cases.

Each candidate carries real catalog evidence: the normalized brand string,
its real occurrence count, up to 5 representative real products (ASIN,
title, bullets snippet, color), and same-bucket brand strings sharing a
token with it (a crude, deterministic alias/near-duplicate signal -- not a
judgment, just evidence for whoever/whatever adjudicates).

This dataset's real catalog does NOT have real product_type/category or
seller fields (round1_eval::catalog's own documented limitation -- every
real product gets a sentinel ProductTypeId(0)/CategoryId(0); there is no
seller field in the ESCI export at all) -- Issue #9 asked for those as
adjudication evidence where available; here they are not available, and
that gap is recorded rather than papered over with invented data.
"""
import json
import random
import sys
from collections import defaultdict
from pathlib import Path

SEED = 7  # matches R1-E04's random.Random(seed=7) precedent
CATALOG_PATH = Path("dataset_cache/export/catalog.jsonl")
OUT_PATH = Path("dataset_cache/export/brand_adjudication_corpus.jsonl")

BUCKET_DEFS = [
    ("singleton", lambda f: f == 1),
    ("low", lambda f: 2 <= f <= 5),
    ("mid", lambda f: 6 <= f <= 25),
    ("near_threshold", lambda f: 20 <= f <= 30),
]
SAMPLES_PER_BUCKET = 50
CALIBRATION_HIGH_FREQ_BRANDS = [
    "nike", "adidas", "disney", "hanes", "under armour", "columbia", "skechers",
    "amazon essentials", "amazon basics",
]
MAX_REPRESENTATIVE_PRODUCTS = 5


def normalize(s: str) -> str:
    return s.strip().lower()


def token_set(s: str) -> set:
    return set(s.split())


def main() -> None:
    if not CATALOG_PATH.exists():
        print(f"missing {CATALOG_PATH} -- run scripts/round1/export_esci.py first", file=sys.stderr)
        sys.exit(1)

    print("loading real catalog and grouping by normalized brand...", file=sys.stderr)
    by_brand = defaultdict(list)
    total_products = 0
    with CATALOG_PATH.open() as f:
        for line in f:
            total_products += 1
            rec = json.loads(line)
            brand = rec.get("brand")
            if not brand:
                continue
            by_brand[normalize(brand)].append(rec)
    print(f"{total_products} real products, {len(by_brand)} distinct normalized brands", file=sys.stderr)

    frequency = {b: len(prods) for b, prods in by_brand.items()}
    singleton_count = sum(1 for f in frequency.values() if f == 1)
    print(
        f"singleton brands: {singleton_count} ({singleton_count / len(frequency) * 100:.1f}%) "
        f"-- cross-check against P2-E05's independently-measured 49.4%",
        file=sys.stderr,
    )

    rng = random.Random(SEED)
    corpus = []

    for bucket_name, predicate in BUCKET_DEFS:
        candidates = sorted(b for b, f in frequency.items() if predicate(f))
        sample = rng.sample(candidates, min(SAMPLES_PER_BUCKET, len(candidates)))
        print(f"bucket={bucket_name}: {len(candidates)} eligible, sampled {len(sample)}", file=sys.stderr)
        for brand in sample:
            corpus.append(build_entry(brand, by_brand, frequency, bucket_name))

    calibration = []
    for brand in CALIBRATION_HIGH_FREQ_BRANDS:
        if brand in by_brand:
            calibration.append(build_entry(brand, by_brand, frequency, "calibration_high_frequency"))
        else:
            print(f"WARNING: calibration brand {brand!r} not found in this catalog export", file=sys.stderr)
    corpus.extend(calibration)

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with OUT_PATH.open("w") as f:
        for entry in corpus:
            f.write(json.dumps(entry) + "\n")

    print(f"wrote {len(corpus)} candidates to {OUT_PATH}", file=sys.stderr)
    bucket_totals = defaultdict(int)
    for entry in corpus:
        bucket_totals[entry["bucket"]] += 1
    for bucket, count in sorted(bucket_totals.items()):
        print(f"  {bucket}: {count}", file=sys.stderr)


def build_entry(brand, by_brand, frequency, bucket_name):
    products = by_brand[brand]
    rng_local = random.Random(f"{SEED}:{brand}")  # deterministic per-brand sub-sample
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
    brand_tokens = token_set(brand)
    same_bucket_token_overlap = sorted(
        other
        for other, f in frequency.items()
        if other != brand
        and token_set(other) & brand_tokens
        and len(token_set(other) & brand_tokens) >= 1
    )[:10]
    return {
        "brand_normalized": brand,
        "bucket": bucket_name,
        "real_occurrence_count": frequency[brand],
        "representative_products": representative_products,
        "same_bucket_token_overlap_candidates": same_bucket_token_overlap,
        "note_missing_evidence": "no real product_type/category/seller field in this dataset (round1_eval::catalog's documented ESCI export limitation)",
    }


if __name__ == "__main__":
    main()
