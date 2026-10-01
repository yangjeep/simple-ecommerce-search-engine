#!/usr/bin/env python3
"""Issue #66 amendment 1 section 5: candidate minimum from one descent.

A descent directory holds `descent.tsv` lines `<level> <S1> <S2> <mode> <file>`,
highest level first. The candidate minimum for SLO S is the lowest level L that
passes S with every higher tested level also passing S (monotone prefix);
"INFEASIBLE" when the highest level fails S.

    candidates.py <descent_dir> <S1|S2>          -> prints the level or INFEASIBLE
    candidates.py --step-up <ladder> <level>     -> next higher ladder level or NONE
"""
import sys

CPU_LADDER = [3, 2.5, 2, 1.5, 1, 0.75, 0.5]
MEM_LADDER = [12, 8, 7, 6, 5, 4, 3, 2, 1.5, 1]


def rows(descent_dir):
    out = []
    for line in open(f"{descent_dir}/descent.tsv"):
        level, s1, s2, *_ = line.split()
        out.append((float(level), {"S1": s1 == "PASS", "S2": s2 == "PASS"}))
    return out


def candidate(descent_dir, slo):
    best = None
    for level, ok in rows(descent_dir):
        if not ok[slo]:
            break
        best = level
    return "INFEASIBLE" if best is None else f"{best:g}"


def step_up(ladder, level):
    ladder = CPU_LADDER if ladder == "cpu" else MEM_LADDER
    higher = [x for x in ladder if x > float(level)]
    return f"{min(higher):g}" if higher else "NONE"


if __name__ == "__main__":
    if sys.argv[1] == "--step-up":
        print(step_up(sys.argv[2], sys.argv[3]))
    else:
        print(candidate(sys.argv[1], sys.argv[2]))
