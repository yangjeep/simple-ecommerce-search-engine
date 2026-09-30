#!/usr/bin/env python3
"""Issue #66 amendment 1 section 5: confirmation / step-up / joint job plans.

Jobs are TSV lines: id kind dim mix rate heap cores mem_gib slos stage.
Results live under <results>/<stage>/<id>/run{1,2,3}.json (+ .env.json).

    plan.py cpu-confirm   <results>   # candidates from the primary CPU descents
    plan.py mem-confirm   <results>   # best-heap RAM candidates (T2, 3 cores)
    plan.py stepup <stage> <results>  # one step up for every SLO confirmed < 2/3
    plan.py joint         <results>   # (min cores, min GiB, best heap) at T2
    plan.py summary       <results>   # confirmed minima -> summary.json
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from candidates import candidate, step_up  # noqa: E402
from judge import judge, load_pair  # noqa: E402

TIERS = [50, 100, 200]
T2 = 100
HEAPS = ["3g", "1g", "512m"]
KINDS = ["b0", "h1"]
SLOS = ["S1", "S2"]


def cpu_dir(r, mix, rate, kind):
    return f"{r}/cpu/{mix}/t{rate}/{kind}"


def mem_dir(r, heap, kind):
    return f"{r}/mem/heap{heap}/{kind}"


def emit(jobs):
    for j in jobs:
        print("\t".join(str(j[k]) for k in ("id", "kind", "dim", "mix", "rate", "heap", "cores", "mem", "slos", "stage")))


def read_jobs(path):
    keys = ("id", "kind", "dim", "mix", "rate", "heap", "cores", "mem", "slos", "stage")
    return [dict(zip(keys, line.rstrip("\n").split("\t"))) for line in open(path) if line.strip()]


def passes(r, job):
    """Pass count per SLO over a job's confirmation runs."""
    out = {s: 0 for s in SLOS}
    for i in (1, 2, 3):
        p = f"{r}/{job['stage']}/{job['id']}/run{i}.json"
        try:
            v = judge(*load_pair(p))
        except (OSError, ValueError, KeyError):
            continue
        for s in SLOS:
            out[s] += bool(v[s])
    return out


def group(jobs_by_level):
    """Merge SLOs that share a level into one job (one window judges both)."""
    out = []
    for (kind, dim, mix, rate, heap, cores, mem), slos in jobs_by_level.items():
        lvl = cores if dim == "cpu" else mem
        out.append({"id": f"{kind}_{dim}_{mix}_t{rate}_h{heap}_{lvl:g}", "kind": kind, "dim": dim, "mix": mix,
                    "rate": rate, "heap": heap, "cores": f"{cores:g}", "mem": f"{mem:g}",
                    "slos": ",".join(sorted(slos)), "stage": None})
    return out


def best_mem(r, kind, slo):
    """Lowest RAM candidate over the heap ladder; ties keep the larger heap."""
    best = None
    for heap in HEAPS:
        d = mem_dir(r, heap, kind)
        if not os.path.exists(f"{d}/descent.tsv") or os.path.exists(f"{d}/HEAP_EXCLUDED"):
            continue
        c = candidate(d, slo)
        if c != "INFEASIBLE" and (best is None or float(c) < best[1]):
            best = (heap, float(c))
    return best


def confirmed(r, stage_jobs, kind, dim, slo):
    """Lowest level confirmed (>= 2/3) for (kind, dim, slo) over the given jobs."""
    lv = []
    for j in stage_jobs:
        if j["kind"] == kind and j["dim"] == dim and slo in j["slos"].split(",") and passes(r, j)[slo] >= 2:
            lv.append((float(j["cores"] if dim == "cpu" else j["mem"]), j))
    return min(lv, key=lambda x: x[0]) if lv else None


