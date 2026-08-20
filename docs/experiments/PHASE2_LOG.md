# Phase 2 Experiment Log

Append-only, continuing the format established by `docs/experiments/LOG.md`
(Phase 0) and `docs/experiments/ROUND1_LOG.md` (Round 1, Issue #5). Phase 2
(Issue #6) executes the NARROW THE PRODUCT branch selected in
`ROUND1_DECISION_TREE.md`: keep the typed structural/facet retrieval core
and the ambiguity-preserving compiler, delegate lexical retrieval and
ranking to an embedded Tantivy index rather than building that from
scratch (`docs/adr/0008-narrow-to-structural-planning-layer.md`). If
evidence here contradicts an earlier entry (from any log), a new entry
records it — nothing in any experiment log is rewritten.

Same evidence-class/independence framing as `ROUND1_LOG.md`: **Evidence
class** (`real`/`synthetic`/`hand-authored`) and **Independence** (was the
query set built independently of the thing being measured) are required
per entry.

---

## P2-E01 — Does an embedded Tantivy index recover Solr's real relevance numbers?

**Evidence class**: real (same 1,215,854-product ESCI catalog and
22,458-query real human-judgment set used throughout Round 1).
**Independence**: yes — same real, third-party ESCI test-split judgments
as every Round 1 relevance measurement; Tantivy's scoring is unrelated to
and independent of both the query set and the judgment set.

**Question**: ADR 0008's central, falsifiable bet is that delegating
lexical retrieval/ranking to Tantivy — rather than continuing to grow
`commerce_core`'s own substring/token matching into a ranking engine
(R1-E07's finding: retrieval is cheap to build, ranking is the actual
gap) — recovers real relevance quality. Does it, measured against the
exact same real catalog and real ESCI judgments Solr was measured
against in R1-E04?

**Hypothesis**: an embedded Tantivy index, using its default BM25
scoring and a competently-configured (not hand-tuned, not
hand-crippled) `TEXT` field over the same title+description+bullets
content Solr's `all_text` copy field covered, will produce NDCG@10,
Recall@10, MRR, and zero-result-rate numbers close to R1-E04's Solr
baseline — because both are, at their core, mature Lucene-lineage BM25
implementations over comparable indexed content, not because Tantivy is
expected to be dramatically better or worse than a system built on the
same underlying retrieval theory.

**Decision threshold**: per ADR 0008's own consequence, "if it does not
[recover relevance close to Solr's], this ADR's decision should be
revisited before further integration work proceeds." No specific
numeric tolerance was fixed in advance beyond "close" — this is
deliberately not a hairsplitting threshold, since the real question is
qualitative (does delegating to a mature engine actually work in this
environment, in-process, with a competent-but-untuned configuration) not
whether Tantivy beats Solr by some margin.

**Implementation**  
New crate `crates/phase2-eval` (depends on `commerce-core` and
`round1-eval` path-wise; reuses `round1_eval::data`'s existing real-data
JSONL loaders read-only — no changes to `commerce_core` or `round1-eval`
from this crate). `crates/phase2-eval/src/bin/tantivy_relevance_eval.rs`:
schema `id: STRING | STORED` (real ASIN, for joining back to judgments),
`all_text: TEXT` (title + description + bullets concatenated, matching
what Solr's `all_text` copy field covered in R1-E04); Tantivy's default
tokenizer (lowercasing + simple alphanumeric tokenization) and default
BM25 similarity — no synonym lists, no custom scoring, no field boosts,
matching R1-E04's own "standard, competently configured... not
hand-crippled" standard for the Solr side. Queries parsed via
`QueryParser::parse_query_lenient` (tolerant of the same special
characters — `#`, `$`, `"` — that broke Solr's *default* Lucene parser
in R1-E04, forcing that experiment to edismax; Tantivy's lenient parser
is the equivalent competent choice here, not a workaround unique to this
experiment). Metric definitions (graded relevance E=3/S=2/C=1/I=0,
NDCG@10 with `log2(i+2)` discount and an ideal ranking built from *all*
of that query's judged grades, Recall@10 against Exact+Substitute, MRR,
zero-result rate) are copied exactly from `scripts/round1/solr_bench.py`
so the two numbers are directly comparable, not just similarly named.
Evaluated against the **full** 22,458-query set (every query with at
least one Exact/Substitute judgment) rather than R1-E04's 1,000-query
`random.Random(seed=7)` sample — Python's Mersenne-Twister-seeded
`random.sample` cannot be reproduced bit-for-bit in Rust, and the full
set is strictly stronger evidence once an in-process query makes it
computationally tractable (it did: ~4 seconds for all 22,458 queries).

**Results** (same environment as R1-E01 through R1-E07: 4 vCPU Intel
Xeon @2.80GHz, 15 GiB RAM, Linux 6.18.5; single run, deterministic
pipeline):

```
Indexing (1,215,854 real products):
  Tantivy (in-process, default writer heap 512MB): 19.9s
  (R1-E04 Solr baseline, HTTP bulk JSON, 5k-doc batches: 321.1s --
   not apples-to-apples, includes network+JSON overhead Tantivy's
   in-process add_document call doesn't pay)

Index footprint:
  Tantivy on-disk index: 565,724,910 bytes (565.7 MB)
  (R1-E04 Solr baseline: 1.9 GB on-disk -- Tantivy's is smaller, though
   Solr's schema also stored separate brand/color fields this one
   doesn't)

Relevance (FULL 22,458-query real set, all queries with >=1 real
Exact/Substitute judgment):
  zero-result rate: 0.6% (125/22,458)
  NDCG@10:          0.3033
  Recall@10:        0.1801
  MRR:              0.4838

  (R1-E04 Solr baseline, 1,000-query sample:
   zero-result rate=0.2%, NDCG@10=0.3052, Recall@10=0.1811, MRR=0.4910)

Query latency (in-process, n=22,458):
  p50=1.0905ms  p95=6.2243ms  p99=22.2627ms
  (R1-E04 Solr baseline, Python HTTP round-trip: p50=1486us;
   Solr's own server-reported QTime: 0-5ms)
```

**Interpretation**

**Confirmed, decisively and closely.** NDCG@10 (0.3033 vs. 0.3052),
Recall@10 (0.1801 vs. 0.1811), and MRR (0.4838 vs. 0.4910) are all
within roughly 0.6-1.5% relative of Solr's numbers — for two
independently-configured systems evaluated on different query samples
(the full 22,458-query set here vs. R1-E04's 1,000-query random sample),
this is a striking, not merely directional, confirmation. Zero-result
rate is slightly higher (0.6% vs. 0.2%) but both are low; the difference
is plausibly explained by evaluating the full query set here (including
whatever long-tail queries a 1,000-query random sample happened not to
include) rather than a like-for-like discrepancy. This is exactly the
outcome ADR 0008 bet on: **a competently-configured, un-tuned, default
BM25 implementation recovers real relevance quality close to another
competently-configured, un-tuned, default BM25 implementation**, because
both are built on the same well-understood retrieval theory a mature
engine already implements correctly. It is direct, positive evidence
that R1-E07's conclusion ("ranking, not retrieval, is the differentiator,
and it's exactly what a mature engine already provides") was correctly
diagnosed, not merely plausible.

The index footprint result is a secondary, additional point in Tantivy's
favor: 565.7 MB on disk vs. Solr's 1.9 GB for a comparable (if not
identical-schema) corpus, and indexing completed in 19.9s in-process vs.
Solr's 321.1s over HTTP (not a fair latency comparison given the
HTTP/JSON overhead difference, but a real, favorable data point for an
embedded, in-process architecture with no separate server process to
run). Query latency (p50=1.09ms) is in the same rough range as Solr's
HTTP-measured figure (1.49ms) despite paying zero network/serialization
overhead — a fully fair comparison isn't possible here either (this
number includes real BM25 scoring + top-10 retrieval + stored-field
lookup, work Solr's number also includes), but it is not a red flag: a
production system embedding this index directly (no HTTP layer between
the query planner and the lexical engine at all) would plausibly do
better still, though that specific claim is not measured by this
experiment and should not be asserted beyond what was actually run.

**What this confirms and what it does not**: this experiment validates
that *delegating* lexical retrieval/ranking to Tantivy is viable and
recovers real relevance quality — the specific, falsifiable bet ADR 0008
made. It does **not** yet validate the *integration* of this delegated
path with `commerce_core`'s structural/facet layer (how the two compose
at query time — e.g. does a genuinely selective structural predicate
narrow the candidate set *before* Tantivy scores it, or does Tantivy
receive the full free-text query independently and results get merged —
this is explicitly Issue #6 priority 5, not addressed here). It also
does not validate the canonicalization stage (Issue #6 priority 2) or
the precision-aware promotion gate (priority 3), both still open.

**Caveats**: single run (deterministic index/query pipeline — no
variance to characterize, matching R1-E02's precedent for deterministic
pipelines). Tantivy's default tokenizer (lowercase + simple alphanumeric
split) is not identical to Solr's `text_general` analyzer chain
(`StandardTokenizer` + `LowerCaseFilter` + `StopFilter`) — "competent
defaults on both sides," not "identical configuration," is the fair
framing, matching R1-E04's own caveat about Solr's schema not being
hand-tuned. The full-22,458-query evaluation here and R1-E04's
1,000-query sample are not the *same* queries, so this is a comparison
of two representative measurements on overlapping-but-not-identical
samples of the same real query population, not a paired test — flagged
per this log's own "never compare results generated from different
query sets without labeling the comparison invalid or adjusted" rule
(inherited from `docs/EXPERIMENT_LOOP.md`). Index-size comparison is
schema-asymmetric (Solr's schema also stores separate filterable
brand/color fields this experiment's schema does not) — a real but
partial explanation for some of the size difference, not a fully
controlled comparison.

**Regression check**: none yet — `phase2-eval` is a new, standalone
experiment crate with no test suite of its own (mirrors `round1-eval`'s
precedent: an experiment harness over `commerce_core`/`round1_eval`,
neither of which this entry modifies). `commerce-core`'s 36 tests remain
green throughout (unaffected — `phase2-eval` only depends on it,
read-only).

**Next question**: ADR 0008's central bet holds — Issue #6's remaining
priorities (canonicalization stage, precision-aware promotion gate,
compiler fixes for R1-E03's disjunction/negation bugs, and the
structural-plus-delegated-Tantivy integration design) are now
appropriately unblocked to proceed, rather than needing the decision
tree revisited first. Priority 2 (canonicalization) is attacked next,
since it directly targets R1-E02/E02b's single most severe finding
(5.0% filter recall against real Exact-labeled products).

---

## P2-E02 — A frequency-threshold canonicalization stage more than doubles real filter recall

**Evidence class**: real (same 1,215,854-product catalog and
22,458-query real ESCI judgment set as R1-E01 through P2-E01).
**Independence**: yes — same real, third-party held-out queries/judgments
as every other real-data measurement in this project; the threshold
itself is a structural, catalog-derived signal, not tuned against these
specific judgments before being evaluated on them (each threshold value
was evaluated once, not selected by fitting to this test set).

**Question**: R1-E02/E02b's most severe finding was that a
cold-start lexicon built by trusting every distinct raw catalog `color`
value produces a **5.0%** filter recall against real Exact-labeled
relevant products — traced to extraction quality (noisy, one-off,
non-categorical field values indexed with the same 1.0 confidence as
genuine values), not aggregation logic. Issue #6 priority 2 and ADR 0008
both call for a canonicalization/validation stage before a raw catalog
value becomes a trusted lexicon entry. Does the simplest deterministic,
zero-model-call signal available — how many times a value recurs across
the catalog — actually fix this, and at what cost to coverage (Semantic
FIB hit rate) and precision?

**Hypothesis**: a genuine controlled-vocabulary value (a real color name,
a real brand) gets reused across many products; a one-off data-entry
mistake (the R1-E02 diagnostic's "#2", "Without Lids", "10 Gallon")
typically does not. Requiring a minimum occurrence count before an enum
value becomes a trusted lexicon entry will raise filter recall
substantially (more than R1-E02b's rejected OR-within-attribute fix,
which only moved Exact recall 5.0% -> 6.0%) without a comparable
precision cost, because the mechanism targets the actual root cause
(extraction quality) rather than working around it (aggregation logic).

**Decision threshold**: "substantially" is calibrated against R1-E02b's
already-rejected fix as the floor to beat (a ~20% relative improvement
was rejected as insufficient) — a canonicalization threshold that only
matches or modestly exceeds R1-E02b's result would not be different
enough to call this root-cause fix confirmed rather than another
marginal mitigation.

**Implementation**  
`commerce_core::cold_start::CatalogProfile` gains `enum_occurrence:
BTreeMap<String, usize>` (incremented once per catalog attribute
occurrence, alongside the existing dedup-by-source bookkeeping) and a
public `enum_occurrence_count` accessor. `compile_lexicon` gains a
required `min_enum_frequency: usize` parameter: an enum/multi-enum
value's entire lexicon entry (all its candidates together — filtering is
per-value, not per-candidate) is skipped unless its combined occurrence
count meets the threshold. `min_enum_frequency=1` means "no filtering" —
every Phase 0 test fixture call site was updated to pass `1` explicitly,
preserving exact prior behavior (verified: all pre-existing tests still
pass unmodified). Brand/product-type/category/boolean vocabulary is
**not** subject to this threshold — those come from an already-curated
registry, not raw per-product field values, matching the actual root
cause R1-E01/E02 identified (the `color` field specifically). A new
regression test, `min_enum_frequency_excludes_one_off_values_but_keeps_repeated_ones`
(`crates/commerce-core/tests/cold_start.rs`), uses `cold_start_catalog`'s
known occurrence counts (verified via `enum_occurrence_count` directly in
the test, not assumed) to confirm threshold 2 excludes a value seen once
("red") while keeping values seen twice ("black", and the deliberately
planted "green" collision, which correctly stays *ambiguous*, not
resolved to one arbitrary candidate) resolvable, and confirms
brand/product-type resolution is unaffected. `crates/phase2-eval/src/bin/canonicalization_eval.rs`
sweeps `min_enum_frequency` in {1, 2, 3, 5, 10, 25, 50, 100, 250} against
the real catalog/query set, reusing `round1_eval::classify`'s
classification and `measure_precision` (existing `AggregationRule::ExistingAnd`,
the actual compiler's real aggregation rule) unmodified.

**Results** (same environment as R1-E01 through P2-E01; single
deterministic run, ~0.4-0.5s per threshold after the catalog/profile
build):

```
 threshold  fib_rate  ambig_rate  punt_rate  precision  recall_ES  recall_Exact
         1     55.4%      38.4%       2.5%      94.5%       4.3%        5.0%   (baseline, R1-E02)
         2     48.4%      41.7%       2.8%      93.3%       6.1%        7.1%
         3     47.9%      39.4%       2.9%      93.5%       7.2%        8.4%
         5     48.2%      36.6%       3.0%      93.2%       8.2%        9.5%
        10     48.5%      33.3%       3.0%      93.2%       9.2%       10.7%
        25     55.6%      21.4%       3.1%      92.3%       9.7%       11.2%   <- recall peak
        50     59.8%      16.0%       3.1%      92.3%       9.2%       10.6%
       100     61.8%      13.0%       3.1%      92.2%       9.1%       10.5%
       250     64.0%      10.0%       3.1%      92.2%       8.9%       10.2%
```

**Interpretation**

**Confirmed, decisively, and the fix is substantially stronger than
R1-E02b's rejected alternative.** Exact-label filter recall more than
**doubles** (5.0% -> 11.2% at its peak, threshold=25; still 10.2-10.7%
across the whole threshold >= 10 range) — a ~2.2x improvement, well past
R1-E02b's rejected ~1.2x (5.0% -> 6.0%) result, confirming the
hypothesis that extraction-quality canonicalization is the dominant
lever, not aggregation logic (consistent with R1-E02b's own root-cause
diagnosis). Precision costs almost nothing (94.5% -> ~92.2-93.5% across
the whole sweep, a 1-2 point absolute change) — the filter stays
precise, it just now also *retains* far more of what it should. Ambiguity
rate — R1-E02's other major finding (38.4% at baseline, largely
accidental collisions among six-figure noisy vocabulary) — falls sharply
at high thresholds (down to 10.0% at threshold=250), meaning most of
that ambiguity really was a symptom of untrusted, low-frequency
vocabulary polluting the lexicon, not genuine multi-attribute collisions
like the deliberately-planted Phase 0 case.

**A real, non-monotonic wrinkle worth explaining rather than glossing
over**: ambiguity rate does not fall monotonically with threshold — it
*rises* from 38.4% to 41.7% between threshold 1 and 2, before falling
at higher thresholds. This is not noise or a bug: `ir::query::compile`
matches the *longest* phrase window first (`crates/commerce-core/src/ir/query.rs`).
When a low-frequency multi-word lexicon entry (e.g. a two-word noisy
"color" value) is filtered out at a given threshold, the compiler falls
through to trying *shorter* sub-windows within the same span — and a
shorter, single-word substring can land on a genuinely ambiguous entry
that the longer (now-removed) phrase had been silently shadowing. This
is a real, mechanistic, second-order effect of greedy longest-match
compilation interacting with frequency filtering, not a flaw in the
threshold approach itself (the *net* effect across the full sweep is
still a large ambiguity reduction) — flagged here so a future reader
does not mistake the small threshold=1-to-2 uptick for a regression.

**Semantic FIB hit rate follows a U-shape, and the far end is
informative**: it dips to 47.9% around threshold 3 (removing noisy
entries converts some illusory `structural_only` resolutions into safe
`residual`/`ambiguous` outcomes) before *exceeding* the unfiltered
baseline at threshold >= 25 (64.0% at threshold 250) — because at
aggressive thresholds, the surviving lexicon entries are overwhelmingly
genuine, high-frequency, real-world-common values (actual frequently-sold
colors/brands), which resolve unambiguously and correctly far more
often than the six-figure long tail of one-off noise ever did. This is
consistent with real commerce catalogs' vocabulary being Zipfian: a
small set of true common values accounts for a large share of
occurrences.

**What this does not claim**: even at its best (11.2%), Exact-label
filter recall remains far below a ranked lexical engine's Recall@10
(P2-E01: Tantivy 0.1801, i.e. 18.01%) — this experiment fixes a specific,
severe defect in the structural path's *own* filter recall, it does not
and is not intended to make structural retrieval a substitute for
delegated lexical ranking on the queries it cannot resolve, consistent
with ADR 0008's narrowed scope (structural retrieval owns the confident,
validated subset; Tantivy owns the rest). Recall also does not increase
monotonically forever — it peaks around threshold 25 and drifts slightly
down at 50-250 (11.2% -> 10.2%), meaning very aggressive thresholds trade
a small amount of real recall for further ambiguity/FIB-rate improvement;
threshold selection is a real tradeoff, not a "higher is strictly
better" free win.

**Caveats**: single run per threshold (deterministic pipeline, no
variance to characterize). The sweep only tests one canonicalization
signal (occurrence frequency); other signals named in Issue #6 (e.g. a
"does this look like a genuine categorical value" content heuristic) are
not tested here and could plausibly compose with frequency filtering for
a further improvement — not attempted this entry, since frequency alone
already clears the "substantially stronger than R1-E02b" bar this
entry's decision threshold set. No single threshold value is adopted as
"the" production default in this entry — that is a downstream
integration decision (Issue #6 priority 5) informed by, but not settled
by, this sweep.

**Regression check**: `commerce-core`'s test suite grew from 36 to 37
tests (1 new: the canonicalization-behavior regression test described
above), all green; every pre-existing test's expected output is
unchanged (`min_enum_frequency=1` call sites are a verified no-op).
Verified via `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `cargo test --workspace
--all-features`, `cargo build --workspace --release`.

**Next question**: Issue #6 priority 3 (a precision-aware promotion gate
for the control plane, fixing R1-E06's structural safety gap) is the
next highest-value item — it is independent of this entry's
canonicalization work (different subsystem: control-plane promotion, not
cold-start lexicon construction) and was flagged as equally severe in
`ROUND1_DECISION_TREE.md`.

---

## P2-E03 — A precision-aware promotion gate closes R1-E06's structural safety gap, after a real-data-caught loophole in the first design

**Evidence class**: real (same catalog/queries/judgments; the fixture
regression tests added to `commerce-core` are hand-authored, correctness-
only, and labeled as such). **Independence**: yes — the oracle backing
the real-data validation run is the same real, third-party ESCI
judgments used throughout this project, not generated by this fix.

**Question**: R1-E06 found `try_promote`'s coverage-only replay gate is
mathematically unable to reject a mapping for a never-before-seen term
regardless of whether the mapping means anything — proven with a real
control experiment (mapping the most frequent real residual term to an
unrelated `waterproof=true` constraint; accepted, "resolving" 42 queries
to nonsense). Issue #6 priority 3 and ADR 0008 call for a precision-aware
check using held-out relevance judgments. Does adding one actually reject
R1-E06's exact real control experiment, without blocking genuine fixes
that happen to lack judgment evidence?

**Hypothesis**: a second, independent gate — replay a candidate's
*newly-resolved* queries (the only ones a mapping actually changes)
against real relevance judgments, reject if what the resolution matches
isn't relevant — will reject R1-E06's real nonsensical mapping while
still accepting the already-proven genuine fix from Gate 5's own test
suite (adidas/blue), and must not penalize a query the oracle simply has
no data for (absence of evidence is not evidence of a problem).

**Decision threshold**: binary and unambiguous — replaying R1-E06's exact
real control experiment (same catalog, same lexicon, same naive
provider) through the new gate must produce REJECTED, matching the
"if punt rate falls only because the model guesses aggressively, reject
the approach" instruction the original brief stated and R1-E06 found
violated.

**Implementation**  
`commerce_core::control_plane` gains a new `precision` module: a
`PrecisionOracle` trait (`commerce_core` defines the mechanism, has no
concept of real relevance judgments itself — mirrors `ModelProvider`'s
existing design exactly), a `Judgment` type, and `try_promote_with_precision`
— additive alongside the original `try_promote` (unchanged, all its
existing tests still pass unmodified) so the coverage-only gate remains
available where no judgment evidence exists at all. A `FixtureJudgmentOracle`
(deterministic, no real data, satisfying "no test may require a real
model API key" applied to judgment data too) backs new `commerce-core`
regression tests reproducing R1-E06's finding directly: one test proves
`try_promote` still accepts a nonsensical mapping (the safety gap,
preserved and now pinned by a test rather than only an experiment
observation); a second proves `try_promote_with_precision` rejects the
same mapping; a third proves a genuine fix (Gate 5's known-good
adidas/blue mapping) is *not* blocked when the oracle has no data for it
at all. `crates/phase2-eval/src/bin/precision_gate_eval.rs` replays
R1-E06's exact real scenario (same unfiltered `min_enum_frequency=1`
lexicon, same naive provider shape, same real catalog/queries) through
both gates side by side, using a `RealJudgmentOracle` backed by real ESCI
judgments (reusing `round1_eval::classify::product_satisfies_and`,
exposed `pub` for this purpose, unmodified).

**A real loophole found and fixed during the real-data run, not
hypothesized in advance**: the first version of `Judgment`/the precision
check only flagged a query as failing when the resolution matched
*something* (`total_matches > 0`) but mostly irrelevant products. Running
`precision_gate_eval` against real data the first time showed this
version **still accepted** R1-E06's exact nonsensical mapping — not the
intended, confirming result. Root cause: `waterproof` compiles to a
`Constraint::Boolean` constraint, and `round1_eval::classify`'s real-data
execution engine (`product_satisfies_and`) only evaluates `Brand`/`color`
constraints on this dataset (R1-E01/E02's real limitation: no other
structural fields exist in the source data) — any other constraint kind
"fails closed" by design, meaning the naive mapping's filter matches
**zero** products for every judged query, not irrelevant ones. The
original check's `total_matches > 0` guard treated "matched nothing" as
"nothing to judge" and silently passed it — a real, structural loophole,
not an edge case: a resolution that turns a query which would otherwise
fall through to lexical retrieval into a hard *zero-result* query is
arguably worse than one that matches irrelevant products, and the first
gate design had no way to catch it. Fixed by redefining `Judgment` to
also carry `judged_relevant_total` (how many relevant products are known
to exist for a query at all, independent of the filter) and rejecting
whenever relevant products are known to exist but the filter matches
none of them, in addition to the original imprecision check. A new,
dedicated regression test
(`precision_gate_rejects_a_resolution_that_matches_nothing_despite_known_relevant_products`)
pins this exact scenario going forward. Recorded here, not smoothed over,
per `docs/experiments/ROUND1_LOG.md`'s "record failed experiments" rule
carried into this log — the *first* real-data run of a new safety
mechanism finding a real gap in that same mechanism is exactly the kind
of result this project's discipline exists to surface, not hide.

**Results** (same environment as prior entries; `commerce-core` test
suite; single real-data validation run):

```
commerce-core: 37 -> 40 tests (3 new: reproduces R1-E06 via try_promote,
  proves try_promote_with_precision rejects it, proves the zero-match
  loophole is now also caught), all green; every pre-existing test
  unchanged.

Real-data replay of R1-E06's exact scenario (naive "shirts" ->
waterproof=true, 22,458 real queries, unfiltered min_enum_frequency=1
lexicon matching R1-E06's original setup):

  try_promote (original, coverage-only):
    ACCEPTED -> version 2   [reproduces R1-E06 exactly]

  try_promote_with_precision (new, real ESCI judgments as the oracle):
    REJECTED at the precision gate
    newly_resolved queries: 42
    queries judged by real evidence: 42/42
    queries below min_precision (0.5): 42/42 (all of them)
```

**Interpretation — confirmed, decisively, after one real-data-caught
correction.** Every one of the 42 real queries the nonsensical mapping
newly "resolved" had real judgment evidence available, and every one of
them correctly failed the precision check — not a partial improvement,
a complete rejection of the exact scenario R1-E06 flagged as the single
most important finding of Round 1. The coverage-only `try_promote` gate,
run side-by-side on identical inputs, still accepts it — confirming the
new gate is doing real, additional work, not just duplicating the
existing check. Combined with the "no judgment evidence" test (the
genuine adidas/blue fix promotes cleanly even when the oracle has zero
data for either newly-resolved query), this closes R1-E06's structural
safety gap as specifically diagnosed: the mechanism can now tell a real
fix from a nonsensical one, provided real judgment evidence is available
to check against — which is exactly the caveat the gate's own design
makes explicit (`PrecisionCheck::queries_judged` reports how much of the
newly-resolved set actually got checked, so a caller can see when the
gate is flying blind rather than assume silence means success).

**What this does not claim**: the gate's protection is only as good as
the judgment evidence available to it — a term whose newly-resolved
queries have *no* real judgment data at all still promotes on coverage
alone (by design, to avoid penalizing ignorance as if it were evidence of
a problem), so this is not a claim that every future nonsensical mapping
will be caught, only that the specific, real, previously-demonstrated
gap is closed for the queries evidence exists for. A production system
would need a real, continuously-updated judgment source (e.g. click/
purchase signals, not a fixed academic benchmark) for this gate to keep
protecting newly-introduced vocabulary over time — not built or claimed
here.

**Caveats**: single real-data validation run (deterministic pipeline).
The `min_precision=0.5` threshold used throughout was not tuned against
this specific scenario — chosen as a reasonable illustrative default
before running the validation, not fit to produce the result. The
`RealJudgmentOracle`'s execution engine (`product_satisfies_and`) is the
same real-data-scoped one `classify.rs` already uses (Brand/color only,
by construction on this dataset) — a production oracle would need a real
execution engine covering whatever constraint kinds a real provider might
actually propose, not just the two this specific dataset supports.

**Regression check**: `commerce-core` test suite: 40/40 tests green
(`cargo test --workspace --all-features`). `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo build --workspace --release` all clean.

**Next question**: two of Issue #6's evidence-driven priorities
(canonicalization, precision-aware promotion) are now validated against
real data. The remaining priorities — R1-E03's compiler fixes
(disjunction/negation), and the structural-plus-delegated-Tantivy
integration design — are the next highest-value items; both are
independent subsystems from what P2-E02/P2-E03 touched.

---

## P2-E04 — Fixing negation inversion: R1-E03's most severe correctness bug

**Evidence class**: hand-authored (the 10 R1-E03 adversarial queries,
re-run verbatim against the fix, plus 2 new deterministic
`commerce-core` regression tests) and real (the full 22,458-query
corpus, to check for real-data regressions). **Independence**: the
adversarial queries are the brief's own verbatim examples, unrelated to
this fix's implementation; the real-data check reuses the same real ESCI
corpus every other real-data entry in this project has used.

**Question**: R1-E03 found the compiler doesn't fail safely on negation —
"Nike shoes not red" compiled to a **required** `color=Red` constraint,
the exact opposite of stated intent, and R1-E03 named this "the single
most severe correctness defect" found in the whole round, because it
produces a confidently wrong answer, not merely an incomplete one. Issue
#6 priority 4 calls for exactly this fix, gated by R1-E03's own
10-query adversarial suite as the regression test. Does a minimal,
well-scoped negation-marker fix eliminate the bug without a) requiring a
full negated-constraint representation in the domain model (a much larger
change, deliberately deferred, matching R1-E03's own "don't rush a
second, differently-shaped bug into the same session" reasoning) or b)
regressing anything on real data?

**Hypothesis**: detecting a small set of negation markers ("not", "no",
"without", "non", and the un-split contractions "aren't"/"isn't"/
"don't"/"doesn't") immediately before a phrase that would otherwise
resolve, and routing that phrase to `residual_lexical` instead of
emitting a constraint or preference for it (whether the phrase was
single-candidate or ambiguous), eliminates the specific inversion bug
while keeping every other R1-E03 finding (disjunction, scope-
sufficiency, numeric words, units, ranges) exactly as recorded —
deliberately not attempting those in the same change, since each is an
independent, larger design question R1-E03 already flagged as needing
its own dedicated pass.

**Decision threshold**: binary on the adversarial suite — "Nike shoes not
red" (or an equivalent negated query) must no longer emit a positive
constraint for the negated term, and must not turn `residual_lexical`
into a silent drop (the term must remain visible, just unresolved). On
real data: no material regression in Semantic FIB rate, ambiguity rate,
precision, or filter recall relative to P2-E02's threshold=1 baseline
(R1-E06's stopword-fixed unfiltered lexicon) — a "fixed a severe
correctness bug" result that quietly made real coverage or precision
worse would need its own follow-up investigation before being called a
clean win.

**Implementation**  
`crates/commerce-core/src/ir/query.rs`: a new `NEGATION_WORDS` const and
a negation-handling block in `compile`'s token loop, inserted before the
general phrase-matching block (same position as the existing `size`/
`under`/`over` special cases). On a negation marker: look ahead for the
longest phrase match starting immediately after it (same greedy-longest-
window rule the rest of the compiler already uses); if found, push it to
`residual_lexical` instead of calling `apply_candidates` (so it can never
become a constraint or preference, ambiguous or not); if not found, the
marker is simply consumed like a stopword and the following token(s) are
evaluated normally on the next loop iteration. Two new `commerce-core`
regression tests (`crates/commerce-core/tests/ir_compiler.rs`) pin the
fix directly against `fixtures::shoe_lexicon`: one proves a
single-candidate negated phrase ("not red") no longer becomes a
constraint; a second proves an *ambiguous* negated phrase ("aren't
leather", Phase 0's own planted `leather` collision) is suppressed the
same way, not merely handled for the simple case. All 40 pre-existing
tests pass unmodified (grep confirmed no existing Phase 0 test asserts
behavior for any of the negation-marker strings this change touches).

**Results**

```
R1-E03's 10 adversarial queries, re-run verbatim against the fix
(crates/round1-eval/src/bin/adversarial_ir.rs, unmodified):

  "Nike shoes not red"
    before (R1-E03): constraints=[Brand(Nike), color=Red]  <- WRONG (required red)
    after  (this fix): constraints=[Brand(Nike)]  residual=["shoes","red"]  <- FIXED

  "dress shoes that aren't leather"
    before (R1-E03): ambiguous=["leather"]  (safe only by coincidence, per R1-E03's own note)
    after  (this fix): ambiguous=[]  residual=[...,"leather"]  <- now safe BY DESIGN

  (the other 8 queries, including "black or navy running shoes" [disjunction,
  still unfixed by design] and "black size 9" [scope-sufficiency, still
  unfixed by design], are unchanged from R1-E03's recorded output)

commerce-core: 40 -> 42 tests, all green (2 new, all else unchanged).

Real-data check (22,458 real queries, threshold=1 unfiltered lexicon,
same setup as P2-E02's threshold=1 row):

                       FIB rate  ambig rate  punt rate  precision  recall_ES  recall_Exact
  P2-E02 (pre-fix)       55.4%      38.4%       2.5%      94.5%       4.3%        5.0%
  post-negation-fix      55.2%      38.0%       2.5%      94.4%       4.4%        5.1%

  (same small, consistent direction at every canonicalization threshold
  tested, e.g. threshold=25 recall_Exact: 11.2% -> 11.5%)
```

**Interpretation — confirmed, and a clean win on both axes.** The
adversarial trace shows the fix does exactly what it was built to do:
"Nike shoes not red" no longer asserts a required red constraint (the
single most severe bug found in Round 1), and the "aren't leather" case
is now safe for the *right* reason (detected negation) rather than by
accident (a planted ambiguity collision that happened to also be
negated). The real-data check answers the "did fixing a correctness bug
quietly cost something" question directly: it did not — FIB rate and
ambiguity both move down by roughly 0.2-0.4 points (consistent with a
small number of real queries that used to confidently-but-wrongly
resolve now correctly falling through to residual/ambiguous-free
resolution instead), while precision holds essentially flat and recall
improves slightly at every threshold tested. This is the expected shape
for a correctness fix that removes wrong positive resolutions: a little
less illusory "coverage," a little more real recall — the same direction
(if a smaller magnitude) as P2-E02's canonicalization fix, and for the
same underlying reason: removing a source of incorrect confident
resolution.

**What this does not fix**: R1-E03's other two findings remain exactly
as recorded — disjunction ("black or navy") still silently narrows to
one option, and the scope-sufficiency gap ("black size 9" resolving with
full confidence despite no product-type signal) is untouched. Both
require larger, independent design work (disjunction needs an
alternation representation the Commerce IR doesn't have yet; scope-
sufficiency needs a "does this resolution have enough signal to trust"
check that's a different kind of gate than anything built so far) —
deliberately not attempted in this entry, consistent with R1-E03's own
reasoning against rushing multiple structural changes into one pass.

**Caveats**: the negation-marker list (8 entries) is a minimal,
literature-free heuristic covering exactly the forms seen in R1-E03's
adversarial examples and R1-E02's real diagnostic output ("without" from
"#4 pads without wings") — not validated against a broader negation-
detection benchmark, and multi-word negation phrases ("is not", "does
not" as two separate tokens rather than a contraction) are not handled
(only single-token markers are). Single deterministic run for both the
adversarial trace and the real-data check (no variance to characterize).

**Regression check**: `commerce-core` test suite: 42/42 green.
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo build --workspace --release` all
clean.

**Next question**: Issue #6's remaining priorities are the structural-
plus-delegated-Tantivy integration design (priority 5) and the memory-
representation follow-up (priority 6) — both larger, architecture-level
tasks appropriate for the epic's continued execution rather than a
single further checkpoint in this session.

---

## P2-E05 — Structural-plus-delegated-Tantivy integration (Issue #6 priority 5): three real bugs found, one dominant real root cause

**Evidence class**: real (same 1,215,854-product catalog and full
22,458-query real ESCI judgment set as every prior real-data entry) for
the final results; hand-authored/deterministic for the `commerce_core`
unit/regression tests. **Independence**: yes — same real, third-party
held-out judgments as every other real-data measurement in this project.

**Question**: P2-E01 validated that a delegated Tantivy index recovers
Solr's real relevance *standalone*. It explicitly did not validate how
that delegate composes with `commerce_core`'s own structural/facet index
at query time — Issue #6 priority 5, and the `FastPath`/`Hybrid`/`Punt`
execution-outcome contract Issue #5's own Branch A/C priority list named.
Does a real implementation of that contract, run end-to-end against the
full real catalog/query set, actually work — and does it preserve the
relevance P2-E01 already proved reachable?

**Hypothesis**: a query that resolves entirely to structural constraints
needs no delegate call at all (`FastPath`); a query with a genuinely
selective structural constraint should narrow via the index before
delegating (`Hybrid`), recovering close to P2-E01's standalone relevance
on the residual free text; a query with no constraint, or a non-selective
one, should skip structural narrowing entirely and delegate over the
whole corpus (`Punt`), avoiding R1-E05's non-selective-bitmap collapse.
`commerce_core` should own correctness throughout, never trusting a
delegate's own filtering.

**Decision threshold**: qualitative, matching P2-E01's own framing — does
the integrated system's real relevance approach P2-E01's standalone
Tantivy numbers (NDCG@10=0.3033, Recall@10=0.1801) closely enough to call
the composition sound, or does composing structural-first execution with
a delegate cost real relevance versus using the delegate alone?

**Implementation**

`commerce_core::plan` (new module): `LexicalDelegate` trait (`commerce_core`
defines the mechanism only — no lexical-engine dependency enters
`commerce_core`, mirroring `control_plane::provider::ModelProvider`/
`control_plane::precision::PrecisionOracle` exactly), `ExecutionOutcome`,
`PlannedQuery`, `PlannedHit`, `plan()` (pure routing decision) and
`execute_planned()` (routes and executes). `CommerceQuery::matches_variant`
extracted from `execute()` (zero behavior change — `execute()` now calls
it) so `plan` and `execute()` share exactly one definition of "matches."
`CatalogIndex::candidate_product_ids` added so a delegate can be
restricted by `ProductId` without knowing about internal bitmap ordinals.
Regardless of outcome, every delegate hit is re-verified against
`query.matches_variant` before being returned — a delegate is trusted for
ranking/recall, never correctness. 9 new `commerce-core` tests
(`tests/plan.rs`) using a deliberately misbehaving mock delegate (returns
out-of-restriction / constraint-violating hits) to prove `execute_planned`
re-checks everything itself.

Real-data validation harness: `crates/phase2-eval/src/bin/planner_integration_eval.rs`,
wiring a `TantivyDelegate` (wrapping the exact same Tantivy index/schema
P2-E01 validated) into `commerce_core::plan` against the full real
catalog and query set.

**Three real defects found via real-data validation, each recorded rather
than smoothed over (this project's established discipline, set by
P2-E03's "loophole found and fixed during the real-data run" precedent):**

1. **A performance bug that looked like a hang.** The first
   `verify_and_truncate` looked up each delegate hit's product via
   `catalog.products.iter().find(...)` — a linear scan, fine against
   unit-test fixtures with 11 products, but the full 1.2M-product real run
   did not complete in the few seconds every other real-data experiment in
   this project has taken; it was still running after 590s with zero
   output. Every delegate hit (up to `k * delegate_oversample` per query,
   across thousands of queries) was paying up to O(catalog size) just to
   be verified. Fixed via `CatalogIndex::lookup_product` (an existing O(1)
   hash-map lookup already used elsewhere in `commerce_core`).

2. **A delegate design that looked reasonable but collapsed relevance.**
   The first `TantivyDelegate` did not push `Hybrid`'s structural
   restriction into the Tantivy query itself — it asked for a global,
   oversampled top-N ranked by free-text relevance, then let
   `verify_and_truncate` post-filter to the restricted set. A 500-query
   real-data smoke run showed this fails badly: 8/8 `Hybrid` queries in
   that sample returned fewer than `k` verified hits, and integrated
   relevance came out at NDCG@10=0.0536 — far below P2-E01's standalone
   0.3033. Mechanism: a genuinely *selective* structural candidate set
   (the exact condition that routes a query to `Hybrid` in the first
   place) essentially never overlaps a generic free-text query's global
   top few thousand results by chance — oversampling further would not
   have fixed this in general. Fixed with real query-time push-down:
   `BooleanQuery` combining the free-text query with a `TermSetQuery` over
   the restricted `ProductId` set's ASINs, both `Occur::Must`, so Tantivy
   only ever scores documents that are actually structurally eligible.

3. **The real, dominant root cause, found while diagnosing (2), confirmed
   independently.** Even after fix (2), the *full* 22,458-query real run
   still showed severely degraded integrated relevance (NDCG@10=0.1080),
   **identical across every `selectivity_threshold` swept (0.01, 0.05,
   0.20)** — a strong signal the routing threshold wasn't the actual
   variable in play. Investigation traced this to `commerce_core::cold_start::compile_lexicon`:
   its brand loop was never subject to P2-E02's `min_enum_frequency`
   canonicalization, on the documented assumption that brand came from
   "an already-curated registry." Independently verified false for this
   real catalog: `round1_eval::catalog::build_catalog` interns brand from
   a raw per-product field exactly like it does `color` — no validation.
   Direct measurement: **206,227 distinct real "brand" strings, 49.4%
   occurring on exactly one product**, the large majority of those
   one-off values being seller-junk text ("funny musician gifts co", "this
   is a sharp not a hashtag tee for musicians") rather than genuine brand
   names — R1-E02/E02b's exact original failure mode (noisy, unvalidated
   per-product field values trusted as controlled vocabulary), undetected
   until now for this specific vocabulary, and explaining both the
   threshold-invariance (most `Hybrid`-routed candidate sets were
   singleton products, insensitive to any reasonable selectivity
   threshold) and the collapsed relevance (a query happening to match a
   junk "brand" string hard-filters to one wrong product). Fixed:
   `CatalogProfile` now tracks `brand_occurrence` the same way it tracks
   `enum_occurrence`; `compile_lexicon` gates brand entries on the same
   `min_enum_frequency` parameter already used for enum values.

**A fourth finding, from a deliberate RED-evidence check, not a bug but a
documented design decision.** Attempting to retroactively demonstrate RED
for `verify_and_truncate_drops_a_delegate_hit_outside_restrict_to` (by
temporarily removing the `restrict_to` membership check) stayed GREEN:
that test's query has a Brand constraint that already excludes the same
product for an unrelated reason, so the test did not isolate `restrict_to`
from `matches_variant`'s own constraint check. Traced to a real, provable
fact about the current code: `execute_planned`'s only caller derives
`restrict_to` from `query.constraints` itself, and `matches_variant`
already checks that same constraint set completely — in *that one call
pattern*, the `restrict_to` check can never independently change the
outcome. The correct fix was not to delete the "redundant" check: kept
deliberately as the extension point for a future restriction *not*
derivable from `query.constraints` (a merchandising/curated-collection
policy, Issue #5 section 12's merchandising-policy category). Made
`verify_and_truncate` `pub(crate)` and added a genuine white-box unit
test (`plan::tests::restrict_to_independently_excludes_a_constraint_satisfying_hit`)
that isolates `restrict_to`'s independent effect directly, with a
constraint-free query where only `restrict_to` can be responsible for an
exclusion — proving the mechanism does real, independent work when
actually exercised on its own terms, something the existing integration
test could not demonstrate given today's one call pattern.

**A fifth, architecture-only change from self-review against Issue #6's
own review questions** ("are we accidentally introducing a feature flag
where a semantic type or policy should exist?"): `selectivity_threshold`
was a bare `f64` threaded through `plan()`/`execute_planned()`, and the
oversample factor was a private const — neither had a typed home a future
per-vertical/per-merchant override could extend without changing a
function signature. Replaced both with `PlannerPolicy{selectivity_threshold,
delegate_oversample}`, a named policy type with deliberately no `Default`
(neither field had an evidence-backed recommended value at the time —
asserting one would have asserted a conclusion ahead of the evidence that
justifies it). Zero behavior change — verified by reproducing the prior
bare-parameter values exactly in every caller.

**Results** (same environment as every prior real-data entry: 4 vCPU
Intel Xeon @2.80GHz, 15 GiB RAM, Linux 6.18.5; full real 22,458-query set,
`selectivity_threshold` fixed at 0.05 — swept separately at {0.01, 0.05,
0.20} against the *unfixed* brand lexicon and found not to matter, see
finding 3 above — sweeping `min_enum_frequency`, now that it gates brand
too, instead):

```
 min_enum_freq  outcome(Hybrid/Punt/FastPath)  Hybrid<k hits   zero-result  NDCG@10  Recall@10   MRR    p50 latency
            1     12004 / 2659 / 7795            11712/12004     73.94%      0.0456    0.0273   0.0748   0.09ms
            5     10846 / 9908 / 1704              9518/10846     36.06%      0.1365    0.0814   0.2181   1.37ms
           25      5589 /16541 /  328              3277/5589       9.55%      0.2278    0.1354   0.3663   2.62ms
          100      2847 /19507 /  104               916/2847       2.93%      0.2703    0.1611   0.4324   2.84ms

(P2-E01 Tantivy-alone, full 22,458-query set: zero-result=0.6%, NDCG@10=0.3033, Recall@10=0.1801, MRR=0.4838, p50=1.09ms)
(R1-E04 Solr baseline, 1,000-query sample: zero-result=0.2%, NDCG@10=0.3052, Recall@10=0.1811, MRR=0.4910, p50=1486us)
```

`commerce-core`: 42 → 52 tests (10 new: 9 in `tests/plan.rs`, 1 white-box
unit test in `plan::tests`), all green; every pre-existing test unchanged.

**Interpretation**

**Confirmed, with a clear, strong, monotonic trend, not yet at full
parity.** As `min_enum_frequency` rises (less noisy vocabulary trusted),
integrated relevance climbs steadily toward P2-E01's standalone ceiling:
NDCG@10 reaches 89.1% of Tantivy-alone's (0.2703 / 0.3033) and Recall@10
reaches 89.5% (0.1611 / 0.1801) at threshold=100, up from 15.0%/15.2% at
threshold=1. The fraction of `Hybrid` queries returning fewer than `k`
verified hits — the symptom finding (3) explains — falls in lockstep
(97.6% → 87.8% → 58.6% → 32.2% across the same sweep), cross-validating
the diagnosis: as junk brand vocabulary is filtered out, the *genuine*
structural candidate sets remaining are large enough to actually supply
`k` relevant hits more often. This is real, monotonic, multi-point
evidence — not a single before/after number — that the composition
architecture itself (`FastPath`/`Hybrid`/`Punt`, delegate push-down,
`commerce_core`-owned verification) is sound, and that the remaining gap
to full parity is attributable to a *further-improvable* canonicalization
signal (occurrence-frequency alone), not a structural flaw in the
integration design.

**A real, honest trade-off surfaced by the same sweep, not hidden:**
`FastPath` — the zero-delegate-call outcome, the entire point of Gate 3's
specialization — shrinks sharply as the threshold rises (7795 → 1704 →
328 → 104 out of 22,458): a stricter canonicalization threshold means
fewer full queries resolve entirely to trusted structural vocabulary.
Higher `min_enum_frequency` buys integrated relevance at the cost of
`FastPath` coverage. Neither this entry nor P2-E02 selects a single
"correct" threshold — that remains a downstream deployment decision, now
informed by both this trade-off and P2-E02's own recall-vs-threshold
curve for the cold-start filter path specifically.

**What this does not claim**: full parity with P2-E01's standalone
Tantivy numbers was not reached at any threshold tested (up to 100);
whether a higher threshold, a stronger canonicalization signal (P2-E02's
own noted future work — a content heuristic beyond raw frequency), or
some other mechanism closes the remaining ~11% gap is not established
here. Latency also does not improve monotonically with the threshold in
the way relevance does (p50 rises from 0.09ms to 2.84ms as more queries
route through `Hybrid`'s `TermSetQuery` construction rather than
`FastPath`'s free index lookup) — a real, secondary cost of the same
trade-off, not previously measured.

**Caveats**: single run per threshold (deterministic pipeline given a
fixed lexicon and index, matching this project's precedent for
deterministic pipelines — no variance to characterize). The
`selectivity_threshold={0.01,0.05,0.20}` sweep against the *unfixed*
brand lexicon is real evidence that threshold didn't matter under that
specific confound, not evidence it never matters — re-sweeping selectivity
threshold against the now-fixed brand lexicon is unattempted here (this
entry fixed `selectivity_threshold=0.05` throughout the `min_enum_frequency`
sweep instead, since that was the newly-identified variable). The
`TantivyDelegate`'s push-down uses a `TermSetQuery` sized to the
restricted candidate set — at large candidate-set sizes (a less selective
`min_enum_frequency`/threshold combination than tested here) this
construction cost is unmeasured and could plausibly dominate at some
scale not reached in this sweep.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo test
--workspace --all-features` (52/52 `commerce-core` tests green), `cargo
build --workspace --release` all clean throughout.

**Next question**: Issue #6 priority 6 (memory representation follow-up)
is now unblocked and is the next entry.

---

## P2-E06 — Memory representation follow-up (Issue #6 priority 6): the bitmap index was never the dominant cost, and Tantivy's own added cost is modest

**Evidence class**: real (same full real 1,215,854-product catalog and
22,458-query set). **Independence**: n/a for this entry (a resource
measurement, not a relevance/precision claim scored against held-out
judgments).

**Question**: R1-E04 found Solr's on-disk index 7.3x larger than
`commerce-core`'s approximate index size, yet Solr's live RSS grew by
only 175MB versus `commerce-core`'s 3.76GB for the same real catalog — "a
real, measured memory-architecture disadvantage" (ADR 0008). Issue #6
priority 6 asks whether *delegating* lexical storage to Tantivy's
segment/mmap model closes that gap. `commerce_core`'s own structural
index is not being replaced by this project's architecture (ADR 0008:
delegate lexical/ranking, keep structural) — so the real, useful question
is not "does the integrated system's RSS match Solr's," it is "does
*adding* a delegated Tantivy index on top of the structural index that
already exists cost roughly what Tantivy's mmap-based design promises (a
small, mostly-lazily-paged-in increment), or does it stack another
multi-GB cost on top of the one R1-E04 already found?"

**Hypothesis**: Tantivy's own incremental RSS contribution — building the
index, opening a reader, and running the full real query workload once to
touch mmap pages under real access patterns — will be small relative to
`commerce_core`'s own already-resident structural index, because mmap'd
segments are lazily paged in by the OS rather than eagerly loaded, unlike
`commerce_core`'s in-memory `RoaringBitmap`/`HashMap`-based structures.

**Decision threshold**: qualitative — "small relative to the structural
index's own footprint" vs. "comparable to or larger than it" (no specific
percentage fixed in advance; the real question, matching R1-E04's own
framing, is architectural direction, not a precise target).

**Implementation**: `crates/phase2-eval/src/bin/memory_representation_eval.rs`,
reusing R1-E01's exact RSS measurement method (`round1_eval::bin::profile_catalog`'s
`/proc/self/status` `VmRSS:` read) so numbers are comparably derived, not
just similarly named. Measures RSS at 6 checkpoints in one process: process
start (baseline), after loading raw `catalog.jsonl` into `RealProduct`
structs, after `commerce_core::domain::Catalog` is built, after
`CatalogIndex::build`, after building the Tantivy index (same schema as
P2-E01/P2-E05) and dropping the writer, after opening a Tantivy
reader/searcher (before any query), and after running all 22,458 distinct
real queries once (to touch mmap pages under real access patterns rather
than measuring a cold, never-queried index).

**Results** (same environment as every prior real-data entry; single run —
RSS is not expected to vary meaningfully run-to-run for a deterministic
build/query sequence):

```
                                                    cumulative RSS    incremental
  process baseline                                      3.29 MB              --
  + raw catalog.jsonl loaded (RealProduct structs)   1,629.64 MB      +1,626.35 MB
  + Catalog struct built                             4,555.81 MB      +2,926.17 MB
  + CatalogIndex::build (structural index resident)  5,383.41 MB        +827.60 MB   (approximate_size_bytes: 259.22 MB)
  + Tantivy index built (writer dropped)              6,035.10 MB        +651.68 MB
  + Tantivy reader/searcher opened                    6,036.35 MB          +1.25 MB
  + full real 22,458-query sweep run once             6,589.88 MB        +553.54 MB

  Tantivy's OWN total incremental cost (build + reader + full real query sweep): ~1,206 MB
  commerce_core's OWN total cost (raw load + Catalog struct + CatalogIndex): ~5,380 MB
    of which CatalogIndex (the bitmap/range structure itself) is only:        ~828 MB
```

**Interpretation**

**Confirmed, and with an important, more precise correction to R1-E04's
original framing than a simple "yes/no."** Tantivy's own incremental cost
(~1.2GB, from an empty index through a fully real-query-warmed one) is
indeed meaningfully smaller than `commerce_core`'s own pipeline (~5.4GB)
— the direction R1-E04/ADR 0008 anticipated. But decomposing
`commerce_core`'s own ~5.4GB shows the *bitmap/range index itself* (Gate
3's actual specialized structure, `CatalogIndex::build`'s own
contribution) is only **~828MB** — a real, non-trivial, but not
dominant, share of the total. The dominant cost, by a wide margin, is
simply **holding a typed Rust representation of 1.2M real, attribute-heavy
products at all**: +1,626MB for the raw parsed `RealProduct` structs, then
a further +2,926MB (nearly 2x the raw JSONL text size) to build
`commerce_core::domain::Catalog` from them — before any index, structural
or lexical, enters the picture. R1-E04's original framing ("commerce-core's
RSS grew 3.76GB... a real, measured memory-architecture disadvantage")
correctly identified a real problem but, this entry's decomposition shows,
attributed it to the wrong layer: the disadvantage is not primarily in the
*index representation* (`RoaringBitmap`s, hash maps over enum values) —
it is in the underlying typed domain-object representation itself
(`String`-heavy `AttributeMap`s, per-product/variant heap allocations,
`BTreeMap`/`HashMap` overhead), which exists *before* `CatalogIndex::build`
is ever called. Delegating *lexical retrieval* to Tantivy therefore cannot,
by construction, close the dominant share of R1-E04's original gap — that
share was never lexical-index-shaped to begin with.

**A genuine, honest limitation of this specific measurement**, not
glossed over: this binary retains the raw `products: Vec<RealProduct>`
for the entire process lifetime (needed again later to build the Tantivy
index), so the reported "+1,626MB raw load" and "+2,926MB Catalog build"
figures both stay resident simultaneously rather than the raw Vec being
freed once the typed `Catalog` is built from it. A real ingestion
pipeline could plausibly stream the raw JSONL once, feed both the
`Catalog`-builder and the Tantivy indexer from that one pass, and drop the
raw `RealProduct` Vec afterward — which would not change
`CatalogIndex`'s own ~828MB figure or Tantivy's own ~1.2GB figure (both
measured as clean deltas *after* the raw Vec was already resident either
way), but would very plausibly reduce the *absolute* total RSS reported
here by close to the raw Vec's own ~1.6GB. This entry's relative findings
(index-alone vs. domain-model-alone vs. Tantivy-alone) hold regardless;
its absolute cumulative numbers are pipeline-shaped, not an inherent
floor.

**What this does not claim**: this does not re-measure Solr's own 175MB
figure (R1-E04's own number, unchanged, real, and not attempted again
here) — it decomposes `commerce_core`'s side of that comparison, which
R1-E04 could not do at the time (Tantivy integration did not exist yet).
It also does not claim the ~2,926MB `Catalog`-struct-build cost is
irreducible — string interning, dense IDs, or a columnar representation
(CLAUDE.md's own "likely physical primitives" list) remain unbenchmarked
alternatives, exactly as R1-E04/ADR 0008 originally flagged; this entry
narrows *where* such an optimization would need to target (the domain
model, not primarily the bitmap index) rather than performing it.

**Caveats**: single run (deterministic build/query sequence, no variance
to characterize, matching this project's precedent for deterministic
pipelines). RSS is a coarse, OS-level, whole-process measurement — it
does not attribute cost to individual fields/structures the way a heap
profiler would; the "~2,926MB for `Catalog` struct build" figure is a
single aggregate delta, not a breakdown of which part of `Product`/`Variant`/
`AttributeMap` contributes most. No comparison to Solr's own live-query
RSS growth under a real 22,458-query sweep is made here (R1-E04 measured
Solr's RSS growth from indexing, not from a full real query workload) —
labeled as a real, structural asymmetry in what's being compared, not
elided.

**Regression check**: n/a (new, standalone measurement binary; no
`commerce_core`/`round1-eval` production code touched by this entry).
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo build --workspace --release` all
clean.

**Next question**: Issue #6's two remaining named priorities (5 and 6)
are both now complete with real-data evidence. The commerce-core-archaeology
workstream (Issue #5 section 12) remains blocked on external repository
access (tracked separately, being pursued in a parallel session per
explicit user instruction) and is the only named item in either epic's
"Definition of done" not yet satisfied. Absent new archaeology findings
or a new architectural requirement, the next genuine boundary is the one
this entry itself surfaced: whether a denser domain-model representation
(string interning / dense IDs / columnar attributes) measurably reduces
the ~2.9GB `Catalog`-struct-build cost this entry found dominates real
RSS — a new, evidence-driven hypothesis for a future entry, not yet
attempted.

---

## P2-E07 — Does a deterministic heuristic canonicalizer beat the shipping frequency-only brand gate, measured against real reconciled ground truth?

**Evidence class**: real. The 209-candidate corpus
(`dataset_cache/export/brand_adjudication_corpus.jsonl`,
`scripts/phase2/build_brand_adjudication_corpus.py`, deterministic,
seed=7) samples real excluded/near-frontier brand vocabulary from the
real 1,215,854-product ESCI catalog, per bucket (singleton/low/mid/
near_threshold + 9 calibration high-frequency brands).

**Independence**: ground truth (`dataset_cache/export/brand_adjudication_ground_truth.jsonl`,
`scripts/phase2/reconcile_brand_adjudication.py`) is reconciled from
**three independent labeling passes** (separate agent runs, no shared
context, per `docs/research/brand-adjudication-rubric.md`'s protocol):
3/3 agreement -> unanimous (135/209, 64.6%); 2/3 -> majority (71/209,
34.0%); 0/3 (three distinct labels) -> the ground truth label is itself
`ambiguous_insufficient_evidence` (3/209, 1.4%). Pairwise raw agreement
between passes: 73.7%, 83.7%, 70.3% — real, imperfect, human-adjudication-
grade agreement, not silently smoothed to 100%. The two deterministic
canonicalizers scored here are independent of how the ground truth was
produced (pure code, no relationship to the labeling passes).

**Question**: `docs/research/brand-adjudication-rubric.md`'s five-class
taxonomy defines "safe to trust as a structural hard-filter entry"
(`VocabularyClass::trusted_as_structural`). Does `HeuristicCanonicalizer`
(word-count/marketing-word-blocklist/shape/title-prefix-consistency
scoring, `commerce_core::cold_start::canonicalize`) classify candidates
into that binary decision more accurately than `FrequencyOnlyCanonicalizer`
(the literal shipping `min_enum_frequency` gate, wrapped unmodified)?

**Implementation**: `crates/phase2-eval/src/bin/brand_canonicalizer_eval.rs`
scores both canonicalizers' `trusted_as_structural()` output against the
reconciled ground truth's positive classes
(`canonical_known_entity_or_alias` + `legitimate_new_entity`, 156/209 =
74.6% of the corpus), sweeping the same threshold set P2-E05 used.

**Results** (precision/recall/F1 of the binary trusted-as-structural decision):

| threshold | FreqOnly prec/recall/F1 | Heuristic prec/recall/F1 |
|---|---|---|
| 1 | 74.6% / 100.0% / 85.5% | 75.5% / 96.8% / 84.8% |
| 3 | 85.4% / 75.0% / 79.9% | 87.8% / 92.3% / 90.0% |
| 10 | 91.4% / 47.4% / 62.4% | 91.8% / 86.5% / 89.1% |
| **25** (P2-E05's measured recall-peak frontier) | **85.7% / 15.4% / 26.1%** | **92.2% / 83.3% / 87.5%** |
| 50 | 100.0% / 5.8% / 10.9% | 93.5% / 82.7% / 87.8% |

At threshold=25, accuracy by ground-truth confidence tier: on the 135
unanimous-agreement candidates, `FrequencyOnlyCanonicalizer` is right
31.1% of the time vs. `HeuristicCanonicalizer`'s 88.1% — the heuristic's
advantage is not an artifact of contested/low-confidence cases; it wins
decisively on the candidates humans agreed on most.

**Interpretation**: at every threshold from 3 upward, the heuristic beats
frequency-only on *both* precision and recall for the classification task
— not a precision/recall tradeoff, an unambiguous win on this specific
metric. At threshold=25, frequency-only's recall collapses to 15.4%
(matching the "catastrophic FastPath coverage loss" this project already
knew the raw gate causes) while the heuristic holds 83.3% recall at
*higher* precision than frequency-only achieves at any threshold. See
P2-E08 below for whether this classification-level win survives contact
with real end-to-end retrieval measurement — it does not, in the way this
result alone would predict.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo test
--workspace --all-features`, `cargo build --workspace --release` all
clean. New crate dependency: `serde`/`serde_json` added to `phase2-eval`
(round1-eval's existing loaders don't re-export their own serde
dependency transitively).

---

## P2-E08 — Does the classification-level win (P2-E07) survive contact with real end-to-end FIB/precision/recall measurement?

**Evidence class**: real. Same 1,215,854-product real catalog and the
full real 22,458-query judged set (not a 1,000-query sample), reusing
`round1_eval::classify`'s existing FIB/precision/recall machinery
unmodified — identical measurement to P2-E02/P2-E05, so the numbers are
directly comparable.

**Independence**: yes — the real ESCI query/judgment set is unrelated to
and was not used to build either canonicalizer.

**Question**: CLAUDE.md: "Do not claim an architectural win from
microbenchmarks alone when end-to-end evidence is available." P2-E07
measured heuristic-vs-frequency-only accuracy against a 209-candidate
adjudication corpus — a proxy. Does swapping `HeuristicCanonicalizer` for
`FrequencyOnlyCanonicalizer` in the actual cold-start lexicon-compilation
path recover real Semantic FIB coverage without sacrificing the real
precision/recall this project has measured since R1-E02?

**Implementation**: `commerce_core::cold_start::compile_lexicon_with_brand_canonicalizer`
(new, `crates/commerce-core/src/cold_start/profile.rs`) — identical to
the shipping `compile_lexicon` for every field except brand vocabulary,
where inclusion is decided by a pluggable `VocabularyCanonicalizer`
instead of the raw `min_enum_frequency` count. Enum-value (color/size/
etc.) filtering is deliberately left on the same raw-threshold gate in
both arms in this experiment, because the adjudication ground truth only
covers brand vocabulary — isolating exactly the one variable P2-E07
measured. `crates/phase2-eval/src/bin/canonicalizer_fib_eval.rs` runs
both arms (plus a sanity arm confirming `FrequencyOnlyCanonicalizer`
routed through the new function reproduces `compile_lexicon`'s numbers
exactly, which it does at every threshold tested) against the real full
query set.

**Results** (real 1.2M-product catalog, real 22,458 queries):

| threshold | arm | FIB | ambig | punt | precision | recall_ES | recall_Exact |
|---|---|---|---|---|---|---|---|
| 3 | FrequencyOnly | 41.1% | 39.8% | 3.2% | 92.5% | 9.7% | 11.2% |
| 3 | Heuristic | 44.5% | 39.3% | 3.0% | 93.0% | 8.3% | 9.6% |
| 10 | FrequencyOnly | 28.5% | 34.1% | 3.8% | 91.7% | 21.1% | 24.3% |
| 10 | Heuristic | 41.4% | 33.3% | 3.3% | 92.4% | 12.2% | 14.2% |
| **25** | **FrequencyOnly** | 21.3% | 22.3% | 4.4% | 90.5% | **31.7%** | **35.6%** |
| **25** | **Heuristic** | **45.3%** | 21.5% | 3.4% | 91.7% | 13.8% | 15.8% |
| 50 | FrequencyOnly | 16.1% | 16.5% | 4.5% | 90.3% | **39.1%** | **43.4%** |
| 50 | Heuristic | 48.6% | 15.9% | 3.4% | 91.6% | 13.2% | 15.1% |

(P2-E02 baseline, threshold=1/unfiltered: FIB=55.4%, precision=94.5%,
recall_ES=4.3%, recall_Exact=5.0%.)

**Interpretation — a real, counter-intuitive negative finding, recorded
in full rather than smoothed over**: `HeuristicCanonicalizer` delivers
exactly what P2-E07 predicted on FIB coverage — roughly double the
FrequencyOnlyCanonicalizer's FIB rate at every threshold >=10, and it
climbs *with* the threshold instead of collapsing the way frequency-
only's does (frequency-only trades FIB for recall as threshold rises;
heuristic's FIB keeps rising too). Precision is essentially a wash
(heuristic slightly higher at every threshold). But **real recall against
actual Exact/Substitute-labeled relevant products is substantially
*worse* under the heuristic at every threshold tested, and gets
dramatically worse at higher thresholds** (at threshold=50: 43.4% ->
15.1%, a 65% relative recall loss) — the opposite of what P2-E07's
classification-level result would predict, since the heuristic classifies
individual brand strings *more* accurately than frequency-only against
real human ground truth at every threshold tested.

The mechanism, as best understood from the data (not directly
instrumented, stated as an inference): a brand value being an
individually *correct* structural entity (P2-E07's question) is not the
same question as whether compiling it into a *hard* structural filter
helps or hurts a specific real query's recall. `min_enum_frequency`'s
recall-rises-with-threshold behavior (already known since P2-E05) is
itself evidence that trusting *more* brand strings as hard filters
increases the rate at which a query gets routed through a wrong or
overly narrow structural constraint — a real product's actual brand
string not exactling matching the compiled value (aliasing, casing,
punctuation variants the canonicalizer doesn't merge) causes the hard
filter to wrongly exclude it. `HeuristicCanonicalizer` trusts
*more* low-frequency values than frequency-only does at any given
threshold (that is precisely its design goal and P2-E07's measured
win) — which means more queries get routed through additional hard
filters, reproducing and amplifying the same recall-suppressing dynamic,
even though each individual classification is more accurate in
isolation.

This is exactly the case CLAUDE.md's "do not claim a win from
microbenchmarks alone" rule anticipates: P2-E07 alone would have
supported "HEURISTICS ARE SUFFICIENT, ship it." P2-E08 shows the real
downstream metric this project has tracked since R1-E02 (recall against
real Exact-labeled relevant products) moves in the *opposite* direction.
Both results are true and both are reported; neither is discarded because
it's inconvenient.

**Regression check**: sanity arm (`FrequencyOnlyCanonicalizer` routed
through the new generic function) reproduces `compile_lexicon`'s exact
FIB/ambig/punt/precision/recall numbers at every threshold tested,
confirming the refactor (`compile_lexicon` now delegates its
non-brand-vocabulary logic to a shared `compile_non_brand_lexicon`
helper) preserved production behavior exactly. `cargo fmt --all --
--check`, `cargo clippy --workspace --all-targets --all-features -- -D
warnings`, `cargo test --workspace --all-features`, `cargo build
--workspace --release` all clean.

**Decision: REVISE, not ship.** Neither `FrequencyOnlyCanonicalizer` nor
`HeuristicCanonicalizer` as currently specified is an unconditional win:
frequency-only tops out around 43% real recall (at threshold=50, with
correspondingly low 16% FIB coverage) while heuristic tops out around
45% FIB coverage but caps near 16% real recall regardless of threshold.
The two mechanisms are optimizing genuinely different things (individual-
value classification accuracy vs. real query-level retrieval recall), and
neither this experiment nor P2-E05 has yet tested a design that targets
recall directly rather than as a downstream side effect of a threshold or
classification decision. A third arm (model-assisted canonicalization,
Issue #9's remaining named baseline) is running independently as of this
entry and will be appended once complete — but this result already
establishes that "does the canonicalizer classify brand strings
correctly" and "does trusting its output as a hard filter help real
retrieval" are two different questions this project must keep measuring
separately, not conflate.

**Next**: append the model-assisted arm's results once its independent
agent run completes; write Issue #9's final decision note against all
three arms; feed this finding into Issue #5/`ROUND1_DECISION_TREE.md` —
"a structural hard filter's value depends on how confidently its
compiled constraint generalizes across real spelling/aliasing variation
in the underlying field, not just whether the *canonical* value itself is
a real entity" is a new, generalizable finding beyond just brand
vocabulary.

---

## P2-E09 — Where does a third, model-assisted canonicalizer arm land, at the classification level?

**Evidence class**: real. Same 209-candidate corpus and reconciled
3-pass ground truth as P2-E07.

**Independence**: the model-assisted arm is a fourth, independently-run
agent pass over the same 209 candidates -- held out of the ground-truth
reconciliation itself (which uses only the three passes that produced
`brand_adjudication_ground_truth.jsonl`), so it is scored here as a
genuine system under test, not one of its own raters. **Not independent
in the stronger sense that threatens validity**, though, and this is
carried forward from the rubric's own disclosure, not glossed over: this
arm and every ground-truth-forming pass are produced by the same
underlying model family in this environment -- no distinct-vendor model
or human panel was available. Every number below is qualified by that.

**Question**: P2-E07 asked whether `HeuristicCanonicalizer` beats the
shipping frequency-only gate at the classification task. Does a
model-assisted arm (an independent offline agent pass, using only the
same bounded catalog evidence a deterministic heuristic gets) do better
still?

**Implementation**: a fourth agent pass (independent run, same rubric/
corpus, explicitly framed as the system-under-test rather than a
ground-truth labeler) produced
`dataset_cache/export/brand_adjudication_model_assisted.json`.
`crates/phase2-eval/src/bin/brand_canonicalizer_eval.rs` was extended
with a `ModelAssistedCanonicalizer` (a `VocabularyCanonicalizer`
implemented as a fixed per-candidate lookup, since this arm's real-world
form is a compiled offline artifact — the same shape as
`control_plane::provider::ModelProvider`'s propose/replay/promote
pattern — not a callable general-vocabulary mechanism) and scored
alongside the two deterministic arms.

**Results** (209 real candidates, 156/209 = 74.6% ground truth positive):

| arm | precision | recall | F1 |
|---|---|---|---|
| FrequencyOnlyCanonicalizer (best: threshold=1) | 74.6% | 100.0% | 85.5% |
| HeuristicCanonicalizer (best: threshold=3) | 87.8% | 92.3% | **90.0%** |
| **Model-assisted (fixed, no threshold)** | **84.6%** | **98.7%** | **91.1%** |

Exact 5-class agreement with ground truth (not just the binary
trusted-as-structural collapse): 151/209 = 72.2%.

Accuracy by ground-truth confidence tier (all three arms at their
threshold=25 configuration for the deterministic two, since that is
where the classification-vs-recall tension in P2-E08 was sharpest):

| tier | n | FrequencyOnly | Heuristic | Model-assisted |
|---|---|---|---|---|
| unanimous | 135 | 31.1% | 88.1% | **91.9%** |
| majority | 71 | 40.8% | 73.2% | **77.5%** |
| no_majority | 3 | 66.7% | 33.3% | 0.0% |

**Interpretation**: at the classification level, the model-assisted arm
is the best of the three -- highest F1, highest accuracy on both the
unanimous and majority confidence tiers (the `no_majority` tier is only 3
candidates, too small to read anything into a 0/3 vs 1/3 difference).
This is a real, measured result, not assumed.

**The critical caveat, stated as prominently as the win**: P2-E08 already
demonstrated, on this exact project, that a canonicalizer which wins
decisively on this exact classification-level metric
(`HeuristicCanonicalizer` beat `FrequencyOnlyCanonicalizer` on both
precision and recall here) can *still* produce substantially worse real
end-to-end retrieval recall once wired into `compile_lexicon` and
measured against the real 22,458-query judged set. This entry's model-
assisted result must not be read as "therefore model-assisted is the
answer" on the strength of this table alone -- CLAUDE.md's own
instruction ("do not claim an architectural win from microbenchmarks
alone when end-to-end evidence is available") applies with extra force
here, precisely because this project's own prior entry already showed
that instinct to be wrong once.

**A concrete, disclosed scope limitation, not an oversight**: unlike
`FrequencyOnlyCanonicalizer`/`HeuristicCanonicalizer` (both cheap,
deterministic code that runs over the full real ~206,227-distinct-brand
vocabulary in milliseconds), the model-assisted arm's real form is
per-value agent judgments -- classifying the full real vocabulary would
mean ~206,227 individual judgments, which CLAUDE.md's own cold-start
discipline ("do not perform one LLM call per SKU/value at scale") and
this environment's lack of a live, cheap model API both rule out. This
entry's 209-candidate result is therefore classification-level evidence
only. A follow-up entry (P2-E10) runs a real, if smaller-scale,
end-to-end test on a real sample of the vocabulary that could actually
change a measured query outcome.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo test
--workspace --all-features` all clean. No production code changed --
extends the existing `brand_canonicalizer_eval.rs` binary only.

**Decision**: classification-level evidence alone is INCONCLUSIVE for a
production decision, by this project's own established standard (P2-E08).
Do not resolve Issue #9 on this entry -- see P2-E10.

---

## P2-E10 — Does the model-assisted arm's classification win survive contact with real end-to-end measurement?

**Evidence class**: real. Same real catalog and full real 22,458-query
judged set as P2-E08. The 500-brand sample and its selection criterion
(`scripts/phase2/build_query_relevant_brand_sample.py`) are real,
deterministic (seed=7), drawn from the real population of brands that
could actually change a measured query outcome. Model-assisted judgments
for the 500 sampled brands are five independent agent-labeled batches
(100 each, `scripts/phase2/merge_model_assisted_batches.py` merges and
order-verifies them against the sample file).

**Independence**: the 500-brand sample and the real query/judgment set
are unrelated to how the model-assisted judgments were produced.

**The tractability + fairness problem this entry solves**: P2-E09 could
only evaluate the model-assisted arm's classification accuracy against a
209-candidate sample -- classifying the full real ~206,227-distinct-brand
vocabulary is not tractable (would mean ~206,227 individual agent
judgments, ruled out by CLAUDE.md's cold-start discipline and this
environment's lack of a live model API). This entry solves it for the
END-TO-END metric with a different technique: a brand string that is
below threshold AND never appears in any real judged query cannot change
any measured FIB/precision/recall number, so restricting model-assisted
judgment to a real, tractable sample (500 of the 7,532 real
below-threshold brands whose exact string appears in some real query)
targets exactly the population relevant to this measurement, not an
arbitrary subset. To keep the comparison fair against
`FrequencyOnlyCanonicalizer` (which decides every one of the real
~206,227 brands), `HybridModelAssistedCanonicalizer`
(`crates/phase2-eval/src/bin/hybrid_model_assisted_fib_eval.rs`) uses the
real model-assisted judgment for the 500 sampled brands and falls back to
the *exact* `FrequencyOnlyCanonicalizer` decision (same threshold) for
every other brand -- both arms decide every brand identically except the
500 where model-assisted's real judgment is substituted in, isolating
its causal effect.

**Results** (real 1.2M-product catalog, real 22,458 queries; 311/500 =
62.2% of the sample trusted as structural by the model-assisted arm):

| threshold | arm | FIB | precision | recall_ES | recall_Exact |
|---|---|---|---|---|---|
| 3 | FrequencyOnly (baseline) | 41.09% | 92.5% | 9.69% | 11.22% |
| 3 | Hybrid (500 real overrides) | 41.09% | 92.5% | 9.67% | 11.19% |
| 10 | FrequencyOnly (baseline) | 28.47% | 91.7% | 21.07% | 24.28% |
| 10 | Hybrid | 29.23% (+0.76pp) | 91.8% | 20.31% (**-0.76pp**) | 23.43% (**-0.85pp**) |
| **25** | **FrequencyOnly (baseline)** | 21.30% | 90.5% | 31.74% | 35.60% |
| **25** | **Hybrid** | 22.64% (**+1.34pp**) | 90.6% | 29.79% (**-1.95pp**) | 33.44% (**-2.16pp**) |
| 50 | FrequencyOnly (baseline) | 16.09% | 90.3% | 39.09% | 43.41% |
| 50 | Hybrid | 17.67% (**+1.58pp**) | 90.4% | 35.58% (**-3.51pp**) | 39.53% (**-3.88pp**) |

**Interpretation**: the model-assisted arm's classification-level win
(P2-E09) does **not** survive contact with real end-to-end measurement --
it reproduces P2-E08's exact directional finding for
`HeuristicCanonicalizer` (more brands trusted as hard filters -> higher
FIB, lower real recall), at a magnitude proportional to how much of the
real vocabulary this isolated test could actually touch (only 500 of
~199,582 real below-threshold brands, 0.25%). At threshold=25 -- P2-E05's
own measured real recall-peak frontier -- the 500 sampled brands' real
model-assisted judgments alone cost 2.16 percentage points of real
Exact-relevance recall for a 1.34-point FIB gain, with only 500/206,227
(0.24%) of the total brand vocabulary touched. This is small in absolute
terms because the touched population is small by necessity (a tractability
constraint, not a claim that the *full* vocabulary's effect would stay
this small), but the *direction and consistency* is the real finding: at
every threshold from 10 to 50, trusting more of the real
model-assisted-judged brands as hard filters costs more real recall than
it gains in FIB coverage, in the same direction P2-E08 already found for
`HeuristicCanonicalizer`'s full-vocabulary sweep, and in the same
direction P2-E05 originally found for the raw `min_enum_frequency`
threshold itself (lower threshold = more brands trusted = lower real
recall, the *original* R1-E02/E02b finding this project's whole
canonicalization workstream exists to fix). Three independent mechanisms
now -- a raw frequency threshold, a deterministic heuristic, and a
held-out model-assisted arm -- all show the same qualitative relationship
between "trust more brand strings as hard filters" and "real recall goes
down," regardless of how each mechanism decides which strings to trust or
how accurately it classifies them in isolation.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy -p
phase2-eval --all-targets --all-features -- -D warnings`, full workspace
`cargo test`/`cargo build --release` all clean. No `commerce_core` file
touched.

**Decision: CANONICALIZATION FRONTIER IS FUNDAMENTAL.** Of Issue #9's
four named decision options, this is now the best-supported by the full
weight of P2-E05/P2-E08/P2-E09/P2-E10 together, not "REVISE AND RETEST"
in the sense of "try a different classifier" -- three structurally
different mechanisms (raw threshold, deterministic heuristic, model-
assisted) all show the same directional tradeoff, which is evidence the
tension is inherent to *how trust is enforced* (a hard structural AND
filter on an exact-matched string, vulnerable to any real spelling/
aliasing/formatting variation between the compiled constraint and a
product's actual field value), not to *which values get trusted*. "MODEL-
ASSISTED IS MATERIAL" and "HEURISTICS ARE SUFFICIENT" are both rejected
by this same evidence -- neither wins on the metric that matters
(real recall), despite winning on classification accuracy. The concrete,
evidence-backed next experiment this points to is not a fourth
canonicalization strategy: it is testing whether a *softer* enforcement
mechanism at query-compile time -- alias-normalized/fuzzy brand matching,
or treating a compiled brand match as a scored `Preference` rather than a
hard `Constraint` when canonicalization confidence is not maximal -- can
break the trend this entry, P2-E08, and P2-E05 all independently found,
where refining *which* strings are trusted cannot.

**Next**: feed into Issue #5/`ROUND1_DECISION_TREE.md` alongside Issues
#7/#8's findings; open a follow-up issue for the soft-match/scored-
constraint experiment this decision implies, rather than silently
expanding Issue #9's own scope.

---

## P2-E11 — Issue #6 P1-B: does confidence-tiered enforcement (alias-normalized hard Constraint, fuzzy soft Preference) beat exact-match, and why (or why not)?

**Evidence class**: real (full 1,215,854-product catalog, full 22,458-query
judged corpus, every cell). **Independence**: yes (ESCI's own human
judgments; the enforcement mechanism under test has no influence on how a
judgment was assigned).

**Background**: P2-E10's decision -- CANONICALIZATION FRONTIER IS
FUNDAMENTAL -- named the concrete next experiment: not a fourth
canonicalization strategy, but a *softer enforcement mechanism* at
query-compile time. Issue #6's reorientation named this explicitly as the
next P1 priority and explicitly forbade another canonicalization arm.

**Hypothesis**: P2-E10's own stated root cause is "real spelling/aliasing/
formatting variation between the compiled constraint and a product's actual
field value." If that is the dominant driver, a confidence-tiered
enforcement mechanism -- deterministic alias-normalized hard matching for
already-trusted brand strings that share an identity after corporate-
suffix/punctuation stripping (tier 1), plus a fuzzy, edit-distance-bounded
soft `Preference` for otherwise-untrusted brand-shaped query terms (tier
2) -- should recover real recall lost to that variation, without
collapsing `FastPath`/`Hybrid` route coverage.

### Mechanism

`crates/commerce-core/src/cold_start/alias.rs` (new): `alias_key` (strip
punctuation, then repeatedly strip *trailing* corporate/legal-suffix
tokens -- "nike, inc." / "nike inc" / "nike" all key to "nike," conservative
by construction, never touches a non-trailing token) and `edit_distance`
(standard Levenshtein, char-based). `ir::StructuralConstraint::BrandAny(Vec<BrandId>)`
(new, generalizes `Brand(BrandId)` to alias-group membership) and
`ir::Preference::StructuralBoost(StructuralConstraint, f64)` (new,
ranking-only). `cold_start::profile::compile_lexicon_with_alias_enforcement`:
same `min_enum_frequency` trust gate `compile_lexicon` uses (isolates the
enforcement variable alone), groups trusted names by `alias_key` for tier
1, and fuzzy-matches a caller-bounded candidate pool (real query
vocabulary, not all ~206K raw catalog strings) against trusted groups for
tier 2.

### RED: a real relevance regression, found by the harness this experiment reused rather than assumed away

First full-corpus run (`crates/phase2-eval/src/bin/alias_enforcement_eval.rs`,
reusing P2-E05's exact planner-integration harness and P2-E07-E10's own
`measure_precision`): `alias_fuzzy` (tier 1 + tier 2) **regressed**
relevance at `min_enum_frequency=25` versus `baseline` (exact match) --
NDCG@10 0.2278 -> 0.2095, Recall@10 0.1354 -> 0.1248, zero-result rate
9.55% -> 10.09% -- and took 2.3x longer (126s -> 286s for the same 22,458
queries).

Root-caused, not shrugged off: `ir::query::apply_candidates` consumed a
phrase resolving to *only* a `Preference` exactly like a hard `Constraint`
would -- removing it from `residual_lexical`, and therefore from what a
lexical delegate ever searches on -- even though a `Preference` explicitly
enforces nothing. This code path had never been exercised before: I7-E04
found `compile_lexicon` never emitted a real `Preference` candidate at any
threshold, so this bug existed but was silently unreachable until tier 2
became the first real caller to produce one. A query whose *only* real
signal was a fuzzy tier-2 match lost that signal from lexical retrieval
entirely, in exchange for a ranking-only boost applied to whatever
(usually worse) candidate set resulted from searching without it -- a real,
production-relevant defect, caught by exactly the kind of end-to-end
measurement (not classifier-quality-in-isolation) this phase's own
instructions demanded.

**Fix**: `apply_candidates` now keeps a preference-resolved phrase in
`residual_lexical` too. This also closes a related latent bug, found while
fixing this one, not yet exploited by real data: `plan::plan` treats an
entirely empty compiled query (no constraints, empty `residual_lexical`)
as `FastPath`, which then ranks the *entire* catalog -- a preference-only
query with nothing else used to hit exactly that shape. Cascading fixture
updates in four existing test files
(`cold_start.rs`/`control_plane.rs`/`coverage.rs`/`ir_compiler.rs`), each
with a comment tracing the new expected number back to this fix.

### GREEN: fix validated on the full real corpus, both thresholds

| min_enum_frequency | mode | NDCG@10 | Recall@10 | MRR | zero-result | outcome dist. (FastPath/Hybrid/Punt) | wall time |
|---|---|---|---|---|---|---|---|
| 25 | baseline | 0.2278 | 0.1354 | 0.3666 | 9.55% | 328/5589/16541 | 136.0s |
| 25 | alias_only | 0.2278 | 0.1354 | 0.3666 | 9.55% | 328/5589/16541 | 133.5s |
| 25 | alias_fuzzy | 0.2279 | 0.1355 | 0.3668 | 9.42% | 325/5588/16545 | 135.8s |
| 100 | baseline | 0.2704 | 0.1611 | 0.4328 | 2.93% | 104/2847/19507 | 107.4s |
| 100 | alias_only | 0.2704 | 0.1611 | 0.4328 | 2.93% | 104/2847/19507 | 107.5s |
| 100 | alias_fuzzy | 0.2704 | 0.1611 | 0.4328 | 2.93% | 104/2845/19509 | 107.1s |

The regression is gone -- `alias_fuzzy` is now statistically indistinguishable
from (threshold 100) or marginally better than (threshold 25) baseline, at
comparable wall time (the length-difference edit-distance prefilter added
in the same cycle, `cold_start/profile.rs`, cut the fuzzy tier's own
lexicon-build cost from +2.3x back to parity, since Levenshtein distance is
always >= the length difference and most candidate/group pairs can never
qualify).

**But**: `alias_only` (tier 1) is **byte-for-byte identical** to `baseline`
at *both* thresholds -- zero measured effect, not a small one, reproduced
twice. `alias_fuzzy`'s own gain over baseline (+0.0001 NDCG@10 at
threshold 25, +0.0000 at threshold 100) is noise-level, not a real win.

### Follow-up: why is the effect negligible, when P2-E10's own stated root cause was spelling/aliasing/formatting variation?

`crates/phase2-eval/src/bin/brand_recall_gap_diagnostic.rs` (new): for every
`StructuralOnly`/`StructuralPlusLexical` query with a compiled `Brand`
constraint, every judged-**Exact** product that fails that constraint is
classified by whether the product's actual brand string is alias-identical
to the constraint's brand (tier 1's territory), fuzzy-close
(`edit_distance(alias_key) <= 2`, tier 2's territory), or neither.

**Real result, full corpus**:

| bucket | rows (one per failing judged-Exact product) | distinct queries |
|---|---|---|
| alias-identical | 31 | (tier 1 measured zero effect anyway -- consistent) |
| fuzzy-close (<=2) | 191 | 114 combined with alias-identical |
| neither | 20,201 | 2,398 |

Alias/spelling variance explains **~1.1% of rows, ~4.5% of distinct
queries** -- P2-E10's own root-cause hypothesis was directionally real but
quantitatively minor on this catalog. The dominant ~95% ("neither") is not
one phenomenon; manual inspection of a query-deduplicated sample surfaces
at least four distinct, structurally different causes, most of which no
string-similarity enforcement mechanism could fix:

1. **Generic English words mis-recognized as brands** ("case," "zoom,"
   "head," "drop," "tops," "king," "duck," "cd") -- common nouns/adjectives
   that coincidentally appear as noisy, unvalidated "brand" field values on
   >= the trust threshold's worth of products, but do not function as a
   brand identifier in the query's actual intent ("phone *case*," "duck
   *boots*," "drop-in oven"). A canonicalization false-*positive* problem
   -- the mirror image of Issue #9's false-negative-shaped findings, not
   an enforcement-semantics problem at all.
2. **Sub-brand/product-line naming** ("Dove" vs. "Dove Men + Care,"
   "Milwaukee" vs. "Milwaukee Electric Tool") -- genuinely the *same*
   parent brand family, but a qualifier/product-line suffix, not a
   corporate-legal suffix or a misspelling, so neither `alias_key` nor a
   small edit-distance bound catches it. The one pattern here that *is* a
   plausible, well-scoped enforcement-layer fix not yet tried: a
   containment/prefix check (does one alias key start with the other as a
   whole word) rather than edit distance.
3. **Franchise/media-property vs. manufacturer mismatch** ("Rick and
   Morty" vs. "lytool," "Kinetic Sand" vs. "National Geographic,"
   "Pokemon" vs. "Ultra Pro") -- the query names a franchise/concept the
   catalog's structured brand field never contains at all (it holds the
   licensed manufacturer instead). No string-similarity mechanism can
   bridge this; it needs real entity-relationship knowledge. This is
   concretely what Issue #6's P1-C (predictive semantic prefill) is
   positioned to investigate next.
4. **Missing brand field** ("Savage Arms," "Bosch" -> `<no brand>`) -- the
   real product simply has no brand data; no enforcement-tier change fixes
   an absent field.
5. **Genuinely different/competing or aftermarket-compatible brand**
   ("Ifixit" vs. "Goof Off," "Fitbit" vs. "KingAcc" charger, "RCA" vs.
   "RFAdapter") -- arguably *correct* exclusion by a strict brand filter;
   ESCI's own "Exact" label can reflect strong product-type/functional
   match despite a genuinely different manufacturer (`round1_eval::classify`'s
   own doc comment already flagged this exact tension for "Substitute").
   Not a defect to fix, a labeling nuance to keep in mind when reading any
   brand-filter recall number as if 100% were the correct ceiling.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, full workspace
`cargo test`/`cargo build --release` clean throughout (including after the
`apply_candidates` fix and its four cascading fixture updates). 21
commerce-core tests now covering the new mechanism (`alias.rs`'s 5,
`ir_compiler.rs`'s updated preference-residual assertion) plus every
existing test still green with corrected, explained expected values.

**Decision: REVISE.** The confidence-tiered enforcement *mechanism* is
sound and now bug-free (validated: fixes the exact regression it caused,
twice-reproduced null effect for tier 1, negligible-but-not-negative
effect for tier 2). It is not, however, where the real leverage is on this
catalog: alias/spelling variance is a real but minor (~1-5%) contributor to
the recall gap, not the dominant one P2-E10's own framing suggested. This
sharpens, rather than reverses, P2-E10's finding -- "enforcement semantics,
not recognition quality" was correct as far as it goes, but the *next*
lever is not a better string-matching enforcement mechanism; it is (a) a
false-positive-aware trust gate for generic/ambiguous brand-shaped strings
(item 1), and (b) genuine latent-structure inference for franchise/missing-
brand cases (items 3-4) -- exactly Issue #6's P1-C.

**Next**: feed into `docs/research/PAPER_NOTES.md` (§8.1, §11) and
`ROUND1_DECISION_TREE.md`; proceed to Issue #6's P1-C (predictive semantic
prefill) as the next research cycle, now concretely motivated by real
franchise/missing-brand failure cases rather than the original abstract
"air force 1 -> Nike" example alone. The sub-brand containment idea (item
2) is recorded as a small, well-scoped follow-up, not pursued in this
entry -- P2-E10 and this entry's own Next-step discipline both point
toward P1-C as the higher-information experiment now, not a third
enforcement-mechanism variant.

---

## P2-E12 — Issue #6 P1-C: does catalog-derived predictive semantic prefill move real traffic and preserve relevance?

**Evidence class**: real (full 1,215,854-product catalog, full 22,458-query
judged corpus). **Independence**: yes (ESCI's own human judgments).

**Background**: P2-E11's root-cause diagnostic found franchise/media-
property-vs-manufacturer mismatches ("Pokemon" query, actual brand "Ultra
Pro") and missing brand data as real, sizeable contributors to the brand-
filter recall gap that no string-similarity enforcement mechanism could
address. Issue #6 named predictive semantic prefill -- inferring latent
commerce structure not literally present in the query -- as the concrete
next P1 experiment.

**Hypothesis**: catalog-derived title-phrase-to-brand co-occurrence
(zero model calls, matching `CatalogProfile::build`'s convention) can
predict a brand for a query phrase the existing lexicon cannot resolve at
all, and injecting that prediction (as a confidence-tiered hard `Constraint`
or soft `Preference`, per Issue #6's explicit "do not assume predicted
semantics must become hard constraints") moves some real Punt-shaped
traffic to `Hybrid`/`FastPath` and/or improves structural recall, without
materially degrading integrated relevance.

### Mechanism

`crates/commerce-core/src/cold_start/prefill.rs` (new): `TitlePhraseIndex`
(mechanism trait, mirrors `plan::LexicalDelegate` -- no full-text engine is
a `commerce_core` dependency), `predict_brand_from_phrase` (samples real
products matching a phrase, estimates brand purity), `apply_predictive_prefill`
(scans a query's raw text for 2-3-word windows, adds a hard
`StructuralConstraint::Brand` at high confidence or a
`Preference::StructuralBoost` at medium confidence -- additively, per ADR
0010, never touching `residual_lexical`; never fires if the query already
has an explicit brand constraint; skips a phrase identical to its own
predicted brand's name as not genuinely inferred). Real implementation:
`crates/phase2-eval/src/bin/prefill_eval.rs`, a `TantivyTitlePhraseIndex`
backed by a dedicated title-only Tantivy field and a same-process cache,
reusing the exact planner-integration harness and `measure_precision`
P1-B/P2-E05 already validated. Policy (first pass, not yet tuned):
`ngram_sizes=[2,3]`, `sample_limit=50`, high confidence = purity>=0.90 and
occurrence>=20, medium confidence = purity>=0.65 and occurrence>=8.

### Real-data result, `min_enum_frequency=25`, full 22,458-query corpus

| metric | baseline | with prefill | delta |
|---|---|---|---|
| outcome dist. (FastPath/Hybrid/Punt) | 328/5589/16541 | 328/5671/16459 | +82 Hybrid, -82 Punt |
| structural filter recall (Exact+Sub) | 31.7% | 32.2% | **+0.5pp** |
| structural filter recall (Exact only) | 35.6% | 36.2% | **+0.6pp** |
| zero-result rate | 9.55% | 9.64% | +0.09pp |
| NDCG@10 | 0.2279 | 0.2276 | -0.0003 (noise-level, see below) |
| Recall@10 | 0.1354 | 0.1353 | -0.0001 (noise-level) |
| MRR | 0.3666 | 0.3662 | -0.0004 (noise-level) |
| wall time (22,458 queries) | 137.0s | 153.0s | +16.0s (Tantivy phrase lookups) |

**Direct prefill effect**: 1,133 of 22,458 queries (5.0%) gained a *new*
hard `Brand` constraint they had none of before. Of those, 80 moved to
`Hybrid` and 45 to `FastPath` (both had zero structural constraints
before) -- **125 queries (0.56% of the full corpus) had their execution
route genuinely changed by inferred structure alone**, a real, if modest,
positive answer to Issue #6's own framing question ("does inferred latent
structure move Punt -> Hybrid").

### A real methodological finding, not just a result: floating-point summation order is not deterministic across runs

Two independent runs of the *identical* baseline configuration
(P2-E11's run and this entry's `Mode::Baseline` run, same commit lineage,
same catalog/query files) produced NDCG@10=0.2278 vs. 0.2279 -- not
identical. Root cause: `judged_by_query.values()` iterates a `HashMap`,
whose iteration order is not guaranteed stable across process runs (Rust's
default hasher is randomized per-process); NDCG/Recall/MRR are computed by
summing one `f64` per query, and floating-point addition is not
associative, so a different summation order can produce a different value
in the last 1-2 decimal places. `docs/research/PAPER_NOTES.md` §4.2
previously claimed this pipeline's relevance/correctness numbers are
"exactly reproducible bit-for-bit" -- **corrected**: true for integer-count
metrics (structural filter recall/precision, route-distribution counts,
`measure_precision`'s output), not quite true for `f64`-averaged metrics
(NDCG@10/Recall@10/MRR) at the ~1e-4 level. This means any NDCG/Recall/MRR
delta at or below ~0.0004 between two runs -- exactly the size of every
delta in the table above -- **cannot be distinguished from this
noise floor without either a fixed iteration order (switching the
per-query loop to a `BTreeMap` or a sorted `Vec`, not done in this entry)
or repeated measurement with a confidence interval** (§4.2's own
bootstrap-CI machinery, not yet applied to a relevance metric, only
designed for timing so far). Recorded as a real, previously-unstated
threat to validity, not smoothed over; a candidate small fix for a future
rigor pass, not executed here since it does not change this entry's
conclusion.

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, full workspace
`cargo test`/`cargo build --release` clean. 27 commerce-core tests now
(was 21) -- `prefill.rs`'s 6 new tests cover high/medium/low-confidence
tiering, the explicit-brand-suppression safety rule, and the
identical-to-brand-name exclusion, all against hand-built fixtures (no
real-data dependency for correctness, matching this project's convention
of unit-testing the mechanism and real-data-testing the effect
separately).

**Decision: NARROW.** The mechanism works as designed and is bug-free: it
found real, previously-unreachable brand signal (0.56% of all real traffic
route-changed, +0.5-0.6pp structural filter recall, both exact/reproducible
integer-count metrics, not noise) for exactly the failure class P2-E11
diagnosed it against. But the effect is small in absolute terms -- far
from decisive for the 5-10x aggregate thesis on its own -- and the
downstream *integrated* relevance effect (NDCG/Recall/MRR) is
indistinguishable from measurement noise in this single run, with a small,
real zero-result-rate cost (+0.09pp) from occasionally-wrong predictions.
This is real, positive, reproducible-in-mechanism evidence for a narrow
slice of real traffic (queries naming a franchise/product-family the
catalog's structured brand field doesn't literally contain), not evidence
for a broad win. Whether a larger, decision-grade effect exists requires
either parameter tuning (higher/lower confidence thresholds, more n-gram
sizes) or is capped by how much of the real query mix is actually
franchise/prefill-eligible in the first place -- neither has been tested.

**Next**: fix the HashMap-iteration-order noise source (switch to a
BTreeMap or sorted iteration) before running any further decision-grade
relevance comparison in this campaign, per §4.2's own rigor protocol.
Feed into `docs/research/PAPER_NOTES.md` (§8.2, §4.2 correction, §11).
Proceed to Issue #6's P1-D (physical advantage by query class) as the next
research cycle, now that both P1-B and P1-C have real, evidence-backed
(REVISE / NARROW) conclusions rather than continuing to iterate on
semantic-interpretation experiments alone.

## P2-E13 — Issue #6 P1-D: building the physical-advantage-by-class harness, and two real bugs the first real run found

**Evidence class**: real (full 1,215,854-product catalog, full 22,458-query
judged corpus, a fresh live local Apache Solr 9.10.1 instance re-indexed
with the same real catalog -- `dataset_cache/solr/solr-9.10.1`, not Docker,
not a stale prior run's numbers).

**Hypothesis**: for each of the 9 real-query structural-shape classes
(`round1_eval::query_taxonomy::QueryClass9`), commerce-native structural/
hybrid execution shows a measurable, statistically defensible physical
advantage (throughput, latency percentiles, candidate-set size) over
Solr and an embedded Tantivy-standalone baseline on some subset of
classes, without materially degrading relevance/correctness -- and the
traffic-weighted aggregate across the real query mix indicates whether
that advantage plausibly supports Issue #6's 5-10x north star.

### Infrastructure

New `crates/phase2-eval/src/bin/p1d_physical_advantage_eval.rs`: for each
class, a single-pass correctness phase (up to 200 real queries: NDCG@10/
Recall@10/MRR against real ESCI judgments, `BTreeMap`-ordered iteration
throughout to avoid the HashMap-iteration-order floating-point noise
P2-E12 found) and a separate repeated-measurement latency phase (20
queries x 30 reps/method, methods interleaved via
`bench_harness::round_robin_schedule`, `bench_harness::Distribution` +
`bootstrap_ci_diff_of_means` for the headline commerce-native-vs-baseline
comparison). Three methods per query: commerce-native (`plan::execute_planned`),
an embedded Tantivy-standalone baseline (whole raw query text, no
structural involvement at all -- P2-E01's validated-equivalent-relevance
engine), and the live Solr instance over HTTP (`ureq`).

### First real run: two genuine bugs, not architectural signal

The first full sweep produced two results severe enough to investigate
before trusting any of the numbers around them, per this campaign's own
rule ("if a benchmark methodology problem is discovered, fix the
methodology first"):

**Bug 1 -- `execute_ranked` costing ~1078ms for a single non-selective
FastPath query.** `commerce_core::index::rank::execute_ranked` computed
`effective_attributes` (a per-candidate `HashMap` merge/clone) for *every*
candidate returned by a FastPath query, unconditionally -- even though
`compile_lexicon` (this project's own shipping baseline lexicon, I7-E04)
never emits a real `Preference`, so `query.preferences` is empty on
essentially every real query, and the merged attributes were computed
only to feed a `score_preferences` call that returns `0.0` regardless,
without ever reading them. Fixed by skipping the merge entirely when
`query.preferences.is_empty()`; behavior is byte-identical (every score
`0.0`, same deterministic `(product_id, variant_id)` tie-break), proven by
a new regression test
(`ranking_with_no_preferences_still_returns_every_candidate_score_zero_and_deterministically_ordered`,
`crates/commerce-core/tests/physical_index.rs`). Real measured effect:
`structural_exact_entity`'s commerce-native latency dropped from ~1078ms
to ~0.02ms (see P2-E16's final numbers) -- roughly five orders of
magnitude, entirely wasted work removed.

**Bug 2 -- Solr's `brand`/`color` filters against a completely
unpopulated field.** The harness originally filtered structural brand/
color constraints against `brand_lower`, a Solr schema field. Direct curl
queries against the live Solr instance
(`q=brand_lower:*` -> 0 hits across all 1,215,854 documents; a facet
query on `brand_lower` returned an empty facet list) confirmed it is a
schemaless-mode artifact `scripts/round1/solr_index.py` never actually
populates (it only ever sets `doc["brand"]`), producing a 100%
zero-result Solr baseline for `structural_exact_entity` and
`selective_multi_attribute_structural` -- an artifact of the eval
harness, not evidence about Solr's real capability. Fixed by
`case_insensitive_field_regex`, a case-insensitive Lucene `RegexpQuery`
against the real, raw-cased `brand`/`color` fields (`solr.StrField`, no
case-folding analyzer) -- verified correct via direct curl testing before
porting to Rust (`brand:/[Nn][Ii][Kk][Ee]/` -> 6165 matches vs. exact
`brand:Nike` -> 6160, the extra 5 being genuine case variants
`commerce_core`'s own trim+lowercase brand identity already merges, so
this is the *fair* filter, not a weaker one).

**Regression check**: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, full workspace
`cargo test --workspace --all-features`, `cargo build --workspace
--release` clean after each fix.

**Decision**: methodology-correction entry, not yet a class-level
decision -- proceed to re-run with both fixes applied (P2-E14 found a
third real bug in that very re-run before any class's numbers could be
trusted as final; see below).

## P2-E14 — Issue #6 P1-D: the compiler ANDed mutually-exclusive same-entity constraints together

**Evidence class**: real (same full catalog/corpus as P2-E13).

**Background**: the corrected re-run (P2-E13's fixes applied) showed
`selective_multi_attribute_structural` at 100% zero-result for
commerce-native (22/22 real queries), `NDCG@10=0.0000`, against Tantivy's
`NDCG@10=0.476` on the *identical* queries -- a suspiciously total
failure, not a narrow-selectivity effect (22/22 real queries all
returning literally nothing is implausible as a coincidence).

**Root cause**, confirmed with a real-data diagnostic
(`crates/phase2-eval/src/bin/selective_multi_attribute_diagnostic.rs`):
this harness passes empty `product_types`/`categories` slices to
`CatalogProfile::build` (the real Amazon ESCI catalog has no such field --
`round1_eval::catalog`'s documented `UNKNOWN_PRODUCT_TYPE`/
`UNKNOWN_CATEGORY` sentinel), so the *only* structural entity type the
lexicon can ever emit is `Brand`. All 22 real queries in this class --
e.g. "harry potter lego", "sega genesis", "hot wheels jeep", "funko pop
avengers" -- independently resolved two (or three) different phrases to
two different `Brand` ids, and `ir::query::compile` hard-ANDed them
together. A product has exactly one brand, so
`Brand(Harry Potter) AND Brand(Lego)` is not a narrow query, it is a
guaranteed-empty one for every real product -- a real compiler defect,
not evidence about structural execution's physical cost.

**Fix**: `ir::query::apply_candidates` now tracks which single-valued
"entity slot" (`Brand`/`BrandAny`, `ProductType`, `Category`) each hard
structural constraint occupies. The first phrase to claim a slot keeps
its hard constraint (matching the compiler's existing leftmost/
longest-match-first bias); a later phrase that would conflict with an
already-filled slot falls back to residual free text instead of a
second, mutually-exclusive hard constraint -- this project's own
established structural-narrows/lexical-ranks-residual contract
(`docs/adr/0010`), not a new mechanism. Identical repeated matches (e.g.
"nike nike shoes") are correctly treated as a harmless no-op, not a
conflict.

**RED-first**: `crates/commerce-core/tests/ir_compiler.rs` adds
`conflicting_same_slot_entity_constraints_do_not_get_and_ed_together`
(failed against the pre-fix code, reproducing the real bug shape --
`"nike adidas running shoes"` compiling to both brands ANDed) and
`identical_repeated_entity_matches_are_not_treated_as_a_conflict` (guards
the no-op case). Quality gate green: fmt, clippy `-D warnings`, full
workspace test suite (no regressions in any crate), release build.

**Direct measured effect of the fix**: re-running the corrected harness,
`selective_multi_attribute_structural` dropped to **n=0** -- every one of
the 22 previously-misclassified real queries reclassified into a
different taxonomy class once the second, conflicting brand phrase
correctly fell to residual text instead of a hard constraint. This is
itself a finding, not just a bug fix: on this real catalog, once the
compiler defect is removed, there are **no real queries that genuinely
warrant two or more distinct structural entity constraints together** --
not because commerce-native fails at this class, but because the
catalog's only real, per-product-diverse structural entity dimension is
brand (product type and category are both unpopulated sentinels in this
ingestion). Recorded honestly as a dataset limitation, matching this
harness's own pre-existing doc comment ("Solr's `product_type`/
`category`/price fields do not exist in this real catalog ... reported
honestly as such, not fabricated").

**Decision**: REVISE (compiler defect, fixed) for the mechanism;
`selective_multi_attribute_structural` itself becomes **N/A on this
dataset** rather than a class with a measurable verdict -- proceed to
P2-E15 to check whether the same shape recurs elsewhere before treating
the corrected run as final.

## P2-E15 — Issue #6 P1-D: the same bug generalizes to attribute-level `Enum` constraints, and the remaining zero-result cases are catalog data quality

**Evidence class**: real (same full catalog/corpus as P2-E13/P2-E14).

**Background**: after P2-E14's fix, `variant_scoped_structural` still
showed 68.8% zero-result for commerce-native (22/32 real queries) *and*
53.1% for Solr (17/32) -- both exact-match systems struggling on the same
real queries -- while Tantivy (free-text, no structural involvement) was
0%. That signature (both structural/exact-match systems fail, lexical
does not) reads differently from P2-E14's compiler bug and needed its own
diagnostic before concluding anything.

**Diagnostic**: `crates/phase2-eval/src/bin/variant_scoped_diagnostic.rs`
prints, for each real `variant_scoped_structural` query with zero
commerce-native hits, the compiled constraints and -- for every real
judged-relevant product -- its actual attribute values for the
constrained attribute name(s). This surfaced two distinct root causes:

1. **A recurrence of the same guaranteed-empty-AND bug, one level down.**
   "skeleton toy" independently resolved "skeleton" and "toy" to two
   *different* `color` values and hard-ANDed them
   (`color=Skeleton AND color=Toy`), which no variant can ever satisfy
   since `AttributeValue::Enum` is single-valued per attribute name --
   exactly the same shape as "harry potter lego", just on a
   `Constraint::Enum` instead of a structural entity. **Fixed** by
   generalizing P2-E14's `EntitySlot` into `SingleValuedSlot`, adding an
   `Attribute(String)` case keyed by attribute name for `Constraint::Enum`
   (deliberately *excluding* `Constraint::MultiEnumContains`, which is
   multi-valued by design -- a variant can legitimately carry several
   tags/features at once, so two different tag matches should still
   combine). Same first-wins/residual-fallback mechanism, no new code
   path. RED-first:
   `conflicting_same_attribute_enum_constraints_do_not_get_and_ed_together`
   (`crates/commerce-core/tests/ir_compiler.rs`, using the existing
   `shoe_lexicon` fixture's two color values, "black red running
   shoes"). Quality gate green: fmt, clippy `-D warnings`, full workspace
   test suite, release build.

2. **Genuine catalog data-quality noise, not a bug** -- the diagnostic's
   remaining zero-result cases, left as negative evidence rather than
   chased further:
   - **Color-vocabulary granularity mismatch**: "moss tile" compiles to
     `color=Moss`, but real judged-relevant products carry `color=Green`,
     `Juniper`, `Blackcoffee`, `"2"`, `"M"`; "pumpkin chapstick" compiles
     to `color=Pumpkin` against relevant products' `Pumpkin Spice`,
     `Non-tinted`, or no color at all. The catalog's raw `color` field is
     visibly populated with non-color garbage values in places (`"2"`,
     `"M"`), consistent with this project's already-established brand-
     vocabulary-noise finding (Issue #6 point 3), now confirmed to extend
     to the `color` attribute.
   - **A garbage value trusted as real signal**: "playstation 1" compiled
     to `Brand(PlayStation) AND color="1"` -- the trailing "1" (a console
     generation number) spuriously matched a real but nonsensical
     `color="1"` lexicon entry that had cleared the `min_enum_frequency`
     trust gate, because occurrence-frequency alone cannot distinguish a
     genuine controlled-vocabulary value from noise that happens to recur
     >=25 times across 1.2M real products.
   - **Attribute entirely absent, not mismatched**: "luvs size 3" and
     "ring size 4" both compile a numeric `size` constraint, but every
     real judged-relevant product for both queries has *no* `size`
     attribute in its `effective_attributes()` map at all -- these
     product categories (diapers, rings) simply don't carry structured
     size data in this ingestion, so any numeric size constraint is
     unsatisfiable regardless of value.
   - **A recurrence of the already-known brand-exact-match gap**:
     "nintendo poster" has real relevant products with `color="Poster"`
     matching *exactly*, yet `matches_query=false` -- the `Brand(Nintendo)`
     half is failing, the same real-brand-string-vs-compiled-identity gap
     P2-E11 already diagnosed and found not worth chasing further with
     string-similarity mechanisms.

**Decision**: REVISE (compiler defect generalized and fixed) for the
mechanism. `variant_scoped_structural`'s remaining zero-result rate is a
genuine, class-level finding about this real catalog's attribute data
quality -- not a commerce-native-specific defect (Solr fails on the same
real queries for the same reason) and not fixable by more compiler logic.
Recorded as negative evidence, feeding into P2-E17's final class-by-class
verdict and `docs/research/PAPER_NOTES.md` §10/§11.

**Next**: re-run the full P1-D sweep with all three fixes applied as the
final, stable measurement; adversarially review the result (could a
speedup be a benchmark artifact, are baselines fair, is relevance being
traded away, does it survive repeated runs, which classes create the
advantage, what is the weighted advantage under the real query mix, what
would falsify the conclusion) before writing the class-by-class and
traffic-weighted verdict.

## P2-E16 — Issue #6 P1-D: adversarial review finds the harness's own Solr *latency* measurement was broken

**Evidence class**: real (same full catalog/corpus as P2-E13-E15), plus a
4-agent adversarial-review workflow tasked with independently re-deriving
the traffic-weighted economics, auditing the harness for fairness issues,
auditing the relevance guardrail, and root-causing why commerce-native's
Hybrid/Punt path measured slower than Solr -- run against the corrected
(P2-E13/E14/E15) sweep's raw log, before treating its ~3.5-4.1x-slower
traffic-weighted result as this campaign's answer.

**What the review confirmed**: the `weighted_economics` and
`fairness_audit` agents *independently* re-derived the same ~3.5-4.1x
(median/mean) slower ratio from the raw log (agreement to 3-4 significant
figures via two different methods -- hand computation and an independent
re-implementation), and both flagged the same class-uniformity evidence:
every one of the six populated query classes representing >0.1% of real
traffic (99.19% of the corpus) individually showed commerce-native slower
than Solr, while its only wins (~39-49x faster) were confined to two
classes totaling 0.81% of traffic.

**What the review found wrong -- a severe, previously-unnoticed harness
bug**: two independent agents (`fairness_audit` and
`hybrid_overhead_rootcause`), working from different angles, converged on
the same defect. The harness's *latency* sub-experiment measured Solr via

```rust
solr_search(&solr_base_url, text, &[], K)
```

-- the raw query text as `q`, an **empty** `fq`, bypassing
`solr_query_for()`'s edismax/`all_text`/brand-color-`fq` construction
entirely -- in both the warmup loop and the timed measurement loop. The
*correctness* sub-experiment, a few dozen lines earlier in the same file,
correctly builds `(q, fq)` via `solr_query_for()`. Root cause (confirmed
by reading `dataset_cache/solr/solr-9.10.1`'s own `solrconfig.xml`/
`managed-schema.xml`, then live-verified against the running Solr core):
Solr's `<str name="df">_text_</str>` makes `_text_` the default search
field for an unqualified `q`, but the schema's only `copyField`s target
`all_text`, not `_text_` -- so `_text_` holds zero indexed content across
all 1,215,854 documents. Every "solr latency" measurement in every prior
P1-D run was timing a guaranteed-zero-hit lookup against an empty field,
not real search work. Live reproduction against the exact running Solr
core:

```text
q=running+shoes           (broken)  -> numFound=0,     time_total≈0.002s
q={!edismax qf=all_text}running+shoes (fixed) -> numFound=57808, time_total≈0.032s
```

This explains why Solr's reported latency was suspiciously flat
(~1-2ms, sd 0.2-0.5ms) across every query class regardless of difficulty
in P2-E13/E14/E15's runs, while commerce-native/Tantivy showed wide,
class-dependent spread that actually tracked real work -- and it directly
means the prior ~3.5-4.1x-slower number, while independently reproduced
twice, was measured against an artificially-fast, not-really-searching
Solr baseline and cannot be trusted as a final figure.

**Fix**: factored the brand/color extraction the correctness loop already
did inline into `extract_brand_color()`; the latency sub-experiment now
precomputes the same `solr_query_for()` `(q, fq)` for every query in the
latency sample *before* the timed loop -- symmetric with how
commerce-native's own `compile()` cost is excluded from its timed block
via `compiled_cache` (an intentional, disclosed asymmetry the
`fairness_audit` agent separately flagged as small and favoring
commerce-native, concentrated in the negligible-traffic FastPath classes),
not a new one.

**A second, independently-evidenced (and independently fixable)
inefficiency** the same review surfaced, in `commerce_core::plan` itself
rather than the harness: `execute_planned`'s `Punt` branch asked the
delegate for `k * delegate_oversample` (200) results even when
`query.constraints` is empty -- exactly `lexical_first`'s shape (36.8% of
all real traffic, 100% `Punt`-via-no-constraint). `CommerceQuery::matches_variant`'s
`.all()` over an empty constraint list is vacuously true for every hit,
so no delegate hit can ever be rejected on constraint grounds there, and
oversampling cannot change which `k` hits end up returned -- only force
the delegate (a real Tantivy `TopDocs` collection plus per-hit stored-
field fetches) to do 20x more work for an identical result. **Fixed**:
`execute_planned` now only applies `delegate_oversample` when
`query.constraints` is non-empty (`Hybrid`, and `Punt`-via-non-
selectivity, both of which have real constraints that can reject a
delegate hit and genuinely need the headroom).

**RED-first**: `crates/commerce-core/tests/plan.rs` adds
`punt_with_no_constraints_at_all_asks_the_delegate_for_exactly_k_not_the_oversampled_limit`
(failed against the pre-fix code: delegate called with `limit=200`, not
`10`), and extends the existing `Hybrid`/`Punt`-via-non-selectivity tests
to assert they still correctly request the oversampled limit. Quality
gate green: fmt, clippy `-D warnings`, full workspace test suite, release
build.

**Other issues the review found and explicitly did *not* treat as
blocking** (recorded as threats to validity, `docs/research/PAPER_NOTES.md`
§10, not further "fixed" this cycle): the correctness/latency query
samples are a deterministic "smallest-N-by-native-ESCI-query-id"
selection, not random or stratified, covering as little as ~2.4-6% of the
four classes that carry >99% of real traffic; every per-class latency
distribution/bootstrap CI is built from only 20 unique queries x 30 reps
(repeated measurement of the same 20 query shapes, not 30 independent
real queries), which understates true query-to-query variance; `FastPath`
has no selectivity safeguard at all (unlike `Hybrid`/`Punt`, which
explicitly gate on `selectivity <= policy.selectivity_threshold|`) --
`range_plus_structural`'s single sampled non-selective query shows
commerce-native's worst latency in the entire log (mean ~30ms, actually
slower than both baselines), a real, source-verified warning that
`FastPath`'s dramatic wins may be a property of this corpus's favorable
`structural_exact_entity`/`variant_scoped_structural` samples rather than
a general architectural guarantee -- currently negligible traffic weight
(0.01%) but a genuine open risk; and the harness's own `TantivyDelegate`
reference implementation turns `Hybrid`'s `restrict_to` into a per-query
`TermSetQuery` over the full narrowed candidate set (up to ~60K terms),
undermining some of the narrowing's own cost advantage -- a delegate-
implementation limitation, not a `commerce_core::plan` defect (the trait
boundary is respected), but relevant to any real deployment wanting to
realize `Hybrid`'s full benefit.

**Corrected traffic-weighted result** (re-running with both fixes,
commit after this entry): weighted commerce-native mean latency dropped
from ~6.98ms/~1.72ms (Solr) to ~6.16ms/~2.05ms -- Solr's weighted mean
roughly *doubled* (1.72ms -> 2.05ms) once it started doing real work,
narrowing the ratio from ~4.05x to **~3.01x slower** (mean-weighted) and
from ~3.52x to **~2.31x slower** (median-weighted). The qualitative
conclusion -- commerce-native slower than Solr on every class representing
material real traffic -- survives this correction; the magnitude
meaningfully narrows. See P2-E17 for the full corrected class-by-class
results and final decision.

**Decision**: methodology-correction entry (a second one this campaign,
after P2-E13) -- "if a benchmark methodology problem is discovered, fix
the methodology first," including when the flawed measurement happens to
favor the baseline rather than commerce-native. Proceed to P2-E17 for the
corrected, adversarially-reviewed final verdict.

## P2-E17 — Issue #6 P1-D/P1-E final result: physical advantage by query class, traffic-weighted economics, and decision

**Evidence class**: real (full 1,215,854-product catalog, full
22,458-query judged corpus, live local Solr, 3 independent full-corpus
sweeps across this experiment's debugging cycle -- P2-E13/14/15's
corrected run, and this entry's final P2-E16-corrected run -- all with
the same 9-class taxonomy, `bench_harness`-driven 30-rep/method repeated
measurement, and bootstrap CIs). Commit at time of this run:
the P2-E16 fix commit.

### Class-by-class result (final, corrected numbers)

| class | real n | traffic share | outcome | CN mean latency | Solr mean latency | CN/Solr ratio | CN NDCG@10 | Solr NDCG@10 | zero-result (CN / Solr) |
|---|---|---|---|---|---|---|---|---|---|
| structural_exact_entity | 153 | 0.68% | FastPath | 0.0172ms | 1.499ms | **87x faster** | 0.161 | 0.235 | 0% / 0% |
| selective_multi_attribute_structural | 0 | 0% | -- | N/A on this dataset (no real product_type/category data; see P2-E14) | | | | | |
| variant_scoped_structural | 30 | 0.13% | FastPath | 0.0148ms | 1.559ms | **105x faster** | 0.014 | 0.035 | 66.7% / 56.7% |
| range_plus_structural | 2 | 0.01% | FastPath | 30.27ms | 1.660ms | **18x SLOWER** | 0.000 | 0.000 | 0% / 0% |
| structural_plus_lexical_residual | 3253 | 14.48% | Hybrid | 11.72ms | 3.997ms | **2.9x slower** | 0.082 | 0.075 | 30.5% / 30.0% |
| structural_plus_semantic_residual | 57 | 0.25% | Hybrid | 6.29ms | 1.660ms | **3.8x slower** | 0.114 | 0.114 | 21.1% / 21.1% |
| lexical_first | 8274 | 36.84% | Punt | 5.93ms | 1.954ms | **3.0x slower** | 0.276 | 0.277 | 1.0% / 0.5% |
| ambiguous_punt | 5005 | 22.29% | mixed | 6.35ms | 1.877ms | **3.4x slower** | 0.130 | 0.128 | 5.5% / 5.0% |
| long_tail_noisy | 5684 | 25.31% | mixed | 3.33ms | 1.229ms | **2.7x slower** | 0.171 | 0.173 | 4.5% / 2.5% |

(All bootstrap CIs for the commerce_native-vs-solr latency diff exclude
zero in every populated class except the n=2 `range_plus_structural`
row, per `docs/research/artifacts/p1d_run5/full_run_output.log`.)

### Traffic-weighted whole-workload economics

Weighting each class's mean/median latency by its real query-count share
of the full 22,458-query corpus (Python re-derivation, cross-checked by
the adversarial-review workflow's independent implementation, agreeing to
4 significant figures):

- **Mean-weighted**: commerce-native 6.158ms vs. Solr 2.045ms -> **~3.01x SLOWER**.
- **Median-weighted**: commerce-native 3.871ms vs. Solr 1.679ms -> **~2.31x SLOWER**.

Against Issue #6's north-star target of roughly **5-10x FASTER**, this is
a clear, corrected, adversarially-reviewed **negative result on the
weighted whole-workload measure**, not a narrow miss. The uniformity is
the strongest part of the evidence, not the aggregate ratio alone: every
one of the six populated classes representing more than 0.1% of real
traffic (99.19% of the corpus) individually shows commerce-native slower
than Solr, by 2.7x-3.8x depending on class. Commerce-native's only wins
(87x, 105x faster) are confined to two classes totaling 0.81% of real
traffic on this real catalog/query corpus.

### The FastPath wins are real, but not "free" -- a relevance guardrail failure

The adversarial review's `relevance_guardrail_audit` traced the exact
mechanism: `commerce_core::index::rank::execute_ranked` only computes a
nonzero score when `query.preferences` is non-empty; the shipping
baseline lexicon (`compile_lexicon`, used throughout this benchmark) never
emits a real `Preference` (confirmed at the source level: the only path
that ever does is `compile_lexicon_with_alias_enforcement`'s fuzzy tier-2
brand match, which this benchmark does not use). So for every real query
in `structural_exact_entity`/`variant_scoped_structural`, `execute_ranked`
scores every hit `0.0` and the final sort falls through entirely to the
`(product_id, variant_id)` tie-break -- FastPath's "top 10" is the first
10 matching products in ascending, ingestion-order `ProductId`, with
**zero relevance signal applied**. This directly explains
`structural_exact_entity`'s NDCG@10 gap (0.161 vs. Solr's 0.235, a 31.5%
relative loss) and an even larger MRR gap (0.153 vs. 0.361, -58% relative,
since MRR is most sensitive to first-hit rank, which is essentially
random under ID-order truncation). `variant_scoped_structural`'s already-
poor NDCG (0.014) is a compound effect: the 66.7% zero-result rate is
genuine catalog data-quality noise (P2-E15's diagnostic; Solr suffers a
similar 56.7% rate on the same real queries for the same underlying
reason, Tantivy 0%), but the ~33% of queries that *do* find a match are
also arbitrarily ID-ordered rather than ranked, plausibly explaining why
commerce-native's NDCG is still ~2.4x worse than Solr's even on the
shared zero-result problem. **Verdict**: for both FastPath-eligible
classes, "5-10x faster without materially degrading relevance" is not
supported by this evidence -- relevance *is* materially degraded. This is
a real, fixable engineering gap (a default ranking signal -- even a
simple catalog-derived proxy -- could be added to FastPath without
touching routing), not evidence that structural exact-match retrieval can
never be both fast and relevant, but as measured at this commit it is a
cost bundled with the speed win, not a pure win on both axes.

### Final adversarial-review checklist (per Issue #6's own requirement)

1. **Could the speedup be a benchmark artifact?** Partially, for the
   FastPath wins: some of the 87-105x multiplier is inflated by comparing
   an in-process Rust call against Solr's HTTP+JSON round trip. Controlling
   for that via Solr's own reported server-side `avg_server_qtime`
   (0.83-0.97ms across these classes) rather than full wall-clock, the
   advantage is still ~50-90x -- large and real even after removing the
   HTTP-boundary confound. For the Hybrid/Punt "slower" result: the
   original ~3.5-4.1x figure *was* partly a benchmark artifact (P2-E16's
   broken Solr latency measurement); corrected, a real, smaller (~2.3-3.0x)
   gap remains, independently reproduced across the debugging cycle's
   three full-corpus runs.
2. **Are baselines fair?** Yes, after P2-E16's fix: the same fresh,
   same-environment Solr instance measured with a matching query
   construction in both the correctness and latency sub-experiments; the
   Tantivy-standalone baseline reuses P2-E01's already-validated-
   equivalent-relevance engine. Residual, smaller asymmetries remain
   (documented, not blocking): commerce-native's own query-compile cost is
   excluded from its timed block the same way Solr's/Tantivy's per-query
   parsing is not -- small and favoring commerce-native, concentrated in
   the negligible-traffic FastPath classes.
3. **Are we trading relevance for speed?** Yes, for the FastPath classes
   specifically (above) -- a real, disqualifying-for-a-clean-win finding.
   The Hybrid/Punt classes show comparable (not better, not
   catastrophically worse) relevance to Solr, so the "slower" result there
   is not offset by a relevance win either.
4. **Does it survive repeated runs?** The qualitative direction (FastPath
   dramatically faster on two negligible-traffic classes; Hybrid/Punt
   consistently slower on the traffic-dominant classes) held across three
   independent full-corpus sweeps (P2-E13/14/15's run and this entry's
   P2-E16-corrected run), each with 30-rep/method bootstrap CIs excluding
   zero. A real limitation, explicitly not hidden: the underlying latency
   samples are only 20 unique queries per class, repeated 30 times each
   -- not 30 independent draws from the class's full real population (up
   to 8274 queries) -- so the *exact* multiplier carries more uncertainty
   than the tight-looking CIs suggest. The direction is corroborated by
   Solr's own correctness-loop `avg_server_qtime` (computed over the
   larger, though still non-random, 200-query correctness sample),
   pointing the same way.
5. **Which classes create the advantage?** `structural_exact_entity` and
   `variant_scoped_structural`, both 100% `FastPath`, together 183 of
   22,458 real queries (0.81%). `selective_multi_attribute_structural` is
   empty on this dataset (P2-E14). `range_plus_structural` is *not* an
   advantage (n=2, commerce-native 18x slower, a real warning sign --
   see below).
6. **What is the weighted advantage under a realistic mix?** ~2.3-3.0x
   **SLOWER**, not 5-10x faster, using this real dataset and its real
   query-class distribution.
7. **What would falsify this conclusion?** (a) A real catalog with genuine
   structured `product_type`/`category`/price data (this Amazon ESCI
   export has none -- P2-E14's finding) could populate
   `selective_multi_attribute_structural` and shift real traffic further
   into FastPath-eligible shapes; this real dataset cannot test that.
   (b) Fixing FastPath's missing ranking signal would make the two
   current wins "clean," though they would still be <1% of traffic on
   *this* dataset. (c) A bitmap/doc-id-set-based `Hybrid` delegate
   restriction (instead of the harness's reference `TantivyDelegate`'s
   per-query `TermSetQuery`, which the review flagged as undermining
   `Hybrid`'s own narrowing advantage) could narrow the gap for the 14.7%
   of traffic in `structural_plus_lexical_residual`/
   `structural_plus_semantic_residual`. (d) A random/stratified re-sample
   with a much larger per-class N would tighten the exact multiplier's
   confidence interval, though is unlikely to reverse the direction given
   the already-observed uniformity across all six dominant-traffic
   classes.

### `range_plus_structural`'s 18x-slower result: a real, unresolved architectural risk, not noise to ignore

`plan()` (`crates/commerce-core/src/plan/mod.rs`) routes to `FastPath`
purely on `query.residual_lexical.is_empty()`, with **no check on how
large the resulting structural candidate set is** -- unlike `Hybrid`/
`Punt`, which explicitly gate on `selectivity <= policy.selectivity_threshold`
specifically to avoid materializing-and-sorting a non-selective candidate
set (R1-E05's own finding, restated in this module's doc comment).
`execute_ranked`/`CatalogIndex::execute` then unconditionally builds a
`Vec` over every matching candidate and fully sorts it (`O(n log n)`, not
a partial-select) before truncating to `k`. `range_plus_structural`'s two
real queries happened to resolve to a large, non-selective candidate set,
and commerce-native's FastPath latency (mean 30.27ms, one measurement
running to 96ms in an earlier run) was worse than *both* baselines --
exactly the "materialize + fully sort a huge candidate set" cost pattern
`Hybrid`/`Punt`'s selectivity gate exists to avoid, but with no such
avoidance available to `FastPath`. With n=2 this specific number carries
~0% weight in the traffic-weighted aggregate and does not change the
reported ratios either way, but it is a legitimate, source-verified
signal that `structural_exact_entity`/`variant_scoped_structural`'s
dramatic wins may be a property of this corpus's favorable samples for
those two classes, not a general architectural guarantee that *every*
FastPath query is cheap. Recorded as an open risk, not fixed this cycle
(no failing real query volume currently justifies the smallest-correct-fix
discipline; would need either a `FastPath`-side selectivity check or
real data showing this matters at more than 0.01% of traffic).

### Decision: **NEGATIVE RESULT** for Issue #6's whole-engine 5-10x thesis, on this real catalog/query corpus

Per the campaign's own stop-condition framework: "after reasonable
permutations and ablations, mature Solr/Lucene/Tantivy execution erases
the weighted advantage ... preserve the evidence and write the negative
result clearly." That is what this entry does. Five real, distinct bugs
were found and fixed across P2-E13-E16 (an unconditional full-catalog
attribute merge, an unpopulated Solr schema field used for correctness,
a compiler defect ANDing mutually-exclusive brand constraints, the same
defect generalized to attribute-level `Enum` constraints, and a broken
Solr latency measurement plus a real delegate-oversample inefficiency) --
each investigated to root cause, fixed with the smallest correct change,
and validated with a RED-first regression test, exactly per this
campaign's engineering discipline. After all five fixes, the result is
not a universal 5-10x win, narrowed to a smaller-but-still-decisive win,
or a wash: it is that commerce-native's structural/hybrid architecture is
**dramatically faster (87-105x) on <1% of real traffic, with a real,
uncorrected relevance cost on that same slice, and consistently slower
(2.7-3.8x) on the 99%+ of real traffic that reaches `Hybrid`/`Punt`**,
where an embedded lexical delegate call is added on top of (not instead
of) structural planning, at comparable relevance to a single mature Solr
call. The traffic-weighted whole-workload economics is
**~2.3-3.0x SLOWER than Solr, not 5-10x faster**.

This does not mean the structural retrieval mechanism itself is wrong --
`FastPath`'s per-query cost really is two to three orders of magnitude
below Solr's when a query resolves entirely to structural constraints and
that structural predicate is selective. It means: (a) this real dataset's
actual query mix does not contain enough of that query shape for the win
to matter economically; (b) the current implementation does not realize
`Hybrid`'s intended narrowing benefit due to a delegate-implementation
limitation (`TermSetQuery` over a large candidate set); and (c) even the
classes that do win physically do not yet win on relevance. None of these
are contradicted by anything found this cycle -- they are exactly what
was measured, after real, adversarially-verified debugging effort, not
before it.

**Next**: update Issue #6 and `docs/research/PAPER_NOTES.md` (§8.3, §9
ablations, §10 limitations, §11 negative findings, §12 conclusion) with
this result. Given the campaign's decision discipline (KEEP/REVISE/
DELEGATE/P2/STOP), P1-D's own verdict is closest to **STOP** for the
whole-engine thesis as currently measured on this dataset -- but Issue #6
spans P1-A through P1-F, and P1-B/P1-C's own REVISE/NARROW verdicts (soft
enforcement mechanism sound but small effect; predictive prefill narrow
but real) are independent of this result and do not need to be
re-litigated here. The campaign-level synthesis belongs in Issue #6's
own update and a `SCALE_UP_DECISION.md`-style final artifact, not
repeated in this log.
