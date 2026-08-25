# Issue #55 Preregistered Protocol — localizing native's ranking-cost-vs-candidate-set-size scaling

Committed before any production code is changed or any benchmark is run,
per this repository's governance.

## 0. What this is testing

`docs/decisions/PHASE9_DECISION.md`'s "what would be built next" item 2
names this directly: P9-E06 found native's `execute_ranked` cost scales
with candidate-set size, producing a 0.42x-0.60x latency ratio (native
SLOWER than Solr-restricted) once a realistic, entity-anchored candidate
set (median 568) is used, instead of the earlier degenerate near-parity
reading. PHASE9_DECISION.md explicitly asks: "profile
`CatalogIndex::execute_ranked` directly (is it a full sort, or could it
be a partial top-K selection?) before Issue #38 E1 treats this as a
settled physical-execution-advantage input." Reading
`crates/commerce-core/src/index/rank.rs:104-153` directly (not assumed)
confirms `execute_ranked` computes a score for every candidate, then
calls `Vec::sort_by` over the **entire** candidate vector, then
`.truncate(k)` — a full O(n log n) sort regardless of how small `k` is.
This experiment localizes whether that full sort is the dominant,
fixable cause of the scaling finding, or whether the scaling survives
even after it is removed (evidence of a different, more fundamental
cost).

## 1. Hypothesis

**H0 (fixable, full-sort is the dominant cost)**: replacing the full sort
with a partial top-k selection (bounded by `k`, not candidate-set size
`n`) produces byte-identical output (same top-k set, same order, same
tie-breaking) while asymptotically reducing cost for `n >> k` — the
realistic regime P9-E06's 568-median candidate sets already sit in, and
which only grows for larger/less-selective structural constraints.
**H1 (not fully fixable at this scope)**: after removing the full sort,
`execute_ranked`'s cost still scales materially with `n` — e.g. because
per-candidate scoring itself, candidate materialization
(`CatalogIndex::execute`), or `lookup_variant` dominates instead — a
genuine, disclosed negative result, not assumed away.

## 2. Baseline

Current branch HEAD. `crates/commerce-core/src/index/rank.rs` is
production code — CLAUDE.md's "Add RED correctness/regression tests
before production fixes where practical" applies directly. This is
treated as an optimization with a provable equivalence argument (a
partial top-k selection under the same comparator yields the identical
sorted top-k prefix as a full sort followed by truncation), not a
speculative behavioral change — but the equivalence is *tested*, not
merely asserted by argument.

## 3. Dataset

Two tiers, matching this project's own "isolate the mechanism, then
confirm on real data" discipline:

- **Synthetic, controlled** (`crates/commerce-core/benches/rank_bench.rs`,
  new): deterministic `ChaCha8Rng`-seeded candidate sets of varying size
  (100 to 100,000), reusing `benches/common::synthetic_catalog`'s own
  generator convention. Isolates `execute_ranked`'s own cost as a
  function of candidate-set size with `k` fixed at 10 — a controlled
  breakpoint/scaling analysis, exactly the use CLAUDE.md's synthetic-data
  allowance names ("controlled selectivity/cardinality experiments,
  breakpoint analysis").
- **Real** (if the synthetic result confirms H0): rerun
  `phase9_eval`'s `p9_e04_isolated_ranking_and_execution` (H3's own
  identical-candidate-set isolation, real WANDS data + fresh Solr) before
  and after the fix, to see whether the fix changes the real, already-
  measured -0.42x-0.60x-class finding at the actual 568-median candidate-
  set size this project has already found in production data. WANDS raw
  data and the Solr 9.10.1 install already exist on disk from the Issue
  #43 checkpoint this session's container retained; refetching/rebuilding
  is not required unless verification shows staleness.

## 4. Treatment

