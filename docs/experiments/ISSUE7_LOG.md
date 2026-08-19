# Issue #7 Experiment Log

Append-only, continuing the format established by `docs/experiments/LOG.md`
(Phase 0), `ROUND1_LOG.md` (Round 1, Issue #5), `PHASE2_LOG.md` (Phase 2,
Issue #6), and `REALTIME_LOG.md` (Issue #8). Issue #7 asks: deep-read
Havenask/IndexLib and the broader commerce-search market as prior art,
and for each residual hypothesis our own architecture might still need,
determine — with real measurement, not assumption — whether it is a
genuine differentiation opportunity or a mature primitive we would
otherwise reinvent.

The archaeology itself (13 research agents + 1 synthesis pass, background
Workflow `wf_81e4323f-dc0`) produced a cross-reference matrix, a 4-layer
classification (generic IR / consumer-search / commerce-domain /
marketplace-scale optimizations), and 5 ranked, falsifiable residual
hypotheses with concrete experiments — full detail in that workflow's
synthesis output (not duplicated here; see the summary each entry below
opens with). This log records the *experiments themselves*: hypothesis,
measurement, real result, decision.

Same evidence-class/independence framing as prior logs.

---

## I7-E01 — Does the already-built planner+Tantivy-delegate composition fix R1-E05's named adversarial Punt-path latency case?

**Evidence class**: real (1,215,854-product ESCI catalog, same as every
prior real-data entry in this project).

**Independence**: n/a (a latency reproduction against this project's own
prior recorded baseline, not a relevance/judgment measurement).

**Background**: the archaeology synthesis ranked "bounded top-K early
termination on the non-selective (Punt) path" the single highest-
information-value residual experiment, hypothesizing that R1-E05's
measured 36,700x-worse-than-selective-baseline degradation (961.23ms p50
for a `Text`-only query with no structural predicate, vs. 26.2µs for a
moderately-selective single-brand filter) is an artifact of *exhaustive*
linear scanning, not something inherent to bitmap-first execution — and
that adding a quota/early-termination bound should recover well past
Issue #7's revised >=5x P50/P95 bar.

Before writing any new mechanism, the synthesis's own "Obvious Wheel-
Reinvention Candidates" list flagged the risk directly: "Custom WAND/
weak-AND top-K pruning for multi-term OR lexical queries... Tantivy
already implements this internally... strengthens our decision to
delegate lexical scoring to Tantivy rather than reimplement it." R1-E05's
961ms number was measured against `CatalogIndex::execute`'s raw,
undelegated linear scan — but Issue #6 priority 5 (`commerce_core::plan`,
P2-E05) already built exactly the mechanism that avoids ever calling that
path for this query shape: a query with no structural constraint at all
routes straight to `ExecutionOutcome::Punt`, which delegates the entire
search to Tantivy instead. This had never been directly re-measured
against R1-E05's own named adversarial case — P2-E05 measured aggregate
relevance/latency across the full real query mix, not this specific
worst-case query in isolation.

**Hypothesis**: reproducing R1-E05 Case 1's exact query shape (no
structural constraint, free-text term "waterproof") through the current
planner+Tantivy-delegate path, with no new code beyond a benchmark
harness, already clears the >=5x bar — because the real cost driver in
R1-E05's number was the naive per-document substring `.contains()` check
across 1.2M products (matching R1-E07's independent finding that a
substring scan is ~6,660x slower than an indexed token lookup for a
similar reason), not "scanning" per se, and Tantivy's own inverted index
sidesteps that cost class entirely.

**Implementation**: `crates/phase2-eval/src/bin/punt_path_adversarial_eval.rs`.
Reuses `planner_integration_eval.rs`'s exact `TantivyDelegate`/schema/
build (same real catalog, same Tantivy config as P2-E01/P2-E05) and
R1-E05's own `time_iters`/percentile methodology (n=30) so the comparison
is apples-to-apples. Constructs the identical adversarial query
(`constraints: []`, `residual_lexical: ["waterproof"]`), asserts
`plan()` routes it to `Punt`, and times `execute_planned` end to end
(including `commerce_core`'s own re-verification of every returned hit —
this is not measuring Tantivy in isolation, it's measuring the real
composed path a shopper query actually goes through).

