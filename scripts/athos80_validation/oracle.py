#!/usr/bin/env python3
"""Independent O(N) contract oracle. No native code/index/comparator imports.

This intentionally scans catalog rows for every filter and facet. BM25 is not
reimplemented: supplied hits isolate the execution contract; optional HTTP
candidate traces verify integration but are not an independent lexical oracle.
"""
from __future__ import annotations
import copy
import json
import math
import operator
import struct
from collections import Counter, defaultdict
from functools import cmp_to_key

CONTRACT = "athos80-experimental-v1.1"
PROFILE_NAMES={"exact_token_and":"exact_token_and_no_bm25", "supplied":"supplied_fixture_hits_unverified_completeness",
               "bm25":"tantivy_0_22_variant_bm25_strict_parser_and_bucket0", "bm25_literal":"tantivy_0_22_variant_bm25_literal_and_bucket0_v1"}

def tokens(text):
    # Rust's is_alphanumeric includes Unicode letters and numbers, not '_'.
    out, word = [], []
    for char in text:
        if char.isalnum():
            word.append(char)
        elif word:
            out.append("".join(word).lower()); word = []
    if word:
        out.append("".join(word).lower())
    return out

def norm(text):
    return " ".join(tokens(text))

def defaults(request):
    result = dict(q="", filters=[], in_stock_only=True, result_mode="products",
                  facet_count_mode=None, facets=[], lexical={"profile":"exact_token_and"},
                  context={}, rules=[], personal_signals=[], sort="relevance",
                  sort_policy="shopper_overrides", offset=0, limit=20)
    result.update(copy.deepcopy(request))
    return result

def rows(catalog):
    return [(p, v, dict(p.get("attributes", {}), **v.get("attributes", {})))
            for p in catalog["products"] for v in p["variants"]]

class ScanCatalog:
    """Immutable row-materialization cache only: no filter/rank/facet indexes.

    Every search and facet still scans rows independently. This removes repeated
    copying of public attribute dictionaries across four experimental arms.
    """
    def __init__(self, catalog):
        self.catalog = catalog
        self.rows = rows(catalog)
        self.identities = {v["variant_id"]:(p,v) for p,v,_ in self.rows}

def field_of_filter(f):
    kind = f["type"]
    if "attribute" in f:
        return ("attribute", f["attribute"])
    return ("price_cents" if kind.startswith("price_") else kind,)

def field_key(field):
    return (field["type"], field["name"]) if field["type"] == "attribute" else (field["type"],)

def matches_filter(row, f):
    p, v, attrs = row
    kind, want = f["type"], f["value"]
    if kind in ("brand", "category", "product_type"):
        return p[kind + "_id"] == want
    if kind == "price_under_cents": return v["price_cents"] < want
    if kind == "price_over_cents": return v["price_cents"] > want
    attr = attrs.get(f["attribute"])
    if attr is None: return False
    expected_type = {"enum":"enum", "multi_enum_contains":"multi_enum", "boolean":"boolean",
                     "numeric":"numeric", "text_contains":"text"}[kind]
    if attr["type"] != expected_type: return False
    have = attr["value"]
    if kind in ("enum", "boolean"): return have == want
    if kind in ("multi_enum_contains", "text_contains"): return want in have
    return {"eq":operator.eq, "lt":operator.lt, "lte":operator.le,
            "gt":operator.gt, "gte":operator.ge}[f["op"]](have, want)

def matches_all(row, filters):
    return all(matches_filter(row, f) for f in filters)

def matches_target(row, target):
    kind = target["type"]
    if kind == "all": return True
    if kind == "products": return row[0]["product_id"] in target["ids"]
    if kind == "variants": return row[1]["variant_id"] in target["ids"]
    if kind == "filters": return matches_all(row, target["filters"])
    raise ValueError("unknown target: " + kind)

def specificity(rule):
    return {"global":0, "category":1, "query":2}[rule.get("scope", {"type":"global"})["type"]]

def rule_order(rule):
    return (-specificity(rule), -rule.get("priority", 0), rule["id"])

def active(rule, request):
    scope = rule.get("scope", {"type":"global"})
    if scope["type"] == "query" and norm(scope["query"]) != norm(request["q"]): return False
    ctx = request["context"]
    if scope["type"] == "category" and ctx.get("category_id") != scope["category_id"]: return False
    for c in rule.get("conditions", []):
        kind = c["type"]
        if kind == "query_equals" and norm(c["query"]) != norm(request["q"]): return False
        if kind == "query_contains_all" and not all(t in set(tokens(request["q"])) for part in c["tokens"] for t in tokens(part)): return False
        if kind == "context_category" and ctx.get("category_id") != c["category_id"]: return False
        if kind == "context_tag" and c["tag"] not in ctx.get("tags", []): return False
    return True

