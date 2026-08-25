# Issue #55 Decision — localizing native's ranking-cost-vs-candidate-set-size scaling

Full protocol: `docs/experiments/ISSUE55_RANK_SCALING_PROTOCOL.md`. Full
log/raw numbers: `docs/experiments/ISSUE55_RANK_SCALING_LOG.md`. Raw
artifacts: `docs/research/artifacts/i55_rank_scaling/`.

## What was tested

`docs/decisions/PHASE9_DECISION.md` asked whether P9-E06's finding —
native's `execute_ranked` cost scales with candidate-set size, producing
a 0.42x-0.60x latency ratio (native slower than Solr-restricted) on real
WANDS traffic — is fundamental or fixable, specifically asking whether
`execute_ranked` does a full sort or a partial top-K selection. Reading
the code directly confirmed a full `Vec::sort_by` over the entire
candidate set before truncating to `k`.

## Verdict: **REFINE** — the fix is real, correct, and adopted; it is not the answer to P9-E06's original gap

Replaced the full sort with `slice::select_nth_unstable_by` (partition
the true k-th element in average O(n)) followed by sorting only the
resulting k-element front — proven output-identical to the old full-sort
behavior via a 500-trial randomized property test (deliberately including
heavy score ties) plus 3 edge-case tests, all passing. In isolation
(no catalog overhead, truly random scores), the new selection is 4x-13x
faster than the old one and the gap grows with candidate-set size,
confirming the underlying O(n log n)-vs-O(n) mechanism is real.

**But this experiment also found and disclosed its own first draft's
benchmark artifact**: an initial end-to-end benchmark gave every
candidate an identical score, which made the input already sorted with
respect to the comparator — Rust's adaptive stable sort runs close to
O(n) on already-sorted data, so that benchmark's "full sort" arm never
exercised its own worst case, showing no improvement from the fix
(if anything, slightly worse). Corrected to use realistic, varying,
order-uncorrelated scores (the real default text-relevance signal), which
revealed a genuine, reproducible 5-12% improvement in `execute_ranked`'s
own end-to-end cost, growing with candidate-set size — smaller than the
isolated 4x-13x because `score_text_relevance`'s own per-candidate string
tokenization (lowercasing, whitespace-splitting, `HashSet` construction,
redone every call) is itself a larger cost driver than the sort at
realistic-to-large scales — a new finding, not previously known.

**On real WANDS data** (rerunning `p9_e04_isolated_ranking_and_execution`
before/after, each against its own freshly restarted Solr, matching the
Issue #43 checkpoint's own confound-avoidance discipline): H1 (NDCG) is
unaffected, as expected. H3's latency ratio shows **no statistically
distinguishable difference** (before mean 1.29x across 6 runs, after mean
1.39x, ranges overlapping heavily, both within the 1.08x-1.88x
Solr-JVM-warmup noise band the Issue #43 checkpoint already
characterized). This is not a failure of the fix — at WANDS's real
candidate-set sizes (median 568), the fix's absolute benefit is on the
order of single-digit microseconds, invisible against Solr's own
millisecond-scale variance.

**This precisely answers PHASE9_DECISION.md's own question**: the
scaling behavior is *not* fundamentally unfixable — the sort genuinely
was O(n log n) and is now a proven-correct O(n + k log k) — but the
fixable inefficiency this round targeted is **not, by itself, why
P9-E06 found native slower than Solr on real WANDS queries**. That gap's
real drivers are, in descending likely order: Solr's own JVM/network
variance (already disclosed in Issue #43), and `score_text_relevance`'s
own per-query string-tokenization cost (newly disclosed here) — neither
of which this experiment's scope covered fixing.

## Action taken

- `crates/commerce-core/src/index/rank.rs`: `execute_ranked` now uses
  `select_top_k` (partial selection). 6 new tests. Production code
  changed, backed by a property test proving behavioral equivalence
  before the change was trusted, per CLAUDE.md's RED-tests discipline.
- Two new permanent criterion benchmarks:
  `crates/commerce-core/benches/rank_bench.rs`,
  `crates/commerce-core/benches/select_top_k_bench.rs`.
- Full workspace quality gate (fmt, clippy `-D warnings`, tests, release
  build) passes clean.
- No GitHub issue to close — this directly answers a "what would be
  built next" item from `PHASE9_DECISION.md`, tracked there and in this
  decision, not a standalone issue.

## Architecture delta

The "faster" pillar's evidence base gains a small, real, adopted
optimization (correctness-preserving, now the shipping implementation)
and loses a plausible-but-wrong explanation for P9-E06's gap (the sort
was never the dominant cause at real WANDS scale). Two new, concrete
open threads are recorded rather than pursued in this checkpoint's own
scope: `score_text_relevance`'s per-query tokenization cost (a real
candidate for the project's own "move catalog-dependent work to
ingestion time" thesis), and whether Solr's JVM/network variance should
be controlled for more tightly in any future real-data latency-ratio
claim.