Replace `execute_ranked`'s `scored.sort_by(...); scored.truncate(k);`
with a partial selection: compute all scores as today (unchanged), then
use `slice::select_nth_unstable_by` to partition the `k`-th smallest
element into place (average O(n)), then sort only the resulting
`k`-element front slice (O(k log k)) — asymptotically O(n + k log k)
instead of O(n log n), with `k` a small constant (10) in every real
query. Comparator (score descending, then `(product_id, variant_id)`
ascending) is copied verbatim from the current implementation — this
experiment does not change ranking semantics, only how the top-k is
extracted.

## 5. Metrics

- Correctness: byte-identical output (same `Vec<RankedHit>`, same order)
  between the old (full-sort) and new (partial-select) implementations,
  across many randomly generated candidate sets (varying size, score
  distributions including heavy ties, and `k` values) — a property-style
  equivalence test, not a single hand-picked case.
- Synthetic performance: `execute_ranked`'s own wall-clock cost as a
  function of candidate-set size, before and after, via `criterion`
  (matching `index_bench.rs`'s own existing convention).
- Real performance (if pursued): P9-E04's own H3 metrics (native vs.
  Solr-restricted latency ratio on the identical, real candidate set),
  before and after, using the exact same methodology that produced the
  0.42x-0.60x figure (and the Issue #43 checkpoint's own follow-up
  isolated-Solr-JVM rerun, to avoid reintroducing that already-found
  confound).

## 6. Preregistered gates

- **KEEP (H0 confirmed, fix adopted)**: the partial-select implementation
  is proven output-identical to the full-sort implementation across the
  property test suite (zero divergences), AND the synthetic benchmark
  shows materially sub-`n log n` scaling (e.g., near-linear or better)
  where the old implementation showed `n log n`-consistent growth. If the
  real-data rerun is also pursued and shows the latency ratio moving
  materially toward or past parity with Solr at the real 568-median
  candidate-set size, that is recorded as strong confirmatory evidence,
  but is not required for KEEP on the localization question itself (this
  experiment's primary question is "is this fixable," which the
  synthetic result alone can answer).
- **REFINE (partially fixable)**: the fix is correct and improves scaling
  materially, but the real-data rerun shows another cost still dominates
  at the actual candidate-set sizes commerce_core sees in production —
  recorded precisely, not glossed into a clean win.
- **REJECT (H1 — not fixable at this scope)**: the property tests reveal
  the partial-select approach cannot be made output-identical without a
  more invasive change, or the synthetic benchmark shows no material
  scaling improvement (e.g., another O(n) cost dominates the sort's own
  contribution even asymptotically) — a genuine, disclosed negative
  result.

No threshold above is adjusted after results are read.

## 7. Scope boundary

This targets `execute_ranked` only — the function
`docs/decisions/PHASE9_DECISION.md` explicitly named. It does not touch
`execute_ranked_narrowed_by` (P3-E03's separately-admission-gated path,
which sorts by `(product_id, variant_id)` only, not by score, so the
same optimization does not obviously apply the same way and is out of
scope here). It does not add a new ranking feature or change what gets
ranked — only how the top-k extraction is computed.

## 8. Adversarial review checklist (applied before KEEP is recorded)

- Does the property-equivalence test suite actually exercise heavy ties
  (many candidates with identical scores), since tie-breaking order is
  exactly where a partial-selection algorithm most commonly diverges from
  a full stable/unstable sort's own behavior?
- Is `select_nth_unstable_by`'s own comparator argument order correct
  (ascending vs. descending) — verified by a passing test, not assumed?
- Does the benchmark's own measurement floor (per this project's own
  P1-D/Issue #38 precedent) actually resolve a real difference at the
  smallest tested candidate-set sizes, or is the improvement only visible
  once `n` is large enough to clear the timer floor?
- If the real-data rerun is pursued: is the Solr baseline genuinely
  fresh/isolated, per the Solr-JVM-warmup confound the Issue #43
  checkpoint already found and disclosed for this exact binary?