def lexical_rows(all_rows, request, identities=None):
    profile = request["lexical"]["profile"]
    if profile == "supplied":
        lookup = identities if identities is not None else {v["variant_id"]:(p, v) for p, v, _ in all_rows}
        scores = {}
        for hit in request["lexical"]["hits"]:
            vid = hit["variant_id"]
            if vid not in lookup or lookup[vid][0]["product_id"] != hit["product_id"]:
                raise ValueError("supplied lexical identity does not match catalog")
            if vid in scores: raise ValueError("duplicate supplied variant")
            if not math.isfinite(hit["score"]) or not 0 <= hit["score"] <= 1e12:
                raise ValueError("invalid supplied score")
            scores[vid] = (0.0 if hit["score"] == 0 else hit["score"], hit["relevance_bucket"])
        return scores
    if profile != "exact_token_and":
        raise ValueError("Independent oracle needs exact_token_and or supplied hits, not " + profile)
    want = set(tokens(request["q"]))
    scores = {}
    for p, v, attrs in all_rows:
        text = p["title"] + " " + " ".join(a["value"] for a in attrs.values() if a["type"] == "text")
        if want.issubset(set(tokens(text))): scores[v["variant_id"]] = (1.0, 0)
    return scores

def numeric_label(value):
    # Fixtures avoid floating serialization language differences except signed zero.
    if value == 0: return "0"
    if float(value).is_integer(): return str(int(value))
    return str(value)

def facet_values(row, field):
    p, v, attrs = row
    kind = field["type"]
    if kind in ("brand", "category", "product_type"): return [str(p[kind + "_id"])]
    if kind == "price_cents": return [str(v["price_cents"])]
    attr = attrs.get(field["name"])
    if attr is None: return []
    value = attr["value"]
    if attr["type"] == "multi_enum": return list(set(value))
    if attr["type"] == "boolean": return ["true" if value else "false"]
    if attr["type"] == "numeric": return [numeric_label(value)]
    return [value]

