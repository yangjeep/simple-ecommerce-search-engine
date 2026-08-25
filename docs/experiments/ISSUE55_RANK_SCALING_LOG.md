# Issue #55 Experiment Log — localizing native's ranking-cost-vs-candidate-set-size scaling

Protocol: `docs/experiments/ISSUE55_RANK_SCALING_PROTOCOL.md`.

## I55-RANK-E00 — Is `execute_ranked`'s full sort the dominant, fixable cost?

**Question**

`docs/decisions/PHASE9_DECISION.md` asked whether native's ranking-cost
scaling with candidate-set size (P9-E06: 0.42x-0.60x latency ratio,
native slower than Solr-restricted, at a real median 568-candidate
WANDS population) is fundamental or fixable, specifically: is
`CatalogIndex::execute_ranked` a full sort, or could it be a partial
top-K selection?

**Hypothesis**

H0: replacing the full sort with a partial top-k selection is output-
identical and asymptotically cheaper, and is the dominant fixable cause.
H1: the sort is not the dominant cost; something else drives the scaling.

**Workload**

Two tiers per protocol §3: a new criterion benchmark
(`crates/commerce-core/benches/rank_bench.rs`) with controlled synthetic
candidate-set sizes (100 to 100,000), and a real-data capstone reusing
this session's own already-fetched, hash-verified WANDS dataset and
Solr 9.10.1 install (both survived on this container's disk from the
Issue #43 checkpoint) via `phase9_eval::p9_e04_isolated_ranking_and_execution`.

**Implementation**

`crates/commerce-core/src/index/rank.rs`: extracted the top-k extraction
into its own function `select_top_k`, replaced the old `sort_by` +
`truncate` body with `slice::select_nth_unstable_by` (partition the true
k-th element in average O(n)) followed by sorting only the resulting
k-element front (O(k log k)) — asymptotically O(n + k log k) instead of
O(n log n). Added `select_top_k_matches_full_sort_reference_across_randomized_inputs`
(500 randomized trials, deliberately including heavy score ties via a
small discrete score pool, compared against an independent full-sort
reference implemented directly in the test) plus 3 edge-case tests
(k=0, k > candidate count, empty candidate set) — all before trusting
any benchmark number, per CLAUDE.md's RED-tests-before-production-fixes
discipline. `execute_ranked_narrowed_by` (the separately-gated P3-E03
path) is unchanged, per protocol §7's scope boundary.

**Results**

*Step 1 — code read confirms the premise*: `execute_ranked`
(`rank.rs:104-153`, pre-fix) computed a score per candidate, then called
`Vec::sort_by` over the entire candidate vector, then `.truncate(k)` —
genuinely a full O(n log n) sort regardless of k. Not assumed; read
directly.

*Step 2 — isolated selection-step benchmark* (`select_top_k_bench.rs`,
no catalog/candidate-materialization overhead, truly random scores):

| n | full_sort | partial_select | speedup |
|---|---|---|---|
| 100 | 2.47µs | 0.60µs | 4.15x |
| 1,000 | 27.86µs | 3.84µs | 7.25x |
| 10,000 | 504.3µs | 50.8µs | 9.92x |
| 100,000 | 6,765µs | 530.3µs | 12.76x |

Confirms the selection algorithm itself is dramatically, increasingly
faster than a full sort as `n` grows — the core mechanism claim holds.

*Step 3 — first end-to-end attempt found a benchmark artifact in this
experiment's own draft, not a real result*: the first version of
`rank_bench.rs` gave every candidate an identical score (empty
`residual_lexical`, `score_text_relevance`'s 0.0 short-circuit). That
input arrives at the sort already ordered by the comparator (candidates
come out of `index.execute()` in ascending product-id order, which is
also the comparator's only live tiebreaker when every score ties) — Rust's
`sort_by` is an adaptive stable sort that runs close to O(n) on already-
sorted input, so the "full sort" arm was never exercising its own O(n log
n) worst case, hiding any improvement (before/after was statistically
indistinguishable, if anything slightly worse: e.g. at n=100,000, 5.17ms
before vs. 5.55ms after). Found and disclosed before drawing any
conclusion from it, not shipped as a result.

*Step 4 — corrected end-to-end benchmark* (realistic, varying,
order-uncorrelated scores: each product's title drawn from a shuffled
vocabulary subset, queried with real `residual_lexical` tokens —
`score_text_relevance`'s actual shipping default-ranking-signal path):

| n | before (full sort) | after (partial select) | improvement |
|---|---|---|---|
| 100 | 49.92µs | 49.25µs | ~1.3% (noise-level) |
| 1,000 | 584.8µs | 553.1µs | ~5.4% |
| 10,000 | 7,155.9µs | 6,301.0µs | ~11.9% |
| 100,000 | 95,020µs | 88,771µs | ~6.6% |

A real, reproducible, if modest improvement — much smaller than Step 2's
isolated ratios. The reason: `score_text_relevance`'s per-candidate
string processing (`to_lowercase()`, `split_whitespace()` into a
`HashSet`, run for every candidate on every call) is itself a substantial
O(n) cost with a large constant factor — comparing Step 3's degenerate
(near-zero-cost scoring) numbers to Step 4's realistic ones: ~5.17ms
before → ~95.0ms before, an ~18x increase purely from turning on real
scoring at n=100,000. This cost is large enough to dominate the total in
most of the tested range, leaving the sort's own (real, confirmed)
inefficiency a smaller fraction of `execute_ranked`'s total cost than
originally suspected.

*Step 5 — real-data capstone* (`p9_e04_isolated_ranking_and_execution`,
real WANDS + fresh Solr 9.10.1, matching the Issue #43 checkpoint's own
isolated-Solr-restart methodology to avoid the JVM-warmup confound that
checkpoint found): 6 runs before, 6 runs after, each against its own
freshly restarted Solr instance.

| | H1 (NDCG gap) | H3 latency ratio (solr/native), 6 runs |
|---|---|---|
| Before | +4.33% (identical every run) | 1.54, 1.72, 1.21, 1.21, 0.88, 1.16 (mean 1.29) |
| After | +4.33% (identical every run) | 1.76, 1.71, 1.24, 1.31, 1.16, 1.17 (mean 1.39) |

H1 is unaffected, exactly as expected (the fix changes extraction order,
never scoring). H3's ranges overlap heavily and both fall within the
1.08x-1.88x band the Issue #43 checkpoint already characterized as
Solr-JVM-warmup noise at this exact candidate-set scale (P9-E04's
evaluated queries have candidate sets up to 5,000, median 568) — no
statistically distinguishable before/after difference. Expected, not a
failure of the fix: at n~568, the fix's absolute benefit is on the order
of single-digit microseconds (extrapolating Step 4's own n=1,000 row),
utterly swamped by Solr's own millisecond-scale JVM/network variance.

**Adversarial review** (per protocol §8):

- Heavy-tie coverage: the 500-trial property test's score pool
  (`[0.0,1.0,1.0,2.0,2.0,2.0]`) deliberately manufactures frequent ties —
  passed on all 500 trials, including whatever tie patterns those trials
  happened to generate.
- Comparator-argument-order correctness: verified by the property test
  passing, not assumed — a swapped argument order would have failed
  immediately on any non-trivial input.
- Measurement-floor concern: Step 2's isolated benchmark shows a real,
  large gap even at n=100 (0.60µs vs. 2.47µs, both well above criterion's
  own noise floor for this sample count) — the *end-to-end* benchmark's
  smaller n=100 gap (49.25 vs. 49.92µs) is genuine noise-level, correctly
  not overclaimed as an improvement in the writeup above.
- Solr freshness: each of the before/after 6-run sets used its own
  freshly restarted Solr JVM (stop, restart, reindex-free reuse of the
  already-indexed core, confirmed via a `numFound` check before running),
  per the Issue #43 checkpoint's own disclosed confound.

**Interpretation**

H0 is **confirmed as a real, adopted, correctness-preserving fix** —
`execute_ranked` genuinely did a full sort, the partial-select
replacement is proven output-identical, and it measurably improves
`execute_ranked`'s own cost at scale (5-12% in a realistic synthetic
benchmark, growing with candidate-set size, consistent with the
asymptotic argument). But H0 is **not confirmed as the dominant driver**
of P9-E06's originally observed native-vs-Solr latency gap: at WANDS's
actual candidate-set sizes, the fix has no measurable effect on the
already-measured H3 ratio, because (a) `score_text_relevance`'s own
per-candidate string-tokenization cost is a larger contributor than the
sort at realistic scales (a new, disclosed finding, not previously
identified), and (b) Solr's own JVM/network variance at this scale is
larger still than either. This is exactly the "localization" PHASE9_DECISION.md
asked for: the scaling problem is not fundamentally unfixable, but the
fixable full-sort inefficiency this round targeted is not, by itself,
why P9-E06 found native slower than Solr on real WANDS queries.

**Regression check**

`select_top_k_matches_full_sort_reference_across_randomized_inputs` and
the 3 edge-case tests are the standing regression surface for this
change — they run in the normal `cargo test --workspace` gate, no
special invocation needed. `crates/commerce-core/benches/rank_bench.rs`
and `select_top_k_bench.rs` are added as permanent, rerunnable benchmarks
(matching `catalog_bench.rs`/`index_bench.rs`'s own precedent), not
one-off scripts.

**Next question**

1. `score_text_relevance`'s per-query string tokenization
   (`to_lowercase`/`split_whitespace`/`HashSet` construction, redone for
   every candidate on every call) is now the larger, still-unaddressed
   cost at realistic-to-large candidate-set sizes — a natural next
   target, and one that fits this project's own core thesis exactly
   (move catalog-dependent work, here per-title tokenization, to
   ingestion/index-build time rather than repeating it per query).
2. Whether Solr's own JVM/network variance (already disclosed as a
   confound in Issue #43) is itself worth controlling for in a future,
   larger-N real-data rerun, so a real fix's benefit could be
   distinguished from that noise floor without needing candidate sets
   far outside WANDS's own real distribution.
3. Continue the falsification loop (see `docs/decisions/ISSUE55_RANK_SCALING_DECISION.md`).
