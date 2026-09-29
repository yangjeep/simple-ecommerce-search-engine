#!/usr/bin/env python3
"""Issue #64: the 44 cell names, comma-separated, in the order of
`issue77_eval::i64cells::cells()`. run_i64.sh guards the count, and the
per-arm equivalence check fails on any cell whose dump is missing."""
scopes = {"s0": 0, "s1": 1, "s2": 1, "s3": 2, "s4": 3, "s5": 3}
names = []
for s, d in scopes.items():
    kmax = 11 if s == "s0" else 5 + 6 - d
    ks = sorted({k for k in (1, 3, 5, 8, kmax) if k <= kmax})
    names += [f"i64_{s}_k0"] + [f"i64_{s}_k{k}" for k in ks] + [f"i64_{s}_color"]
    if s in ("s2", "s3"):
        names += [f"i64_{s}_k5_style1", f"i64_{s}_k5_style2"]
print(",".join(names))
