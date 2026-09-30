#!/usr/bin/env python3
"""Issue #65 (Infra E5, amendment 1) analysis -> artifacts/issue65/results/report.{json,md}.

Implements amendment sections 11-13 as preregistered:
- confirmed max sustainable QPS = highest of {0.95 Q*, Q*, 1.05 Q*} that PASSes
  in >= 2 of 3 confirmations (else UNSTABLE);
- QPS/core = confirmed max / 3; CPU/query = total serving-slice CPU / ok queries
  at R_c = min(Q*_B0, Q*_H1), median over the confirmations;
- q = H1 QPS/core / B0 QPS/core; c = H1 CPU/query / B0 CPU/query;
- verdict: BROAD (q >= 1.25 or c <= 0.75, SLO/quality equivalent, H1 P99 < SLO at
  its confirmed max), MODEST / MIX-DEPENDENT, NO MATERIAL, NEGATIVE.
Sensitivity mixes are reported per mix and never averaged into the primary.
"""
import json
import os
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
R = Path(os.environ.get("I65_RESULTS", ROOT / "artifacts/issue65/results"))
CORES = 3


def load(p):
    return json.loads(Path(p).read_text())


def points(d):
    out = []
    for f in sorted(Path(d).glob("rate_*.json")):
        j = load(f)
        out.append(j)
    return sorted(out, key=lambda j: j["offered_qps"])


def qstar(d):
    """Q* of a search dir; "none" (first point failed) -> None."""
    p = Path(d) / "qstar.txt"
    if not p.exists():
        return None
    t = p.read_text().strip()
    return None if t == "none" else float(t)


def curve_rows(d):
    rows = []
    for j in points(d):
        o = j["overall"]
        rows.append({
            "offered": j["offered_qps"], "achieved": j["achieved_qps"], "achieved_over_offered": o["achieved_over_offered"],
            "p50": o["p50_ms"], "p95": o["p95_ms"], "p99": o["p99_ms"], "error_rate": o["error_rate"],
            "cpu_util": j.get("total_cpu_utilization_of_3_cores"), "cpu_us_per_q": j.get("total_cpu_us_per_ok_query"),
            "gen_cpu": j["generator_cpu_share"], "late_p99_ms": j["dispatch_lateness_p99_ms"],
            "saturated": j["harness_saturated"], "pass": j["pass"],
        })
    return rows


def fmt(v, nd=1):
    if v is None:
        return "—"
    if isinstance(v, bool):
        return "yes" if v else "no"
    return f"{v:,.{nd}f}"


