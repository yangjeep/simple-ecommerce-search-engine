#!/usr/bin/env python3
"""Synthetic tests for Issue #66's judge / candidate / plan logic.

    python3 scripts/issue66/test_plan.py
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from candidates import candidate, step_up  # noqa: E402
from judge import judge  # noqa: E402
import plan  # noqa: E402


def load(p95, p99, err=0.0, ach=1.0, sat=False, kinds=None):
    return {"overall": {"p95_ms": p95, "p99_ms": p99, "error_rate": err, "achieved_over_offered": ach},
            "harness_saturated": sat, "error_kinds": kinds or {}}


def write_run(path, p95, p99, side=None):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    json.dump(load(p95, p99), open(path, "w"))
    json.dump(side or {"alive": True, "oom_kill_delta": 0, "throttled_share": 0.0},
              open(path[:-5] + ".env.json", "w"))


class Judge(unittest.TestCase):
    def test_slos(self):
        self.assertEqual(judge(load(40, 90)), {"S1": True, "S2": True, "mode": "none"})
        r = judge(load(60, 150))
        self.assertEqual((r["S1"], r["S2"], r["mode"]), (False, True, "latency"))
        r = judge(load(120, 150))
        self.assertEqual((r["S1"], r["S2"]), (False, False))

    def test_shared_conditions_fail_both(self):
        for bad in (load(10, 20, err=0.01), load(10, 20, ach=0.9), load(10, 20, sat=True),
                    load(10, 20, kinds={"check": 1})):
            r = judge(bad)
            self.assertEqual((r["S1"], r["S2"]), (False, False))

    def test_death_and_oom(self):
        r = judge(load(10, 20), {"alive": False, "oom_kill_delta": 1})
        self.assertEqual((r["S1"], r["S2"], r["mode"]), (False, False, "oom"))
        r = judge(load(10, 20), {"alive": False, "oom_kill_delta": 0})
        self.assertEqual(r["mode"], "process_death")


class Candidates(unittest.TestCase):
    def descent(self, rows):
        d = tempfile.mkdtemp()
        with open(f"{d}/descent.tsv", "w") as f:
            for level, s1, s2 in rows:
                f.write(f"{level} {s1} {s2} x level_{level}.json\n")
        return d

    def test_monotone_prefix(self):
        d = self.descent([(3, "FAIL", "PASS"), (2.5, "PASS", "PASS"), (2, "FAIL", "PASS"), (1.5, "FAIL", "FAIL"),
                          (1, "FAIL", "PASS")])
        self.assertEqual(candidate(d, "S1"), "INFEASIBLE")  # highest level fails S1
        self.assertEqual(candidate(d, "S2"), "2")  # 1 passes S2 but 1.5 failed: not a prefix

    def test_step_up(self):
        self.assertEqual(step_up("cpu", "0.75"), "1")
        self.assertEqual(step_up("cpu", "3"), "NONE")
        self.assertEqual(step_up("mem", "1.5"), "2")


class Plan(unittest.TestCase):
    def setUp(self):
        self.r = tempfile.mkdtemp()

    def desc(self, d, rows):
        os.makedirs(d, exist_ok=True)
        with open(f"{d}/descent.tsv", "w") as f:
            for level, s1, s2 in rows:
                f.write(f"{level} {s1} {s2} x f\n")

    def run_plan(self, *args):
        return subprocess.run([sys.executable, f"{HERE}/plan.py", *args, self.r], check=True,
                              capture_output=True, text=True).stdout

    def test_cpu_confirm_groups_shared_levels_and_skips_infeasible(self):
        for rate in plan.TIERS:
            self.desc(plan.cpu_dir(self.r, "primary", rate, "b0"), [(3, "FAIL", "PASS"), (2.5, "FAIL", "FAIL"),
                                                                     (2, "FAIL", "FAIL")])
            self.desc(plan.cpu_dir(self.r, "primary", rate, "h1"), [(3, "PASS", "PASS"), (2.5, "PASS", "PASS"),
                                                                     (2, "FAIL", "FAIL"), (1.5, "FAIL", "FAIL")])
        jobs = [line.split("\t") for line in self.run_plan("cpu-confirm").splitlines()]
        # b0: S2 at 3 only (S1 infeasible); h1: S1 and S2 share 2.5 -> one job.
        self.assertEqual(len(jobs), 6)
        h1 = [j for j in jobs if j[1] == "h1"]
        self.assertTrue(all(j[6] == "2.5" and j[8] == "S1,S2" for j in h1))
        b0 = [j for j in jobs if j[1] == "b0"]
        self.assertTrue(all(j[6] == "3" and j[8] == "S2" for j in b0))

    def test_stepup_and_summary(self):
        for rate in plan.TIERS:
            for k in plan.KINDS:
                self.desc(plan.cpu_dir(self.r, "primary", rate, k), [(3, "PASS", "PASS"), (2, "PASS", "PASS"),
                                                                      (1.5, "FAIL", "FAIL")])
        jobs = self.run_plan("cpu-confirm")
        os.makedirs(f"{self.r}/cpu_confirm", exist_ok=True)
        open(f"{self.r}/cpu_confirm/jobs.tsv", "w").write(jobs)
        for line in jobs.splitlines():
            jid = line.split("\t")[0]
            good = line.split("\t")[1] == "h1"
            for i in (1, 2, 3):
                # h1 confirms (3/3); b0 passes S2 but fails S1 in 2 of 3 runs.
                p95 = 40 if good or i == 1 else 60
                write_run(f"{self.r}/cpu_confirm/{jid}/run{i}.json", p95, 90)
        step = [line.split("\t") for line in self.run_plan("stepup", "cpu_confirm").splitlines()]
        self.assertEqual({(j[1], j[6], j[8]) for j in step}, {("b0", "2.5", "S1")})
        s = plan.summarize(self.r)
        self.assertEqual(s["h1"]["cpu"]["t100"]["S1"]["level"], 2.0)
        self.assertEqual(s["b0"]["cpu"]["t100"]["S2"]["level"], 2.0)
        self.assertIsNone(s["b0"]["cpu"]["t100"]["S1"]["level"])
        self.assertEqual(s["b0"]["cpu"]["t100"]["S1"]["status"], "UNSTABLE")

    def test_best_heap_prefers_lowest_candidate_and_skips_excluded(self):
        self.desc(plan.mem_dir(self.r, "3g", "b0"), [(12, "PASS", "PASS"), (8, "PASS", "PASS"), (7, "PASS", "PASS"),
                                                     (6, "PASS", "PASS"), (5, "PASS", "PASS"), (4, "PASS", "PASS"),
                                                     (3, "FAIL", "FAIL")])
        self.desc(plan.mem_dir(self.r, "1g", "b0"), [(12, "PASS", "PASS"), (8, "PASS", "PASS"), (7, "PASS", "PASS"),
                                                     (6, "PASS", "PASS"), (5, "PASS", "PASS"), (4, "PASS", "PASS"),
                                                     (3, "PASS", "PASS"), (2, "PASS", "PASS"), (1.5, "FAIL", "FAIL")])
        self.desc(plan.mem_dir(self.r, "512m", "b0"), [(12, "PASS", "PASS"), (1, "PASS", "PASS")])
        open(f"{plan.mem_dir(self.r, '512m', 'b0')}/HEAP_EXCLUDED", "w").write("x")
        self.assertEqual(plan.best_mem(self.r, "b0", "S2"), ("1g", 2.0))



def summary(cpu, mem, joint=None):
    """cpu[kind][rate][slo] -> level|None ('INFEASIBLE' marks status); mem[kind][slo] -> level."""
    out = {}
    for k in plan.KINDS:
        out[k] = {"cpu": {}, "mem": {}, "joint": {}}
        for t in plan.TIERS:
            for slo in plan.SLOS:
                lv = cpu[k][t][slo]
                out[k]["cpu"].setdefault(f"t{t}", {})[slo] = (
                    {"level": None, "status": "INFEASIBLE"} if lv == "INF" else {"level": lv})
        for slo in plan.SLOS:
            out[k]["mem"][slo] = {"level": mem[k][slo], "heap": "1g"}
        if joint:
            out[k]["joint"]["S2"] = {"cores": joint[k][0], "mem": joint[k][1], "heap": "1g",
                                     "passes": 3, "jointly_feasible": joint[k][2]}
    return out


def flat(level_by_tier, s1="INF"):
    return {t: {"S1": s1, "S2": level_by_tier[t]} for t in plan.TIERS}


class Verdict(unittest.TestCase):
    def setUp(self):
        import analyze_i66
        self.v = analyze_i66.verdict

    def test_keep_on_cores_with_robustness(self):
        s = summary({"b0": flat({50: 1.5, 100: 2, 200: 3}), "h1": flat({50: 1, 100: 1, 200: 2})},
                    {"b0": {"S1": 2, "S2": 2}, "h1": {"S1": 6, "S2": 6}},
                    {"b0": (2, 2, True), "h1": (1, 6, True)})
        r = self.v(s)
        self.assertEqual(r["label"], "KEEP")
        self.assertEqual(r["limiting_resource_t2_s2"], {"b0": "cpu", "h1": "ram"})
        self.assertAlmostEqual(r["units_t2_s2"]["ratio"], 1.5 / 2)

    def test_refine_when_cores_only_at_t2(self):
        # u: H1 max(1, 8/4) = 2 vs B0 max(2, 0.5) = 2 -> ratio 1 (not met).
        s = summary({"b0": flat({50: 1, 100: 2, 200: 2}), "h1": flat({50: 1, 100: 1, 200: 2})},
                    {"b0": {"S1": 2, "S2": 2}, "h1": {"S1": 8, "S2": 8}},
                    {"b0": (2, 2, True), "h1": (1, 8, True)})
        self.assertEqual(self.v(s)["label"], "REFINE")

    def test_reject(self):
        s = summary({"b0": flat({50: 1, 100: 1, 200: 2}, s1=1), "h1": flat({50: 1, 100: 1, 200: 2}, s1=1)},
                    {"b0": {"S1": 2, "S2": 2}, "h1": {"S1": 6, "S2": 6}},
                    {"b0": (1, 2, True), "h1": (1, 6, True)})
        self.assertEqual(self.v(s)["label"], "REJECT")

    def test_s1_bound_alone_is_refine_not_keep(self):
        s = summary({"b0": flat({50: 1, 100: 1, 200: 2}), "h1": flat({50: 1, 100: 1, 200: 2}, s1=1)},
                    {"b0": {"S1": 2, "S2": 2}, "h1": {"S1": 6, "S2": 6}},
                    {"b0": (1, 2, True), "h1": (1, 6, True)})
        self.assertEqual(self.v(s)["label"], "REFINE")

    def test_flagged_units_cannot_keep(self):
        # u ratio 0.5 but not jointly confirmed (H1 joint fails) -> REFINE.
        s = summary({"b0": flat({50: 1, 100: 2, 200: 2}), "h1": flat({50: 1, 100: 1, 200: 2})},
                    {"b0": {"S1": 12, "S2": 12}, "h1": {"S1": 4, "S2": 4}},
                    {"b0": (2, 12, True), "h1": (1, 4, False)})
        r = self.v(s)
        # GiB ratio 4/12 <= 0.75 still keeps on the RAM criterion.
        self.assertEqual(r["label"], "KEEP")
        self.assertTrue(r["units_t2_s2"]["flagged_not_joint"])



class FEquiv(unittest.TestCase):
    def test_tie_aware(self):
        from f_equiv import equivalent
        a = {"num_found": 10, "hits": [["x", 5.0], ["y", 3.0], ["z", 3.0], ["w", 1.0]]}
        # Reordered ties above the boundary: equivalent.
        self.assertTrue(equivalent(a, {"num_found": 10, "hits": [["x", 5.0], ["z", 3.0], ["y", 3.0], ["w", 1.0]]})[0])
        # Different id at the boundary score (tied group cut at top-K): equivalent.
        self.assertTrue(equivalent(a, {"num_found": 10, "hits": [["x", 5.0], ["y", 3.0], ["z", 3.0], ["v", 1.0]]})[0])
        # Different id above the boundary: not equivalent.
        self.assertEqual(equivalent(a, {"num_found": 10, "hits": [["x", 5.0], ["y", 3.0], ["q", 3.0], ["w", 1.0]]}),
                         (False, "ids_above_boundary"))
        # Score drift or num_found change: not equivalent.
        self.assertEqual(equivalent(a, {"num_found": 10, "hits": [["x", 5.1], ["y", 3.0], ["z", 3.0], ["w", 1.0]]}),
                         (False, "scores"))
        self.assertEqual(equivalent(a, {"num_found": 11, "hits": a["hits"]}), (False, "num_found/len"))


if __name__ == "__main__":
    unittest.main()