**Results** (real 1.2M-product catalog):

```
Tantivy index built in 13.9s
planner routing confirmed: Punt (as expected)
first call: 10 hits returned (k=10)

p50=1.1723ms  p95=1.4766ms  p99=1.5242ms  (n=30, k=10)

R1-E05 Case 1 (raw unbounded linear scan, no delegate):     p50=961.23ms
R1-E05 Case 3 (moderately-selective single-brand baseline): p50=0.0262ms
this experiment (same query, current planner+delegate):     p50=1.1723ms

multiplier vs. Case 1:  820.0x faster   (bar: >=5x)
multiplier vs. Case 3:  44.7x further than the selective baseline still
```

**Interpretation**: the hypothesis is confirmed, decisively — 820x past
the >=5x bar, using zero new production mechanism. The architecture Issue
#6 already built (structural-index-first, delegate-to-Tantivy-on-Punt)
was already the correct answer to R1-E05's finding; this experiment is a
missing regression/confirmation check on a specific named worst case, not
a new capability. This also *validates* rather than contradicts the
synthesis's own wheel-reinvention warning: the "right" experiment here
was recognizing existing machinery already solved it, not building a
bounded-heap collector inside `commerce_core::index` that would duplicate
what Tantivy's `TopDocs::with_limit` already does internally (per the
synthesis's cross-reference matrix, row "Bounded top-K collector / early
termination": "Tantivy already implements this internally").

The remaining, honest gap: this experiment's query has no real judged
relevance ground truth (same limitation R1-E05's own choice of
"waterproof" had — it was picked as a *physical-cost* probe, not a
relevance-quality one). The relevance side of this same code path is
already covered by P2-E05's aggregate NDCG@10/Recall@10/MRR across the
*full* real 22,458-query set (which necessarily includes every real
Punt-routed query, not just this one adversarial term) — this latency
result should be read alongside that evidence, not in isolation, per
CLAUDE.md's "do not claim a win from microbenchmarks alone."

**44.7x further than the selective baseline, a real remaining gap,
stated rather than hidden**: this experiment's 1.17ms p50 is itself
~45x slower than a genuinely selective single-brand structural filter
(26.2µs). That is expected — this query has *no* structural constraint
to narrow on, so it necessarily pays real work proportional to Tantivy's
own inverted-index term lookup + BM25 scoring + result collection over
however many real documents match "waterproof" (14,839, per R1-E05), not
some artificially small quota. This is not a new gap this experiment
introduces; it is the honest cost floor of the Punt path's *actual*
mechanism, correctly much closer to R1-E04's already-accepted Solr
baseline (p50=1486µs) and P2-E01's Tantivy-standalone number (p50=1.09ms,
Punt/full-corpus search) than to the structural-only best case, because
that is architecturally what this query shape requires.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo test
--workspace --all-features`, `cargo build --workspace --release` all
clean before this entry. No `commerce_core` or `phase2-eval` production
code changed by this experiment — new standalone benchmark binary only.

**Decision: CONFIRMED, no new mechanism needed.** Issue #7's synthesis
ranked this the highest-value residual experiment; the answer is that it
was already solved by Issue #6's own architecture, and the real
deliverable of this experiment is the missing regression evidence proving
it, plus an explicit correction to the synthesis's implied plan (build a
bounded-top-K collector) in favor of the cheaper, already-correct one
(measure what's already built). Feed into Issue #5/
`ROUND1_DECISION_TREE.md`: R1-E05's ~36,700x finding is now a *closed*
finding for the current architecture (post-Issue-6), not an open risk —
record this explicitly so it is not re-discovered as a surprise later.

**Next**: proceed to the remaining ranked hypotheses (#2 columnar RSS
reduction, #3 numeric-range-as-bitmap, #4 tiered ranking, #5 mmap
cold-start/degradation) — none of which this experiment's finding
resolves, since each targets a materially different subsystem (RSS,
compound-constraint planning, ranking quality, and storage tiering
respectively, vs. this entry's pure lexical-Punt-latency question).
