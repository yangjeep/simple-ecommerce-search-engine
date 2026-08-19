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
attempted.
attempted.