def main():
    cmd, r = sys.argv[1], sys.argv[-1]
    if cmd == "cpu-confirm":
        by = {}
        for rate in TIERS:
            for kind in KINDS:
                for slo in SLOS:
                    c = candidate(cpu_dir(r, "primary", rate, kind), slo)
                    if c != "INFEASIBLE":
                        by.setdefault((kind, "cpu", "primary", rate, "3g", float(c), 12.0), set()).add(slo)
        jobs = group(by)
        for j in jobs:
            j["stage"] = "cpu_confirm"
        emit(jobs)
    elif cmd == "mem-confirm":
        by = {}
        for kind in KINDS:
            for slo in SLOS:
                b = best_mem(r, kind, slo)
                if b:
                    by.setdefault((kind, "mem", "primary", T2, b[0], 3.0, b[1]), set()).add(slo)
        jobs = group(by)
        for j in jobs:
            j["stage"] = "mem_confirm"
        emit(jobs)
    elif cmd == "stepup":
        stage = sys.argv[2]
        by = {}
        for j in read_jobs(f"{r}/{stage}/jobs.tsv"):
            p = passes(r, j)
            for slo in j["slos"].split(","):
                if p[slo] >= 2:
                    continue
                dim = j["dim"]
                nxt = step_up(dim, j["cores"] if dim == "cpu" else j["mem"])
                if nxt == "NONE":
                    continue
                cores, mem = (float(nxt), float(j["mem"])) if dim == "cpu" else (float(j["cores"]), float(nxt))
                by.setdefault((j["kind"], dim, j["mix"], int(j["rate"]), j["heap"], cores, mem), set()).add(slo)
        jobs = group(by)
        for j in jobs:
            j["stage"] = f"{stage}_stepup"
        emit(jobs)
    elif cmd == "joint":
        s = summarize(r)
        by = {}
        for kind in KINDS:
            for slo in SLOS:
                c = s[kind]["cpu"].get(f"t{T2}", {}).get(slo)
                m = s[kind]["mem"].get(slo)
                if c and m and c.get("level") is not None and m.get("level") is not None:
                    by.setdefault((kind, "joint", "primary", T2, m["heap"], c["level"], m["level"]), set()).add(slo)
        jobs = group(by)
        for j in jobs:
            j["stage"] = "joint"
        emit(jobs)
    elif cmd == "summary":
        s = summarize(r)
        json.dump(s, open(f"{r}/summary.json", "w"), indent=2)
        print(json.dumps(s, indent=2))


def stage_jobs(r, stage):
    p = f"{r}/{stage}/jobs.tsv"
    return read_jobs(p) if os.path.exists(p) else []


def summarize(r):
    """Confirmed minima per treatment: first-round confirmation, else step-up."""
    out = {}
    for kind in KINDS:
        out[kind] = {"cpu": {}, "mem": {}, "joint": {}}
        for rate in TIERS:
            for slo in SLOS:
                desc = candidate(cpu_dir(r, "primary", rate, kind), slo) if os.path.exists(
                    f"{cpu_dir(r, 'primary', rate, kind)}/descent.tsv") else None
                hit = confirmed(r, [j for j in stage_jobs(r, "cpu_confirm") if int(j["rate"]) == rate], kind, "cpu", slo) \
                    or confirmed(r, [j for j in stage_jobs(r, "cpu_confirm_stepup") if int(j["rate"]) == rate], kind, "cpu", slo)
                out[kind]["cpu"].setdefault(f"t{rate}", {})[slo] = (
                    {"level": hit[0], "job": hit[1]["id"], "stage": hit[1]["stage"]} if hit
                    else {"level": None, "descent_candidate": desc,
                          "status": "INFEASIBLE" if desc == "INFEASIBLE" else ("UNSTABLE" if desc else "NOT_RUN")})
        for slo in SLOS:
            hit = confirmed(r, stage_jobs(r, "mem_confirm"), kind, "mem", slo) \
                or confirmed(r, stage_jobs(r, "mem_confirm_stepup"), kind, "mem", slo)
            b = best_mem(r, kind, slo)
            out[kind]["mem"][slo] = (
                {"level": hit[0], "heap": hit[1]["heap"], "job": hit[1]["id"]} if hit
                else {"level": None, "descent_candidate": b, "status": "INFEASIBLE" if b is None else "UNSTABLE"})
            for j in stage_jobs(r, "joint"):
                if j["kind"] == kind and slo in j["slos"].split(","):
                    out[kind]["joint"][slo] = {"cores": float(j["cores"]), "mem": float(j["mem"]), "heap": j["heap"],
                                               "passes": passes(r, j)[slo], "jointly_feasible": passes(r, j)[slo] >= 2}
    return out


if __name__ == "__main__":
    main()
