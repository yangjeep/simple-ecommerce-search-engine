#!/usr/bin/env python3
"""Issue #57 frozen benchmark: Retailrocket traffic/popularity-weighting
analysis for the whole-workload economics synthesis (FULL_MATRIX_PROTOCOL.md
§9.2 -- Retailrocket has no query text or relevance judgments, so it is
used ONLY for real traffic-shape evidence, never for retrieval/relevance
scoring). Reads dataset_cache/retailrocket/events.csv directly (2.76M
real anonymized shopper events, CC BY-NC-SA 4.0) and writes a compact
JSON summary consumed by the economics synthesis doc.

Usage: python3 scripts/datasets/analyze_retailrocket_traffic.py
"""
import csv
import json
from collections import Counter
from pathlib import Path

EVENTS = Path(__file__).resolve().parents[2] / "dataset_cache" / "retailrocket" / "events.csv"
OUT = Path(__file__).resolve().parents[2] / "dataset_cache" / "retailrocket_traffic_summary.json"


def main():
    event_type_counts = Counter()
    item_view_counts = Counter()
    visitor_event_counts = Counter()
    total = 0

    with open(EVENTS) as f:
        reader = csv.DictReader(f)
        for row in reader:
            total += 1
            event_type_counts[row["event"]] += 1
            visitor_event_counts[row["visitorid"]] += 1
            if row["event"] == "view":
                item_view_counts[row["itemid"]] += 1

    n_items = len(item_view_counts)
    n_visitors = len(visitor_event_counts)
    sorted_views = sorted(item_view_counts.values(), reverse=True)
    total_views = sum(sorted_views)

    def top_k_share(k_frac):
        k = max(1, int(n_items * k_frac))
        return sum(sorted_views[:k]) / total_views

    summary = {
        "total_events": total,
        "distinct_items_viewed": n_items,
        "distinct_visitors": n_visitors,
        "event_type_counts": dict(event_type_counts),
        "event_type_shares": {k: v / total for k, v in event_type_counts.items()},
        "conversion_funnel": {
            "view_to_addtocart_rate": event_type_counts.get("addtocart", 0)
            / max(1, event_type_counts.get("view", 1)),
            "addtocart_to_transaction_rate": event_type_counts.get("transaction", 0)
            / max(1, event_type_counts.get("addtocart", 1)),
        },
        "item_popularity_concentration": {
            "top_1pct_items_share_of_views": top_k_share(0.01),
            "top_5pct_items_share_of_views": top_k_share(0.05),
            "top_10pct_items_share_of_views": top_k_share(0.10),
            "top_20pct_items_share_of_views": top_k_share(0.20),
        },
        "visitor_activity": {
            "mean_events_per_visitor": total / n_visitors,
            "median_events_per_visitor": sorted(visitor_event_counts.values())[n_visitors // 2],
            "max_events_single_visitor": max(visitor_event_counts.values()),
        },
    }
    OUT.write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