def search(catalog, request, prepared=None):
    request = defaults(request)
    if prepared is not None and prepared.catalog is not catalog:raise ValueError("ScanCatalog belongs to another catalog")
    all_rows = rows(catalog) if prepared is None else prepared.rows
    lexical = lexical_rows(all_rows, request, None if prepared is None else prepared.identities)
    active_rules = [r for r in request["rules"] if active(r, request)]
    hides = [r for r in active_rules if r["action"]["type"] == "hide"]
    def eligible(row, filters):
        return (row[1]["variant_id"] in lexical
                and (not request["in_stock_only"] or (row[1]["in_stock"] and row[1]["available_units"] > 0))
                and matches_all(row, filters)
                and not any(matches_target(row, r["target"]) for r in hides))
    eligible_rows = [row for row in all_rows if eligible(row, request["filters"])]
    eligible_counts = Counter(p["product_id"] for p, _, _ in eligible_rows)
    facet_mode = request["facet_count_mode"] or request["result_mode"]
    facets = []
    for facet in request["facets"]:
        filters = [f for f in request["filters"] if field_of_filter(f) != field_key(facet["field"])]
        counts = defaultdict(set)
        for row in all_rows:
            if not eligible(row, filters): continue
            identity = row[0]["product_id"] if facet_mode == "products" else row[1]["variant_id"]
            for label in facet_values(row, facet["field"]): counts[label].add(identity)
        facets.append({"name":facet["name"], "counts":{k:len(v) for k, v in sorted(counts.items())}})
    shopper = request["sort"] != "relevance" and request["sort_policy"] == "shopper_overrides"
    group_rules = [r for r in active_rules if r["action"]["type"] == "group"]
    if group_rules:
        winning_scope = max(specificity(r) for r in group_rules)
        group_rules = [r for r in group_rules if specificity(r) == winning_scope]
    axes = sorted({r["action"].get("order", 0) for r in group_rules})
    ranked = []
    for row in eligible_rows:
        p, v, _ = row
        score, bucket = lexical[v["variant_id"]]
        groups, rule_ids = [], []
        for axis in axes:
            winners = [r for r in group_rules if r["action"].get("order", 0) == axis and matches_target(row, r["target"])]
            winner = min(winners, key=rule_order) if winners else None
            groups.append(winner["action"]["rank"] if winner else 0)
            rule_ids.append(winner["id"] if winner else None)
        factors = sorted(s["factor"] for s in request["personal_signals"] if matches_target(row, s["target"]))
        if any(not math.isfinite(f) or not .25 <= f <= 4 for f in factors): raise ValueError("invalid personal factor")
        raw_factor = math.exp(min(math.log(4.0), max(math.log(.25), math.fsum(math.log(f) for f in factors))))
        factor = min(4.0, max(.25, math.floor(raw_factor * 1e12 + .5) / 1e12))
        pin_rules = [r for r in active_rules if r["action"]["type"] == "pin" and matches_target(row, r["target"])]
        pin = min(pin_rules, key=rule_order) if pin_rules and not shopper else None
        hit = dict(product_id=p["product_id"], selected_variant_id=v["variant_id"],
                   matching_variant_count=eligible_counts[p["product_id"]] if request["result_mode"] == "products" else 1,
                   lexical_score=score, relevance_bucket=bucket, merch_groups=[] if shopper else groups,
                   personal_factor=1.0 if shopper else factor,
                   composed_score=score if shopper else score * factor, price_cents=v["price_cents"],
                   group_rule_ids=[] if shopper else rule_ids, pin_rule_id=None)
        ranked.append((hit, pin))
    def rank_key(item):
        hit = item[0]
        price = hit["price_cents"] * (-1 if request["sort"] == "price_desc" else 1)
        stable = (hit["product_id"], hit["selected_variant_id"])
        if shopper: return (price, *stable)
        return (-hit["relevance_bucket"], tuple(-n for n in hit["merch_groups"]),
                -hit["composed_score"], price if request["sort"] != "relevance" else 0, *stable)
    ranked.sort(key=rank_key)
    pin_display = {}
    if request["result_mode"] == "products":
        product_rows = defaultdict(list)
        for hit, pin in ranked:
            product_rows[hit["product_id"]].append((hit, pin))
        grouped = []
        for product_rows_ in product_rows.values():
            claims = [(hit, pin) for hit, pin in product_rows_ if pin is not None]
            selected, strongest = min(claims, key=lambda pair: (rule_order(pair[1]), rank_key(pair))) if claims else (None, None)
            best_hit = product_rows_[0][0]
            if strongest is not None and strongest["target"]["type"] == "variants":
                pin_display[best_hit["product_id"]] = selected
            grouped.append((best_hit, strongest))
        ranked = sorted(grouped, key=rank_key)
    # Pin conflicts resolve once, strongest rule then organic position; losers stay organic.
    slots, claimed = {}, set()
    claims = sorted(((rule_order(pin), i, pin["action"]["position"], pin)
                     for i, (_, pin) in enumerate(ranked) if pin is not None))
    for _, i, slot, pin in claims:
        if slot >= len(ranked): raise ValueError("eligible pin position outside result set")
        if slot not in slots:
            hit = ranked[i][0]
            if request["result_mode"] == "products":
                hit = pin_display.get(hit["product_id"], hit)
            hit["pin_rule_id"] = pin["id"]
            slots[slot] = hit
            claimed.add(i)
    organic = iter(hit for i, (hit, _) in enumerate(ranked) if i not in claimed)
    hits = [slots[i] if i in slots else next(organic) for i in range(len(ranked))]
    return dict(contract=CONTRACT, lexical_profile=PROFILE_NAMES[request["lexical"]["profile"]], result_mode=request["result_mode"], facet_count_mode=facet_mode,
                num_found=len(hits), hits=hits[request["offset"]:request["offset"]+request["limit"]],
                facets=facets, active_rule_ids=sorted(r["id"] for r in active_rules),
                eligible_variants=len(eligible_rows), eligible_products=len(eligible_counts), lexical_candidates=len(lexical), ranked_variants=len(eligible_rows))

def differences(expected, actual, path="", tolerance=1e-9):
    """Compare oracle-selected structure only; runtime measurements are excluded."""
    result = []
    if isinstance(expected, dict):
        if not isinstance(actual, dict): return [(path, expected, actual)]
        for key, value in expected.items():
            if key in ("eligible_variants", "eligible_products", "lexical_candidates", "ranked_variants"):
                result += differences(value, actual.get("metrics", {}).get(key), path+".metrics."+key, tolerance)
            elif key not in actual: result.append((path+"."+key, value, "<missing>"))
            else: result += differences(value, actual[key], path+"."+key, tolerance)
    elif isinstance(expected, list):
        if not isinstance(actual, list) or len(expected) != len(actual): result.append((path+".length", len(expected), len(actual) if isinstance(actual, list) else actual))
        if isinstance(actual, list):
            for i, (left, right) in enumerate(zip(expected, actual)): result += differences(left, right, f"{path}[{i}]", tolerance)
    elif isinstance(expected, float) and isinstance(actual, (float, int)):
        if path.endswith(".lexical_score"):
            if struct.pack("!d", expected) != struct.pack("!d", actual): result.append((path, expected, actual))
        elif not math.isclose(expected, actual, rel_tol=tolerance, abs_tol=tolerance): result.append((path, expected, actual))
    elif expected != actual: result.append((path, expected, actual))
    return result
