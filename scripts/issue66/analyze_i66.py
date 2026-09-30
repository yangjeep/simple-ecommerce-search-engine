#!/usr/bin/env python3
"""Issue #66 amendment 1 section 7 (+ clarifications C1-C7): envelope frontier,
confirmed minima and the preregistered gate.

    python3 scripts/issue66/analyze_i66.py [results_dir]
    -> <results>/report.md, <results>/report.json
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import plan  # noqa: E402
from judge import judge, load_pair  # noqa: E402

BAR = 0.75
T2 = plan.T2


def window_row(path):
    load, side = load_pair(path)
    v = judge(load, side)
    o = load["overall"]
    total = (load.get("cpu_usec") or {}).get("total")
    ok = max(o["ok"], 1)
    return {
        "S1": v["S1"], "S2": v["S2"], "mode": v["mode"],
        "p50": o["p50_ms"], "p95": o["p95_ms"], "p99": o["p99_ms"], "err": o["error_rate"],
        "ach": o["achieved_over_offered"],
        "cores_used": total / 1e6 / load["window_wall_s"] if total is not None else None,
        "ms_per_q": total / ok / 1000 if total is not None else None,
        "throttled": (side or {}).get("throttled_share"),
        "mem_gib": (side or {}).get("memory_current", 0) / 2**30 if side else None,
        "anon_gib": (side or {}).get("anon", 0) / 2**30 if side else None,
    }


def descent_table(d):
    rows = []
    if not os.path.exists(f"{d}/descent.tsv"):
        return rows
    for line in open(f"{d}/descent.tsv"):
        level, _s1, _s2, _mode, f = line.split()
        try:
            rows.append({"level": float(level), **window_row(f"{d}/{f}")})
        except (OSError, ValueError, KeyError):
            rows.append({"level": float(level), "S1": False, "S2": False, "mode": "no_result"})
    return rows


def ratio(h1, b0):
    """(ratio, kind): exact when both confirmed; a bound when B0 is infeasible at 3 cores."""
    if h1.get("level") is None:
        return None, "h1_" + h1.get("status", "unconfirmed").lower()
    if b0.get("level") is not None:
        return h1["level"] / b0["level"], "exact"
    if b0.get("status") == "INFEASIBLE":
        return h1["level"] / 3.0, "bound_b0_infeasible"
    return None, "b0_" + b0.get("status", "unconfirmed").lower()


def units(cores, gib):
    return max(cores, gib / 4.0)


def verdict(s):
    notes = []
    cpu = {}
    for rate in plan.TIERS:
        for slo in plan.SLOS:
            cpu[(rate, slo)] = ratio(s["h1"]["cpu"][f"t{rate}"][slo], s["b0"]["cpu"][f"t{rate}"][slo])
    r_t2, k_t2 = cpu[(T2, "S2")]
    cores_t2 = r_t2 is not None and r_t2 <= BAR
    robust = any(cpu[(t, "S2")][0] is not None and cpu[(t, "S2")][0] <= BAR for t in plan.TIERS if t != T2)
    mem = ratio(s["h1"]["mem"]["S2"], s["b0"]["mem"]["S2"])
    if mem[1] == "bound_b0_infeasible":  # a bound over cores means nothing for GiB
        mem = (None, "b0_mem_infeasible")
    gib_t2 = mem[0] is not None and mem[0] <= BAR
    # C3: u from the joint confirmation when both treatments are jointly feasible.
    u = {}
    flagged = False
    for k in plan.KINDS:
        j = s[k]["joint"].get("S2")
        c = s[k]["cpu"][f"t{T2}"]["S2"].get("level")
        m = s[k]["mem"]["S2"].get("level")
        if j and j["jointly_feasible"]:
            u[k] = units(j["cores"], j["mem"])
        elif c is not None and m is not None:
            u[k] = units(c, m)
            flagged = True
    u_ratio = u["h1"] / u["b0"] if len(u) == 2 else None
    u_t2 = u_ratio is not None and u_ratio <= BAR
    if (cores_t2 and robust) or gib_t2 or (u_t2 and not flagged):
        label = "KEEP"
    else:
        other_tier = any(cpu[(t, "S2")][0] is not None and cpu[(t, "S2")][0] <= BAR for t in plan.TIERS if t != T2)
        s1_only = any(cpu[(t, "S1")][0] is not None and cpu[(t, "S1")][0] <= BAR for t in plan.TIERS)
        if cores_t2 or (u_t2 and flagged) or other_tier or s1_only:
            label = "REFINE"
            if cores_t2:
                notes.append("cores criterion met at T2/S2 without T1/T3 robustness")
            if u_t2 and flagged:
                notes.append("normalized-units criterion met only on separately confirmed (not jointly confirmed) minima")
            if other_tier and not cores_t2:
                notes.append("cores criterion met only at a non-decision tier under S2")
            if s1_only and not (cores_t2 or other_tier):
                notes.append("cores criterion met only under S1")
        else:
            label = "REJECT"
    if k_t2 == "bound_b0_infeasible":
        notes.append("T2/S2 cores ratio is a bound: B0 infeasible at 3 cores")
    limiting = {}
    for k in plan.KINDS:
        c = s[k]["cpu"][f"t{T2}"]["S2"].get("level")
        m = s[k]["mem"]["S2"].get("level")
        if c is not None and m is not None:
            limiting[k] = "cpu" if c >= m / 4.0 else "ram"
    return {
        "label": label, "notes": notes,
        "cores_ratio": {f"t{t}_{slo}": {"ratio": v[0], "kind": v[1]} for (t, slo), v in cpu.items()},
        "gib_ratio_t2_s2": {"ratio": mem[0], "kind": mem[1]},
        "units_t2_s2": {"h1": u.get("h1"), "b0": u.get("b0"), "ratio": u_ratio, "flagged_not_joint": flagged},
        "units_sensitivity": {
            shape: ({k: max(s[k]["cpu"][f"t{T2}"]["S2"]["level"], s[k]["mem"]["S2"]["level"] / gib)
                     for k in plan.KINDS}
                    if all(s[k]["cpu"][f"t{T2}"]["S2"].get("level") is not None
                           and s[k]["mem"]["S2"].get("level") is not None for k in plan.KINDS) else None)
            for shape, gib in (("1:2", 2.0), ("1:8", 8.0))},
        "limiting_resource_t2_s2": limiting,
    }


def fmt(x, nd=1):
    return "—" if x is None else (f"{x:.{nd}f}" if isinstance(x, float) else str(x))


def table(rows, level_name):
    head = (f"| {level_name} | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |\n"
            "|---|---|---|---|---|---|---|---|---|---|---|---|\n")
    body = "".join(
        f"| {r['level']:g} | {'PASS' if r['S1'] else 'fail'} | {'PASS' if r['S2'] else 'fail'} | {r['mode']} | "
        f"{fmt(r.get('p50'))} | {fmt(r.get('p95'))} | {fmt(r.get('p99'))} | {fmt(r.get('err'), 4)} | "
        f"{fmt(r.get('cores_used'), 2)} | {fmt(r.get('ms_per_q'))} | {fmt(r.get('throttled'), 3)} | "
        f"{fmt(r.get('mem_gib'), 2)} |\n" for r in rows)
    return head + body


def main():
    R = sys.argv[1] if len(sys.argv) > 1 else "artifacts/issue66/results"
    s = plan.summarize(R)
    v = verdict(s)
    rep = {"summary": s, "verdict": v, "descents": {}}
    md = [f"# Issue #66 — minimum resource envelope\n\n**Preregistered gate (T2 = {T2} QPS, S2): {v['label']}**\n"]
    for n in v["notes"]:
        md.append(f"- {n}")
    md.append("\n## Confirmed minima\n\n| treatment | tier | S1 cores | S2 cores |\n|---|---|---|---|")
    for k in plan.KINDS:
        for t in plan.TIERS:
            c = s[k]["cpu"][f"t{t}"]
            cell = lambda x: fmt(x.get("level")) if x.get("level") is not None else x.get("status", "—")  # noqa: E731
            md.append(f"| {k} | {t} | {cell(c['S1'])} | {cell(c['S2'])} |")
    md.append("\n| treatment | S1 GiB (heap) | S2 GiB (heap) | joint S2 (cores, GiB, heap, passes) |\n|---|---|---|---|")
    for k in plan.KINDS:
        m = s[k]["mem"]
        cell = lambda x: f"{fmt(x['level'])} ({x['heap']})" if x.get("level") is not None else x.get("status", "—")  # noqa: E731
        j = s[k]["joint"].get("S2")
        jt = f"{j['cores']:g}, {j['mem']:g}, {j['heap']}, {j['passes']}/3" if j else "—"
        md.append(f"| {k} | {cell(m['S1'])} | {cell(m['S2'])} | {jt} |")
    md.append("\n## Ratios (H1 / B0)\n")
    for key, x in v["cores_ratio"].items():
        md.append(f"- cores {key}: {fmt(x['ratio'], 3)} ({x['kind']})")
    md.append(f"- GiB T2/S2: {fmt(v['gib_ratio_t2_s2']['ratio'], 3)} ({v['gib_ratio_t2_s2']['kind']})")
    u = v["units_t2_s2"]
    md.append(f"- normalized units u = max(cores, GiB/4) T2/S2: H1 {fmt(u['h1'], 2)}, B0 {fmt(u['b0'], 2)}, "
              f"ratio {fmt(u['ratio'], 3)}{' (flagged: not jointly confirmed)' if u['flagged_not_joint'] else ''}")
    md.append(f"- sensitivity shapes (non-gating): {json.dumps(v['units_sensitivity'])}")
    md.append(f"- limiting resource at T2/S2: {json.dumps(v['limiting_resource_t2_s2'])}")
    md.append("\n## Descents\n")
    for mix in ("primary", "structural", "lexical"):
        for t in plan.TIERS:
            for k in plan.KINDS:
                d = plan.cpu_dir(R, mix, t, k)
                rows = descent_table(d)
                if rows:
                    rep["descents"][f"cpu/{mix}/t{t}/{k}"] = rows
                    md.append(f"### CPU — {mix} mix, {t} QPS, {k}\n\n" + table(rows, "cores"))
    for heap in plan.HEAPS:
        for k in plan.KINDS:
            d = plan.mem_dir(R, heap, k)
            if os.path.exists(f"{d}/HEAP_EXCLUDED"):
                md.append(f"### RAM — heap {heap}, {k}: HEAP EXCLUDED (pre-pass)\n")
                continue
            rows = descent_table(d)
            if rows:
                rep["descents"][f"mem/heap{heap}/{k}"] = rows
                md.append(f"### RAM — heap {heap}, {T2} QPS, 3 cores, {k}\n\n" + table(rows, "GiB"))
    md.append("\n## Confirmation runs\n")
    for stage in ("cpu_confirm", "cpu_confirm_stepup", "mem_confirm", "mem_confirm_stepup", "joint"):
        jobs = plan.stage_jobs(R, stage)
        if not jobs:
            continue
        md.append(f"### {stage}\n\n| job | run | S1 | S2 | mode | P95 | P99 | cores used | mem GiB |\n|---|---|---|---|---|---|---|---|---|")
        for j in jobs:
            for i in (1, 2, 3):
                p = f"{R}/{stage}/{j['id']}/run{i}.json"
                try:
                    w = window_row(p)
                except (OSError, ValueError, KeyError):
                    md.append(f"| {j['id']} | {i} | fail | fail | no_result | — | — | — | — |")
                    continue
                rep.setdefault("confirm", {}).setdefault(stage, {}).setdefault(j["id"], []).append(w)
                md.append(f"| {j['id']} | {i} | {'PASS' if w['S1'] else 'fail'} | {'PASS' if w['S2'] else 'fail'} | "
                          f"{w['mode']} | {fmt(w['p95'])} | {fmt(w['p99'])} | {fmt(w['cores_used'], 2)} | {fmt(w['mem_gib'], 2)} |")
        md.append("")
    open(f"{R}/report.md", "w").write("\n".join(md) + "\n")
    json.dump(rep, open(f"{R}/report.json", "w"), indent=2, default=str)
    print(f"{v['label']} — {'; '.join(v['notes']) or 'no notes'}")


if __name__ == "__main__":
    main()
