#!/usr/bin/env python3
"""Round 1 R1-E04: latency/relevance benchmark against the real Solr index.

Query classes mirror what crates/round1-eval/src/bin/*.rs measure against
CatalogIndex, so the two are comparable class-by-class (not forcing one
engine to do work the other doesn't):
  - exact lookup: q=id:<ASIN>
  - structural filter: fq=brand:"X" AND fq=color:"Y" (the same brand+color
    pair commerce-core's CatalogIndex filters on)
  - broad lexical: q=all_text:<free text> (standard Solr relevance ranking)

Also computes NDCG@10/Recall@10/MRR against real ESCI judgments for a
sample of real queries, using Solr's own top-K ranking — see
docs/experiments/ROUND1_LOG.md R1-E04 for why a head-to-head with
commerce-core's structural filter isn't apples-to-apples (no ranking
mechanism exists there for free text).
"""

import json
import math
import random
import time
from pathlib import Path

import requests

SOLR_URL = "http://localhost:8983/solr/commerce_bench"
EXPORT = Path(__file__).resolve().parents[2] / "dataset_cache" / "export"


def percentile(sorted_vals, p):
    if not sorted_vals:
        return 0.0
    idx = round((len(sorted_vals) - 1) * p)
    return sorted_vals[idx]


def time_calls(fn, n):
    samples = []
    for _ in range(n):
        t0 = time.perf_counter()
        fn()
        samples.append((time.perf_counter() - t0) * 1e6)  # microseconds
    samples.sort()
    return samples


def report(label, samples):
    print(
        f"  {label:<24} p50={percentile(samples, 0.5):>9.1f}us  "
        f"p95={percentile(samples, 0.95):>9.1f}us  p99={percentile(samples, 0.99):>9.1f}us  (n={len(samples)})"
    )


def load_sample_ids(n=2000, seed=42):
    rng = random.Random(seed)
    ids = []
    with open(EXPORT / "catalog.jsonl") as f:
        for i, line in enumerate(f):
            if i % 500 == 0:  # spread across the file without loading it all
                ids.append(json.loads(line)["id"])
    rng.shuffle(ids)
    return ids[:n]


def main():
    session = requests.Session()

    print("=== Exact ID lookup ===")
    ids = load_sample_ids(2000)

    def exact_lookup():
        pid = random.choice(ids)
        session.get(f"{SOLR_URL}/select", params={"q": f"id:{pid}", "rows": 1}).json()

    report("exact_id_lookup", time_calls(exact_lookup, 2000))

    print("=== Structural filter (brand + color, matches CatalogIndex's filter) ===")

    def structural_filter():
        session.get(
            f"{SOLR_URL}/select",
            params={"q": "*:*", "fq": ['brand:"Nike"', 'color:"Black"'], "rows": 10},
        ).json()

    report("structural_filter", time_calls(structural_filter, 2000))

    print("=== Broad lexical (free-text, default Solr relevance ranking) ===")
    lexical_terms = ["waterproof running shoes", "wireless bluetooth headphones", "kitchen storage container", "led desk lamp"]

    def broad_lexical():
        term = random.choice(lexical_terms)
        session.get(f"{SOLR_URL}/select", params={"q": f"all_text:({term})", "rows": 10}).json()

    report("broad_lexical", time_calls(broad_lexical, 1000))

    print()
    print("=== Relevance: NDCG@10 / Recall@10 / MRR (real ESCI judgments) ===")
    judgments_by_query = {}
    query_text_by_id = {}
    with open(EXPORT / "queries.jsonl") as f:
        for line in f:
            row = json.loads(line)
            judgments_by_query.setdefault(row["query_id"], []).append(row)
            query_text_by_id[row["query_id"]] = row["query"]

    rng = random.Random(7)
    sample_qids = rng.sample(list(judgments_by_query.keys()), 1000)

    def relevance_gain(label):
        return {"E": 3, "S": 2, "C": 1, "I": 0}[label]

    ndcgs, recalls, mrrs = [], [], []
    zero_result = 0
    query_errors = 0
    evaluated = 0
    for qid in sample_qids:
        query_text = query_text_by_id[qid]
        judged = {j["product_id"]: j["label"] for j in judgments_by_query[qid]}
        relevant_ids = {pid for pid, label in judged.items() if label in ("E", "S")}
        if not relevant_ids:
            continue
        # edismax, not the default lucene parser: robust to the special
        # characters real shopper queries contain ($, #, ", etc. — see
        # docs/experiments/ROUND1_LOG.md R1-E04). This is the standard,
        # competently-configured choice for free-text user queries, not a
        # workaround unique to this benchmark.
        resp = session.get(
            f"{SOLR_URL}/select",
            params={"q": query_text, "defType": "edismax", "qf": "all_text", "rows": 10},
        ).json()
        if "response" not in resp:
            query_errors += 1
            continue
        evaluated += 1
        docs = resp["response"]["docs"]
        if not docs:
            zero_result += 1
        ranked_ids = [d["id"] for d in docs]

        dcg = sum(relevance_gain(judged.get(pid, "I")) / math.log2(i + 2) for i, pid in enumerate(ranked_ids))
        ideal = sorted((relevance_gain(l) for l in judged.values()), reverse=True)[:10]
        idcg = sum(g / math.log2(i + 2) for i, g in enumerate(ideal))
        ndcgs.append(dcg / idcg if idcg > 0 else 0.0)

        hit = len(set(ranked_ids) & relevant_ids)
        recalls.append(hit / len(relevant_ids))

        rr = 0.0
        for i, pid in enumerate(ranked_ids):
            if pid in relevant_ids:
                rr = 1.0 / (i + 1)
                break
        mrrs.append(rr)

    n = len(ndcgs)
    print(f"  queries sampled: {len(sample_qids)}, query errors (excluded): {query_errors}, evaluated: {evaluated}")
    print(f"  zero-result rate: {zero_result}/{evaluated} ({zero_result / evaluated * 100:.1f}%)")
    print(f"  NDCG@10: {sum(ndcgs) / n:.4f}")
    print(f"  Recall@10: {sum(recalls) / n:.4f}")
    print(f"  MRR: {sum(mrrs) / n:.4f}")


if __name__ == "__main__":
    main()