def curve_md(title, d):
    rows = curve_rows(d)
    lines = [f"#### {title} (Q* = {qstar(d)})", "",
             "| offered QPS | achieved QPS | ach/off | P50 ms | P95 ms | P99 ms | err | CPU util (of 3) | CPU µs/q | gen CPU | late P99 ms | sat | PASS |",
             "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for r in rows:
        lines.append(
            f"| {fmt(r['offered'])} | {fmt(r['achieved'])} | {fmt(r['achieved_over_offered'], 3)} | {fmt(r['p50'], 2)} | "
            f"{fmt(r['p95'], 2)} | {fmt(r['p99'], 2)} | {fmt(r['error_rate'], 4)} | {fmt(r['cpu_util'], 3)} | "
            f"{fmt(r['cpu_us_per_q'], 0)} | {fmt(r['gen_cpu'], 2)} | {fmt(r['late_p99_ms'], 2)} | {fmt(r['saturated'])} | {fmt(r['pass'])} |")
    return lines + [""]


def confirm_summary(name="confirm"):
    plan = (R / f"{name}/plan.txt").read_text().split()
    q = {k: float(v) for k, v in (x.split("=") for x in plan)}
    scale = q.get("scale", 1.0)
    out = {"plan": q, "treatments": {}}
    for t in ("b0", "h1"):
        qs = round(q["Q*_B0" if t == "b0" else "Q*_H1"] * scale, 2)
        per_label = {"rc": [], "lo": [], "mid": [], "hi": []}
        for run in (1, 2, 3):
            d = R / f"{name}/run{run}/{t}"
            for label in per_label:
                files = list(d.glob(f"{label}_*.json"))
                if files:
                    per_label[label].append(load(files[0]))
        rates = {"lo": round(qs * 0.95, 1), "mid": qs, "hi": round(qs * 1.05, 1)}
        passes = {lab: sum(1 for j in per_label[lab] if j["pass"]) for lab in ("lo", "mid", "hi")}
        confirmed = None
        for lab in ("hi", "mid", "lo"):
            if qs > 0 and passes[lab] >= 2:
                confirmed = rates[lab]
                break
        infeasible = qs == 0  # clarification C1: no sustainable rate >= 1.8 QPS
        if infeasible:
            confirmed = 0.0
        rc = per_label["rc"]
        cpuq = statistics.median([j["total_cpu_us_per_ok_query"] for j in rc]) if rc else None
        best = per_label[{v: k for k, v in rates.items()}.get(confirmed, "mid")] if confirmed else []
        tail = statistics.median([j["overall"]["p99_ms"] for j in best]) if best else None
        out["treatments"][t] = {
            "qstar": q["Q*_B0" if t == "b0" else "Q*_H1"], "ladder_base": qs, "rates": rates, "passes": passes, "confirmed_max_qps": confirmed,
            "slo_infeasible": infeasible,
            "rc_pass": [j["pass"] for j in rc],
            "qps_per_core": confirmed / CORES if confirmed is not None else None,
            "cpu_us_per_query_at_rc": cpuq, "rc_points": [j["overall"] for j in rc],
            "p99_at_confirmed_max_ms": tail,
            "confirmed_points": [{"pass": j["pass"], **j["overall"], "cpu_util": j.get("total_cpu_utilization_of_3_cores"),
                                  "memory": j["memory_current_bytes"], "per_class": j["per_class"], "per_route": j["per_route"]} for j in best],
        }
    return out


def verdict(conf, sens):
    """Precedence (fixed before results, see ISSUE65_LOG.md): NEGATIVE, then
    BROAD, then MODEST, then NO MATERIAL. 'Improvement' means q > 1 or c < 1."""
    b, h = conf["treatments"]["b0"], conf["treatments"]["h1"]
    if h["confirmed_max_qps"] is None:
        return {"verdict": "UNSTABLE (H1 has no confirmed max)", "q": None, "c": None}
    c = h["cpu_us_per_query_at_rc"] / b["cpu_us_per_query_at_rc"]
    q_bound = None
    if b["slo_infeasible"]:
        # Clarification C1: q undefined; reported as a feasibility difference.
        q = None
    elif b["confirmed_max_qps"] is None:
        # B0 unconfirmed after both rounds: its search Q* (with a FAIL just
        # above it) is an upper bound on its max, so H1/B0 >= this bound.
        q = None
        q_bound = h["confirmed_max_qps"] / b["qstar"] if b["qstar"] else None
    else:
        q = h["qps_per_core"] / b["qps_per_core"]
    tail_ok = h["p99_at_confirmed_max_ms"] is not None and h["p99_at_confirmed_max_ms"] < 100.0
    clears = (q is not None and q >= 1.25) or (q_bound is not None and q_bound >= 1.25) or c <= 0.75
    # Sensitivity holds if H1's Q* exceeds B0's on both mixes (no sustainable
    # rate counts as 0, per clarification C1).
    sens_q = {m: {"h1": v.get("h1") or 0.0, "b0": v.get("b0") or 0.0} for m, v in sens.items()}
    sens_hold = bool(sens_q) and all(x["h1"] > x["b0"] for x in sens_q.values())
    if (q is not None and q <= 0.8) or c >= 1.25:
        v = "NEGATIVE"
    elif clears and tail_ok and sens_hold:
        v = "BROAD CAPACITY ADVANTAGE"
    elif clears or (q is not None and q > 1.0) or c < 1.0:
        v = "MODEST / MIX-DEPENDENT ADVANTAGE"
    else:
        v = "NO MATERIAL ADVANTAGE"
    return {"verdict": v, "q": q, "q_lower_bound": q_bound, "c": c, "tail_ok": tail_ok, "primary_clears_bar": clears,
            "sensitivity_q": sens_q, "sensitivity_hold_both": sens_hold}


def main():
    lines = ["# Issue #65 — mixed-workload total-serving-system capacity", ""]
    report = {}
    if (R / "confirm/plan.txt").exists():
        rounds = {"confirm": confirm_summary("confirm")}
        if (R / "confirm2/plan.txt").exists():
            rounds["confirm2"] = confirm_summary("confirm2")
        # Section 11: a treatment whose round-1 ladder is UNSTABLE uses its
        # round-2 (ladder lowered 10%) confirmation.
        conf = {"plan": rounds["confirm"]["plan"], "treatments": {}}
        for t in ("b0", "h1"):
            r1 = rounds["confirm"]["treatments"][t]
            if r1["confirmed_max_qps"] is None and "confirm2" in rounds:
                conf["treatments"][t] = {**rounds["confirm2"]["treatments"][t], "round": 2}
            else:
                conf["treatments"][t] = {**r1, "round": 1}
            # CPU/query at R_c: median over every confirmation launch.
            rc_all = [p for rd in rounds.values() for p in [rd["treatments"][t]["cpu_us_per_query_at_rc"]] if p]
            conf["treatments"][t]["cpu_us_per_query_at_rc_by_round"] = rc_all
        report["rounds"] = rounds
        sens = {}
        for mix in ("structural", "lexical"):
            sens[mix] = {t: qstar(R / f"sensitivity/{mix}/{t}") for t in ("b0", "h1")}
        v = verdict(conf, sens)
        report.update({"confirm": conf, "sensitivity_qstar": sens, "verdict": v})
        lines += [f"**Verdict (primary mix): {v['verdict']}** — q = {fmt(v['q'], 3)}, c = {fmt(v['c'], 3)}", "",
                  "Rounds: " + "; ".join(f"{t.upper()} used round {conf['treatments'][t]['round']}" for t in ("b0", "h1")), "",
                  "## Capacity table (primary mix; confirmed over 3 counterbalanced runs)", "",
                  "| treatment | Q* (search) | confirmed max QPS | QPS/core | CPU µs/query at R_c | PASS counts (0.95/1/1.05 Q*) | P99 at confirmed max ms |",
                  "|---|---|---|---|---|---|---|"]
        for t in ("b0", "h1"):
            x = conf["treatments"][t]
            p = x["passes"]
            lines.append(f"| {t.upper()} | {fmt(x['qstar'])} | {fmt(x['confirmed_max_qps'])} | {fmt(x['qps_per_core'])} | "
                         f"{fmt(x['cpu_us_per_query_at_rc'], 0)} | {p['lo']}/{p['mid']}/{p['hi']} | {fmt(x['p99_at_confirmed_max_ms'], 2)} |")
        lines += ["", f"R_c = {conf['plan']['R_c']} QPS.", "",
                  "Sensitivity (single search each): " + "; ".join(
                      f"{m}: B0 Q* {fmt(s['b0'])}, H1 Q* {fmt(s['h1'])}" for m, s in sens.items()), ""]
    for label, d in [("B0 primary search", R / "search/b0"), ("H1 primary search", R / "search/h1")]:
        if d.exists():
            lines += curve_md(label, d)
    for mix in ("structural", "lexical"):
        for t in ("b0", "h1"):
            d = R / f"sensitivity/{mix}/{t}"
            if d.exists():
                lines += curve_md(f"{t.upper()} {mix} mix", d)
    for t in ("b0", "h1"):
        d = R / f"control/{t}"
        if d.exists():
            lines += curve_md(f"{t.upper()} no-lexical control", d)
    lines += ["## Native concurrency scaling (native_plp mix, native alone in the slice)", "",
              "| config | Q* | ratio vs W=1 |", "|---|---|---|"]
    base = qstar(R / "scaling/w1")
    for cfg in ("n0", "w1", "w2", "w3"):
        q = qstar(R / f"scaling/{cfg}")
        lines.append(f"| {cfg} | {fmt(q)} | {fmt(q / base, 2) if q and base else '—'} |")
    lines += [""]
    for cfg in ("n0", "w1", "w2", "w3"):
        d = R / f"scaling/{cfg}"
        if d.exists():
            lines += curve_md(f"scaling {cfg}", d)
    lines += ["## Transport calibration (diagnostic)", ""]
    for name in ("native_noop", "solr_health", "router_to_solr_health"):
        d = R / f"calibrate/{name}"
        if d.exists():
            lines += curve_md(f"calibration {name}", d)
    (R / "report.json").write_text(json.dumps(report, indent=2, default=str) + "\n")
    (R / "report.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
