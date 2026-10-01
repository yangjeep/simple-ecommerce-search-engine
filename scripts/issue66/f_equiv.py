#!/usr/bin/env python3
"""Issue #66 clarification C7a: tie-aware lexical (class F) equivalence.

F ranks variant-level documents; variants of one product share title and
description, so exact BM25 ties are common and their order follows internal
docids, which may differ between fresh indexes. Equivalence is therefore:
identical num_found, identical top-K score sequence (relative tolerance 1e-6),
and identical id sets strictly above the boundary (lowest top-K) score.

    f_equiv.py record  <solr_url> <pools.json> <out.json>
    f_equiv.py compare <reference.json> <candidate.json>   -> EQUIVALENT|NOT_EQUIVALENT ...
    f_equiv.py frozen  <reference.json> <pools.json>       -> exact-order agreement with frozen ids
"""
import json
import sys
import urllib.request

CORE = "/solr/i77_wands/select"
TOL = 1e-6


def record(url, pools_path, out):
    pools = json.load(open(pools_path))
    res = {}
    for r in pools["requests"]:
        if r["class"] != "F":
            continue
        body = dict(r["solr_body"]) if isinstance(r["solr_body"], dict) else json.loads(r["solr_body"])
        body["fields"] = "id,score"
        req = urllib.request.Request(url + CORE, data=json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json"})
        d = json.load(urllib.request.urlopen(req, timeout=30))
        res[r["id"]] = {"num_found": d["response"]["numFound"],
                        "hits": [[x["id"], x["score"]] for x in d["response"]["docs"]]}
    json.dump(res, open(out, "w"))
    print(f"recorded {len(res)} F responses")


def close(a, b):
    return abs(a - b) <= TOL * max(abs(a), abs(b), 1e-12)


def equivalent(a, b):
    if a["num_found"] != b["num_found"] or len(a["hits"]) != len(b["hits"]):
        return False, "num_found/len"
    sa, sb = [h[1] for h in a["hits"]], [h[1] for h in b["hits"]]
    if not all(close(x, y) for x, y in zip(sa, sb)):
        return False, "scores"
    if not sa:
        return True, ""
    boundary = min(sa)
    above = lambda hits: {h[0] for h in hits if h[1] > boundary and not close(h[1], boundary)}  # noqa: E731
    if above(a["hits"]) != above(b["hits"]):
        return False, "ids_above_boundary"
    return True, ""


def compare(ref_path, cand_path):
    ref, cand = json.load(open(ref_path)), json.load(open(cand_path))
    bad = {}
    exact = 0
    for k, a in ref.items():
        ok, why = equivalent(a, cand.get(k, {"num_found": None, "hits": []}))
        if not ok:
            bad[k] = why
        exact += [h[0] for h in a["hits"]] == [h[0] for h in cand.get(k, {"hits": []})["hits"]]
    kinds = {}
    for why in bad.values():
        kinds[why] = kinds.get(why, 0) + 1
    print(("EQUIVALENT" if not bad else "NOT_EQUIVALENT")
          + f" f_total={len(ref)} not_equivalent={len(bad)} kinds={json.dumps(kinds)} exact_order_same={exact}")


def frozen(ref_path, pools_path):
    ref = json.load(open(ref_path))
    pools = json.load(open(pools_path))
    same_order = same_set = nf = 0
    tot = 0
    for r in pools["requests"]:
        if r["class"] != "F":
            continue
        tot += 1
        got = [h[0] for h in ref[r["id"]]["hits"]]
        exp = r["expect"].get("ids") or []
        same_order += got == exp
        same_set += set(got) == set(exp)
        nf += ref[r["id"]]["num_found"] == r["expect"]["num_found"]
    print(f"vs_frozen f_total={tot} num_found_same={nf} same_set={same_set} same_order={same_order}")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "record":
        record(*sys.argv[2:5])
    elif cmd == "compare":
        compare(*sys.argv[2:4])
    elif cmd == "frozen":
        frozen(*sys.argv[2:4])
