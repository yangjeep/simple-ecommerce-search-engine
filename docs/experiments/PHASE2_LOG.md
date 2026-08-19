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
