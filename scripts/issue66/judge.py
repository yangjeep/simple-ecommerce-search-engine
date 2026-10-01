#!/usr/bin/env python3
"""Issue #66 amendment 1 section 4: judge one measured window against S1 and S2.

S1 is the frozen #65 SLO; S2 doubles its latency bounds. Every other
condition is shared (error rate < 0.1%, achieved >= 98% of offered, not
HARNESS_SATURATED, no correctness-check failure) and a window whose serving
processes died or were OOM-killed fails both.

    judge.py <load.json> [<sidecar.json>]   -> prints "S1=PASS S2=FAIL mode=..."
"""
import json
import sys

SLOS = {"S1": (50.0, 100.0), "S2": (100.0, 200.0)}


def judge(load, side=None):
    side = side or {}
    o = load["overall"]
    dead = side.get("alive") is False or (side.get("oom_kill_delta") or 0) > 0
    common = (
        not load["harness_saturated"]
        and o["achieved_over_offered"] >= 0.98
        and o["error_rate"] < 0.001
        and "check" not in (load.get("error_kinds") or {})
        and not dead
    )
    out = {s: common and o["p95_ms"] < p95 and o["p99_ms"] < p99 for s, (p95, p99) in SLOS.items()}
    if out["S1"] and out["S2"]:
        mode = "none"
    elif dead:
        mode = "oom" if (side.get("oom_kill_delta") or 0) > 0 else "process_death"
    elif load["harness_saturated"]:
        mode = "harness_saturated"
    elif o["error_rate"] >= 0.001 or "check" in (load.get("error_kinds") or {}):
        mode = "errors"
    elif o["achieved_over_offered"] < 0.98:
        mode = "throughput"
    elif (side.get("throttled_share") or 0) > 0.05:
        mode = "latency_throttled"
    else:
        mode = "latency"
    out["mode"] = mode
    return out


def load_pair(path):
    with open(path) as f:
        load = json.load(f)
    try:
        with open(path[: -len(".json")] + ".env.json") as f:
            side = json.load(f)
    except (OSError, ValueError):
        side = None
    return load, side


if __name__ == "__main__":
    try:
        load, side = load_pair(sys.argv[1])
        r = judge(load, side)
    except (OSError, ValueError, KeyError):
        r = {"S1": False, "S2": False, "mode": "no_result"}
    print(" ".join(f"{s}={'PASS' if r[s] else 'FAIL'}" for s in SLOS) + f" mode={r['mode']}")
