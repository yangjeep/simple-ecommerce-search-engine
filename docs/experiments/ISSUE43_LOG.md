# Issue #43 Experiment Log — Phase 9 determinism re-audit

Protocol: `docs/experiments/ISSUE43_PROTOCOL.md` (includes 3 dated
addenda recorded after an adversarial review; read those alongside this
log — they correct two drafting errors and add a follow-up treatment).

## I43-E00 — Rerun P9-E01/E02/E04 against the determinism-fixed `bitmap_delegate`

**Question**

Every currently-published Phase 9 headline number (P9-E01's 11.6–12.1x
bitmap-delegate speedup; P9-E02/E06's structural-routed NDCG gap and
latency ratios) was generated before commit `449c22f` fixed
`phase9_eval::bitmap_delegate::build_index`'s non-deterministic
multi-threaded Tantivy indexing (confirmed via `git merge-base
--is-ancestor`; see protocol §0). Do those numbers still hold once
regenerated against the fixed, single-threaded code?

**Hypothesis**

H0 (numbers reproduce): the fix only affects score-tie-break ordering,
which should not move aggregate NDCG/latency figures outside their
already-reported variance bands. H1 (numbers move): this project has a
demonstrated instance of the same bug moving a headline NDCG figure by a
non-trivial margin elsewhere (Issue #38's E2 binary, 0.5386 vs 0.5869),
so H1 is not speculative.

**Workload**

Real WANDS catalog (42,994 products, 480 queries, 233,448 judgments),
refetched fresh this round, hash-verified against
`scripts/datasets/wands_checksums.sha256` (see protocol §8 for full
provenance). Solr 9.10.1 stood up as a bare JVM process (Docker daemon
unreachable in this environment — confirmed via `docker info`; network
access to `downloads.apache.org` and `raw.githubusercontent.com` both
worked, so no dataset was blocked this round).

**Metrics / decision rule**

See protocol §5/§6 (as corrected by the addenda).

**Implementation**

None — all three binaries (`p9_e01_bitmap_vs_termset_delegate`,
`p9_e02_wands_physical_advantage`, `p9_e04_isolated_ranking_and_execution`)
ran unmodified from current HEAD (`crates/phase9-eval` release build).
This experiment is a rerun, not a code change.

**Results**

*T1 — P9-E01, 6 runs, synthetic 500k-doc/60k-restrict microbenchmark, no
Solr needed* (`docs/research/artifacts/i43_e00_phase9_determinism_reaudit/p9_e01_run{1..6}.txt`):

| Run | Speedup (termset/bitmap) | Same doc set (within-run, bitmap vs termset) |
|---|---|---|
| 1 | 11.80x | true |
| 2 | 11.49x | true |
| 3 | 11.71x | true |
| 4 | 11.82x | true |
| 5 | 11.84x | true |
| 6 | 12.25x | true |

Published band: 11.56x–11.96x (`artifacts/manifests/p9_e01_bitmap_vs_termset_delegate.json`).
This rerun's range (11.49x–12.25x) overlaps but is not strictly a subset
of the published band — 2 of 6 runs land slightly outside on each end.
Given this is wall-clock latency on different underlying hardware than
whatever ran the original 6 published runs (never recorded), this reads
as ordinary cross-environment timing variance, not a qualitative change:
the mechanism-level advantage (>2x bar, in fact >11x) is confirmed every
run. **Correction (protocol addendum 2)**: "same doc set" here is a
within-run check (bitmap arm vs termset arm on one index build) — it
does not by itself prove cross-run indexing determinism. That is
established below, by T2/T3 instead.

*T2 — P9-E02, 3 runs, real WANDS + fresh Solr* (`.../p9_e02_run{1,2,3}.txt`):

| Run | Routing (FastPath/Hybrid/Punt) | structural_routed native NDCG@10 | solr NDCG@10 | relative gap | latency ratio (solr/native) |
|---|---|---|---|---|---|
| 1 | 7/14/459 | 0.2953 | 0.3939 | -25.05% | 3.58x |
| 2 | 7/14/459 | 0.2953 | 0.3939 | -25.05% | 2.35x |
| 3 | 7/14/459 | 0.2953 | 0.3939 | -25.05% | 3.63x |

Routing distribution and NDCG figures are **byte-identical across all 3
independent runs** (each rebuilds the Tantivy index from scratch) *and*
byte-identical to the already-published post-P9-E05-fix figures in
`docs/research/artifacts/p9_e06_corrected_baseline_rerun/p9_e02_after_run*.txt`
(routing 7/14/459, NDCG 0.2953/0.3939, gap -25.05%). This is the direct
cross-run indexing-determinism confirmation P9-E01 could not itself
provide. Latency ratio range (2.35x–3.63x) closely overlaps the original
post-fix range (2.15x–3.63x, from the same reference files) — still
clears the project's >=2x latency bar in every run.

*T3 — P9-E04, first pass: 6 runs, immediately after T2 on the same,
unrestarted Solr JVM* (`.../p9_e04_run{1..6}.txt`):

| Run | H1 native NDCG@10 | H1 solr NDCG@10 | H1 relative gap | H3 latency ratio (solr/native) |
|---|---|---|---|---|
| 1 | 0.4586 | 0.4396 | +4.33% | 0.96x |
| 2 | 0.4586 | 0.4396 | +4.33% | 0.90x |
| 3 | 0.4586 | 0.4396 | +4.33% | 0.79x |
| 4 | 0.4586 | 0.4396 | +4.33% | 0.63x |
| 5 | 0.4586 | 0.4396 | +4.33% | 0.77x |
| 6 | 0.4586 | 0.4396 | +4.33% | 1.11x |

H1's NDCG/gap is byte-identical across all 6 runs and byte-identical to
`p9_e06_corrected_baseline_rerun/p9_e04_after_run*.txt` (+4.33%). H3's
latency ratio (0.63x–1.11x) does **not** overlap the original published
band (0.42x–0.60x, same reference files) — a real, measured shift.

**Adversarial review** (full agent report preserved in this checkpoint's
session record; key points reproduced here per this project's "record the
finding" discipline):

- NDCG byte-identical reproduction verified directly against the raw
  files (not just trusted from a summary) — holds.
- `Cargo.lock` `tantivy`/`roaring` versions identical between the original
  P9-E06 commit and current HEAD (0.22.1 / 0.10.12) — rules out a
  dependency-drift confound.
- No other binary in the repository imports `phase9_eval::bitmap_delegate`
  beyond the three already covered — re-audit scope is complete.
- **Found**: T1's "same document set" claim is within-run, not cross-run
  (protocol addendum 2).
- **Found**: T2's originally-drafted KEEP-band text cited the wrong,
  superseded NDCG figures (protocol addendum 1).
- **Found, and the most consequential**: a single, unrestarted Solr JVM
  process served all of T2 (1,440 queries) immediately before all of T3
  (90 queries) in the first pass — violating the protocol's own "fresh,
  same-run" checklist item. Measured effect: Solr-side latency in T3
  roughly doubled relative to the original published run (~1.93ms →
  ~3.80ms mean) while native latency rose only ~21% — a T3-specific,
  sequencing-specific asymmetry, not generic hardware noise.
- **Found**: `p9_e04_isolated_ranking_and_execution.rs` does not import
  `bitmap_delegate` or `tantivy` at all — its native arm is pure
  `commerce_core::index::CatalogIndex`/`plan`. The Issue #43 fix
  mechanically **cannot** explain any H3 drift; whatever moved H3 has a
  different cause entirely.

*T3 — follow-up, isolated: 6 runs, Solr stopped/restarted fresh,
catalog freshly reindexed, no prior query traffic on that JVM*
(`docs/research/artifacts/i43_e00_phase9_determinism_reaudit_t3_isolated/p9_e04_run{1..6}.txt`):

| Run | H1 relative gap | H3 latency ratio (solr/native) |
|---|---|---|
| 1 | +4.33% (identical) | 1.88x |
| 2 | +4.33% (identical) | 1.56x |
| 3 | +4.33% (identical) | 1.46x |
| 4 | +4.33% (identical) | 1.08x |
| 5 | +4.33% (identical) | 1.13x |
| 6 | +4.33% (identical) | 1.16x |

H1 is unchanged (as expected — it is deterministic and unaffected by
Solr JVM state). H3 with a **cold** Solr JVM lands at 1.08x–1.88x,
decreasing monotonically-ish across the 6 runs as Solr's JIT warms from
cumulative query volume across this second session's own repeated runs
(the JVM itself was not restarted between these 6 sub-runs, only once
before run 1) — the mirror image of what a JIT-warmup story predicts.
Three genuinely different H3 readings now exist for the *identical*
code and dataset:

| Condition | H3 range | Native vs Solr |
|---|---|---|
| Originally published (P9-E06, JVM history unknown/undisclosed) | 0.42x–0.60x | native slower |
| This round, T3 run immediately after T2 (warm carryover) | 0.63x–1.11x | mixed, mostly native slower |
| This round, T3 on a freshly restarted, cold JVM | 1.08x–1.88x | mostly native faster |

The qualitative H3 verdict — native fails to clear the project's >=2x
speed bar — holds in **every one of the 18 measurements** across all
three conditions. The direction (native faster or slower than Solr) and
magnitude do not. This is a genuine, previously-undisclosed finding: the
original P9-E06 decision text presented a single 0.42x–0.60x band as if
it were a controlled, portable measurement, but Solr JVM warm-state
(itself a function of undocumented prior query volume on the same
long-lived process) was never controlled or disclosed as a variable, and
it swings the ratio by more than 3x across observed conditions. This is
**not** an Issue #43 finding (P9-E04 never touches the fixed code path)
— it is a new, separate methodological gap in P9-E04's own benchmark
discipline, recorded here as a new open thread rather than silently
folded into the Issue #43 verdict.

**Interpretation**

Everything the Issue #43 fix could plausibly affect — indexing-dependent
NDCG/relevance figures, computed via `phase9_eval::bitmap_delegate` — is
**byte-identical reproducible** across every rerun (T1's mechanism-level
speedup within normal cross-environment variance; T2's structural-routed
NDCG and gap exact; T3's H1 NDCG and gap exact). No qualitative verdict
in Phase 9's published decision record depends on a number that moved.
The one metric that did move materially (T3's H3 latency ratio) is
provably unrelated to the fix (the code path doesn't even touch
`bitmap_delegate`) and is instead explained by a newly-discovered,
disclosed Solr-JVM-warmup confound in P9-E04's own methodology.

**Regression check**

`crates/phase9-eval`'s existing binaries remain the regression surface;
this experiment adds no new test code (rerun only). The determinism fix
itself already has its own regression protection in `issue38-e2e3-eval`
(5x reruns, byte-identical, per `ISSUE38_DECISION.md`).

**Next question**

1. File the H3/Solr-JVM-warmup confound as a new open thread for any
   future P9-E04-derived work: control/record Solr JVM warm-state
   explicitly (fixed warmup-query count before any measured pass,
   restarted per run or the warmup size documented) before citing an H3
   latency multiplier again.
2. Continue the falsification loop: select and preregister the next
   highest-information experiment per Issue #55 (see
   `docs/decisions/ISSUE43_DECISION.md` for the closing verdict and the
   loop's next selection).
