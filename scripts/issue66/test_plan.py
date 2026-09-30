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


if __name__ == "__main__":
    unittest.main()
