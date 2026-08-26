# Issue #35 Experiment Log — second unseen-vertical slice: real ESCI automotive-parts data

Protocol: `docs/experiments/ISSUE35_ESCI_AUTOMOTIVE_PROTOCOL.md`.

## I35-ESCI-AUTO-E00 — H0 confirmed: checkpoint 13's finding replicates on a second, independent real vertical

**Refactor before new measurement**: `issue35-eval`'s measurement
procedure was extracted from `esci_electronics_eval.rs` into a shared
`issue35_eval::eval::run_vertical_eval` function (parameterized on
dataset paths and a label), so a second vertical reuses identical
measurement code rather than a hand-copied near-duplicate. Verified
byte-identical reproduction of checkpoint 13's own numbers
(routing distribution, correctness gate, NDCG figures to 4 decimal
places) after the extraction, before trusting the refactor.

**Dataset acquisition**: `scripts/datasets/fetch_esci_automotive.sh`
(same pinned HF revision as the electronics slice, an independent
115MB download for full experiment self-containedness) +
`scripts/datasets/filter_esci_automotive.py` (fixed 20-term
automotive-parts keyword list, disclosed before any metric was
inspected). Result: **1,056 real products, 600 real queries**, label
distribution `{Substitute: 383, Exact: 1032, Irrelevant: 309,
Complement: 52}`, 503/600 queries with >=1 non-Irrelevant judgment.

**Disclosed keyword-match noise, consistent with the electronics
checkpoint's own precedent**: spot-checked two off-topic products
directly rather than assuming the slice was clean -- a "Kimono Robe"
matched via the word "Muffler" (a scarf-style garment feature, not a
vehicle part) and a bean-bag-chair cover matched via "Seat Cover" (a
furniture accessory, not a car seat cover). Both are genuine substring
matches on legitimately ambiguous English words, not a data-construction
bug; expected and disclosed, not silently cleaned up.

**Zero `commerce-core` changes required** (same code, same crate, no
edits between the two vertical slices):

```
catalog: 1056 products, 502 distinct brands discovered
routing distribution: {"FastPath": 4, "Hybrid": 33, "Punt": 563}
queries with ambiguity: 12/600
queries with a Brand structural constraint: 37/600
```

**Correctness (hard gate)**: `PASS` -- zero wrong-family violations
across the 37 Brand-constrained queries.

**Relevance** (n=503 queries, real Solr core `esci_automotive_bench`):

```
native NDCG@10=0.4396  solr NDCG@10=0.4511
relative gap (native vs solr): -2.55%
```

**H0 CONFIRMED**, comfortably inside the <=15% bar. Unlike the
electronics slice (where native was *better* than Solr, +8.93%), native
is here slightly *worse* (-2.55%) -- a more balanced, credible result
across the two verticals than "native always wins," and still a clean
pass. Qualitative sample spot-checked directly: "acdelco" -> real
ACDelco oil filters/brake calipers/alternators; "detroit axle wheel
bearing" -> real Detroit Axle wheel-bearing assemblies; "fh group seat
covers neoprene" -> real FH Group seat covers -- all semantically
correct, brand-exact matches.

**Brand-collision check** (following checkpoint 13's own diligence, not
skipped): of 502 discovered brand strings, **zero** collide with the
same 16-word English-stopword list checked before (`{is, a, an, the,
of, to, in, on, at, for, and, or, not, no, it, be}`), vs. 1/1,079 in
the electronics slice. Consistent with that prior finding being a rare,
isolated event rather than a systemic pattern -- not contradicted, not
overstated, by this second data point.

## Decision

See `docs/decisions/ISSUE35_ESCI_AUTOMOTIVE_DECISION.md`.
