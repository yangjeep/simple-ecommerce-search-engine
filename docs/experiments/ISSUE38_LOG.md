# Issue #38 Experiment Log — compiled merchant intelligence into a context-light search plane

## Execution order (per Issue #38 itself)

1. Repair the Phase 9 `compile_lexicon`/`compile()` resolution-priority
   defect, RED tests first. **Done: P9-E05**
   (`docs/experiments/PHASE9_LOG.md`).
2. Re-run the affected Phase 9 H1/H3 measurements from the corrected
   baseline. **Done: P9-E06** (`docs/experiments/PHASE9_LOG.md`).
3. Freeze the corrected baseline. **Done**, `PHASE9_DECISION.md`.
4. Run E1. **This log.**
5. Only if E1 survives its gate, continue E2-E5.

## I38-E1: hard-coded (A) vs compiled-schema (B/B2) vs runtime-generic (C) vs Solr (D)

**Primary research question**: can a dynamically discovered merchant
feature model compile into a serving artifact whose hot-path cost remains
close to a hand-specialized native implementation?

**Concretization used here**: `commerce_core::index::CatalogIndex`
already draws exactly the line Issue #38 asks about, for two different
field kinds in the same real WANDS catalog. `product_type`/`brand`/
`category` are hard-coded Rust struct fields on `Product`, each backed by
a dedicated `HashMap<TypedId, RoaringBitmap>` (path A). Arbitrary catalog
attributes (color, material, ...) are looked up by *string* name through
a generic `HashMap<(String, String), RoaringBitmap>` (`enum_bitmaps`) --
the same physical primitive (`RoaringBitmap` intersection) reached
through a schema-flexible, string-keyed path instead of a typed one. E1
reuses that exact generic mechanism -- not a new invention -- as the
compiled-schema path's representation: the *same catalog field*
(`product_type`) is compiled into it via a purpose-built
`issue38_eval::CompiledEnumIndex`, discovered the same catalog-agnostic
way `cold_start::profile`/`compile_lexicon` already discover real
attribute vocabulary.

**Four (then five) paths, one catalog, one workload**:
- **A** (hard-coded): `CatalogIndex`'s existing `product_type_bitmaps`.
- **B** (naive compiled schema): `CompiledEnumIndex`, a
  `(field, value) -> RoaringBitmap` map.
- **B2** (redesigned compiled schema, added mid-experiment -- see below):
  `CompiledOrdinalIndex`, one dedicated `HashMap<String, RoaringBitmap>`
  per compiled field.
- **C** (deliberate runtime-generic strawman): `GenericStore`, a fully
  generic per-document field map with no precomputed index at all, the
  "accidental Rust-Solr" this project's architecture explicitly avoids
  building for production (CLAUDE.md).
- **D** (context baseline): fresh, same-run Apache Solr 9.10.1
  (`wands_bench` core, identical to Phase 9), `fq=product_class:"..."`.

**Workload**: 40 real `product_type` values, stratified across the
catalog's actual selectivity range (max variant count=1316, min=1) --
not one arbitrarily chosen value. `ProductTypeId(0)`, the ingestion
sentinel `phase6a_eval::catalog::build_catalog` assigns to products
missing a real `product_class`, is excluded from the query workload (it
is not a real merchant-relevant structural entity) but still represented
consistently across all paths since every path iterates every product
unconditionally.

**Correctness first**: `crates/issue38-eval/tests/agreement.rs` --
before any latency/memory number is trusted, A/B/B2/C must return the
*identical* hit set for the identical query on the identical catalog (a
small hand-built fixture with three product types, multi/single-hit and
absent-value cases). All pass. The main experiment binary also
cross-checks hit *counts* across all 40 real workload queries on every
run (0 mismatches in every run recorded here) -- a benchmark comparing
differently-correct engines would be worthless. **D (Solr) is
deliberately excluded from this specific cross-check**: it is reported as
context only throughout this experiment, not part of the B-vs-A gate
decision, and Solr's own tokenization/field-matching semantics for
`product_class` are a separate, already-established concern from earlier
Phase 9 work, not re-litigated here.

### First result: B fails the gate

Path B (`CompiledEnumIndex`) vs A, p50 latency, across 5 independent
runs (numbers below are the final, `black_box`-corrected figures --
see "A second methodology bug" below for the pre-correction numbers,
which read higher and were caught and replaced, not blended in):

| run | A p50 (ms) | B p50 (ms) | B overhead |
|---|---|---|---|
| 1 | 0.0001 | 0.0001 | +63.55% |
| 2 | 0.0001 | 0.0001 | +63.46% |
| 3 | 0.0001 | 0.0001 | +63.03% |
| 4 | 0.0001 | 0.0001 | +63.73% |
| 5 | 0.0001 | 0.0001 | +64.50% |

**DOES NOT PASS** the `<=5%` initial target, reproducibly (a ~63-65%
overhead band across 5 runs, not a one-off).

### Localizing the overhead, per Issue #38's own instruction

Per-query allocation counts (a real, non-noisy, deterministic metric,
measured via a custom counting global allocator) for one representative
median-selectivity query:

| path | allocs/query | bytes/query |
|---|---|---|
| A | 2 | 42 |
| B | 4 | 64 |

**A allocates 2; B allocates exactly 2 more.** `CompiledEnumIndex::query_eq`
mirrors `CatalogIndex::attribute_bitmap`'s real production shape for
`Constraint::Enum`: `self.bitmaps.get(&(field.to_string(),
value.to_string()))` -- building the tuple lookup key clones both
strings on *every* query, since `HashMap<(String, String), _>::get`
cannot borrow a tuple from two `&str`s. This exactly explains the
overhead: two small heap allocations plus a two-string hash, on an
otherwise ~100-nanosecond operation, is a large *relative* cost even
though it is a tiny *absolute* one.

**Is this fundamental or implementation-specific?** `commerce_core::index::CatalogIndex`
itself already answers this question for a structurally identical
problem: Issue #21 Phase 6D built exactly this kind of dictionary/ordinal
representation (`enum_dictionary`/`enum_columns`,
`brand_dictionary`/`category_dictionary`/`product_type_dictionary`)
specifically because a per-candidate `String` hash/clone was found to
cost real, measurable time in a different context (facet counting).
There is a directly applicable, already-proven-in-this-codebase reason
to believe the same class of cost is eliminable here -- exactly Issue
#38's bar for "redesign and re-test," not a speculative hope.

### The redesign: B2 (`CompiledOrdinalIndex`)

One dedicated `HashMap<String, RoaringBitmap>` *per compiled field*
(rather than one shared map keyed by `(field, value)`). `query_eq(value:
&str)` needs no allocation at all: `HashMap<String, _>::get` accepts a
borrowed `&str` directly via `Borrow<str>`. Still fully "compiled from a
dynamically discovered schema," not a hard-coded Rust struct field --
the redesign changes only the key shape, not the "discovered at compile
time, not baked into the domain model" property Issue #38 asks about.

| path | allocs/query | bytes/query |
|---|---|---|
| A | 2 | 42 |
| B2 | 2 | 42 |

**Identical to A.** B2 vs A, p50 latency, across 5 independent runs
(final, `black_box`-corrected figures):

| run | A p50 (ms) | B2 p50 (ms) | B2 overhead |
|---|---|---|---|
| 1 | 0.0001 | 0.0001 | -5.50% |
| 2 | 0.0001 | 0.0001 | -5.24% |
| 3 | 0.0001 | 0.0001 | -6.37% |
| 4 | 0.0001 | 0.0001 | -4.96% |
| 5 | 0.0001 | 0.0001 | -5.31% |

**PASSES** the `<=5%` target comfortably and reproducibly -- B2 is not
measurably slower than A at all in this measurement (the small negative
numbers are not claimed as "B2 architecturally beats A"; a marginally
simpler dispatch path than `CatalogIndex::structural_bitmap`'s enum
match is a plausible, disclosed, minor implementation detail, not an
architecture-level finding).

**Conclusion for B's overhead**: implementation-specific, not
fundamental. The naive tuple-keyed design was a real, measured, avoidable
defect in *this specific* compiled-schema implementation, not evidence
against "compiled schema flexibility can preserve the hard-coded
executor's physical advantage" as a general claim.

### A real methodology bug caught mid-experiment (disclosed, not smoothed over)

The first measurement design timed each individual call via
`Instant::now()`, 10 reps per query, round-robin-interleaved across
methods (matching this project's own anti-drift discipline). Adding B2
to that same 4-way round-robin (making it 5-way) produced a **sign
flip**: B's overhead read **+19.24%** in one run and **-14.60% to
-21.10%** across the very next set of runs -- the same code, no
production changes, just a different random interleaving. At ~1
microsecond per query, `Instant::now()`'s own call overhead and OS
scheduling jitter are comparable in magnitude to the operation being
measured, so individual-call timing at this scale is not trustworthy.

**Fix**: batch `IN_PROCESS_BATCH=200` consecutive same-query calls per
timed sample for the three O(1) in-process paths (A/B/B2), dividing
elapsed time by the batch size -- diluting timer/jitter overhead by two
orders of magnitude. Raised `REPS_PER_QUERY` to 30, this project's own
established ">=30 reps for anything decision-grade" bar (10 reps is
"exploring" tier per `bench-harness`'s own doc comment). Applying the
same 200x batching to path C (a full `O(catalog size)` linear scan per
call, ~0.6ms) by mistake first turned a ~3-second experiment into one
that did not finish in 5 minutes -- caught directly (the command timed
out) rather than shipped; C and D are timed unbatched instead, since
their absolute per-call latency (~0.6-0.75ms) is already ~1000x above
timer resolution and does not need batching for precision.

Post-fix, both B's failure and B2's pass are stable and reproducible
across 5 independent runs (see tables above) -- the sign-flip artifact
is gone, not merely relocated.

### A second methodology bug, caught adversarially before finalizing

Before drawing the final conclusion, an adversarial review question was
asked directly: with `[profile.release] lto = true, codegen-units = 1`
(this workspace's own release profile) and a batch loop that calls the
*same* method with the *same* arguments 200 times, discarding every
result but the last, is there any risk the compiler proves the repeated
calls are redundant and hoists/elides work it should not be able to for
an honest per-call measurement? This is a real, known class of
microbenchmarking error in Rust, not a hypothetical one.

Checked directly: the batch loops did not wrap each iteration's result in
`std::hint::black_box`, only reading `.len()` into a variable overwritten
every iteration. Adding `std::hint::black_box` around each iteration's
returned bitmap (forcing the optimizer to treat every call as
independently observable) **did change the measured numbers**: B's
overhead dropped from the ~75% band above to a **~63-65% band** (still
clearly failing the gate, just a less inflated failure), and B2's
overhead dropped in magnitude from ~-10% to -13% to **~-5% to -6%**
(still clearly passing, just not as dramatically "better than A" as the
unguarded measurement suggested). Both qualitative conclusions
(B fails, B2 passes) are unchanged and remain reproducible across 5
fresh runs with the fix applied -- but the *magnitudes* reported
everywhere in this log and in `ISSUE38_DECISION.md` are the
`black_box`-corrected ones, not the first, uncorrected reading, which is
not reused anywhere as evidence.

This is disclosed for the same reason as the sign-flip bug above: a
benchmark result that happens to look clean is not automatically a
trustworthy one, and this project's own discipline is to keep checking
until a result survives an adversarial pass, not to stop at the first
number that supports a conclusion.

### C (runtime-generic strawman) and D (Solr): context

| path | p50 ratio vs A (range across 5 runs) |
|---|---|
| C (`GenericStore`, linear scan) | ~7,452x - 7,595x |
| D (Solr, cross-process) | ~7,133x - 7,851x |

C's cost confirms the "accidental Rust-Solr" this project's architecture
deliberately avoids: representation genericity (a per-document field map)
*and* no precomputed index at all compounds into a multi-thousand-x cost
versus a compiled bitmap intersection, even before considering D's
additional cross-process/network cost. D is reported as context only --
Solr's absolute latency includes real HTTP round-trip and JVM-side work
unrelated to the in-process A/B/B2/C comparison, so its ratio is not a
fair "compiled schema vs generic document search" data point the way
B-vs-A is.

### Index/structure size and memory

Approximate in-memory structure size (`approximate_size_bytes`, one
representative run):

| path | bytes | scope |
|---|---|---|
| A | 10,984,302 | ALL of `CatalogIndex`'s structures (brand/category/price/enum attrs/lexical postings/ordinal facets), not just `product_type` -- not a fair single-field comparison |
| B | 818,031 | `product_type` only |
| B2 | 807,699 | `product_type` only |
| C | 1,901,061 | `product_type` only |
| D (Solr) | 25,671,675 (whole core, via admin API) | ALL fields, not just `product_class` -- same scope caveat as A |

Isolated RSS (separate process per path, to avoid one process's combined
memory contaminating a single-path reading; delta from post-catalog-load
baseline):

| path | RSS delta (KB) |
|---|---|
| A | 32,760 (all of `CatalogIndex`) |
| B | 1,840 (`product_type` only) |
| B2 | 1,796 (`product_type` only) |
| C | 27,752 (`product_type` only) |

B/B2's much smaller footprint than A's is expected and not a fair
"compiled schema is cheaper" claim (A includes far more than one field);
C's ~15x larger footprint than B2 for the *same single field* is a real,
disclosable cost of "no compilation at all" -- a `BTreeMap<String,
GenericValue>` cloned per document, with no value deduplication, versus
one bitmap per distinct value.

### What was NOT measured, disclosed rather than fabricated

Cycles/instructions/branch-misses/cache-misses (Issue #38's own "where
practical" metrics) are **not measured**: this environment has no `perf`
binary and `/proc/sys/kernel/perf_event_paranoid` is unreadable (checked
directly before writing the measurement binary). CPU/query is
approximated by single-threaded wall-clock latency (no separate CPU-time
accounting instrumented). QPS/core is measured single-threaded within
the timed loop, not under real concurrent load.

### Quality gate

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo test --workspace --all-features`
(0 failures, including 3 new `issue38-eval` correctness tests + 3 unit
tests), `cargo build --workspace --release` -- all clean.

### I38-E1 verdict

**E1 PASSES**, via the redesigned B2 compiled-schema representation. The
naive B design's failure was real, reproducible, and precisely localized
to an avoidable per-query allocation -- implementation-specific, not a
fundamental cost of compiling a dynamically-discovered schema into the
same physical operators a hard-coded executor uses. Per Issue #38's own
GO criterion ("compiled-schema execution preserves most of the
hard-coded native physical advantage"), this clears the bar. See
`ISSUE38_DECISION.md` for the full decision-gate reasoning and what is
explicitly scoped out of this pass (E2-E5).

## I38-E2: full-pipeline generalization to a genuinely unseen vertical

**Methodology pivot from `ISSUE38_DECISION.md`'s original E2 plan**:
that document scoped E2 as "LLM-assisted feature discovery on a
previously unseen catalog." The user directing this pass instead asked
for deterministic synthetic datasets first (generator, fixed seeds,
schema, ground truth, provenance, validation all committed), with
external-dataset search as a non-blocking parallel check -- explicitly
overriding the original plan. LLM-assisted feature discovery remains
untested and is still open (see "What this does NOT establish" in the
decision doc).

**Hypothesis**: the architecture validated against WANDS (E1, real home
furnishings) generalizes to a catalog from a vertical with a materially
different attribute schema and, for the first time, a genuinely new
*structural relationship pattern* -- not just new vocabulary in an
already-tested shape (single-valued entity/attribute lookup, all E1
tested).

**Dataset** (`crates/issue38-e2e3-eval/src/automotive.rs`, `SEED =
0x38E2_A0A0`): a fully synthetic automotive-parts catalog. Attribute
schema shares nothing with WANDS: `thread_size`, `voltage`,
`cold_cranking_amps`, `heat_range`, `micron_rating`, `bulb_type`,
`lumens`, `filter_type`, `material_grade`, `oem_or_aftermarket` -- none
exist in WANDS. The genuinely new relationship: `compatible_fitment`, a
many-to-many vehicle-compatibility set (7 real make/model pairs x 9
model years), represented via the *already-existing*
`AttributeValue::MultiEnum` + `Constraint::MultiEnumContains` mechanism
-- deliberately not a new `StructuralConstraint` variant, per this
project's "do not recreate Solr/Elasticsearch abstractions" rule and this
crate's own `lib.rs` doc comment. Brand names are fictional; vehicle
make/model names are real (a factual, non-trademark-sensitive descriptive
attribute in real aftermarket-parts commerce, like a shoe size). Ground
truth is computed programmatically from the same generation parameters
that produced the catalog (3-way `Exact`/`Partial`/`Irrelevant`, reusing
`phase9_eval::wands_relevance::WandsLabel`), not hand-labeled.

**A real methodology bug caught before ever running the experiment**
(by direct code read of `crates/commerce-core/src/ir/query.rs`'s
`compile()`, not discovered after a failed run): the fitment value was
originally formatted as a pipe-joined key, `"{make}|{model}|{year}"` (e.g.
`"honda|civic|2015"`). `compile()`'s real phrase-lexicon lookup matches
only an *exact, space-joined* token window (`tokens[i..i+window].join("
")`) against `SemanticLexicon`'s keys -- a pipe-joined key can never
equal any such window, no matter how a shopper phrases the query. This
would have made every fitment query in this experiment compile with
*zero* fitment constraint resolved, silently testing nothing about the
new relationship at all while still reporting a plausible-looking number.
Fixed by reformatting to a natural, space-joined phrase,
`"{year} {make} {model}"` (e.g. `"2015 honda civic"`), matching this
module's own query templates' word order exactly. A dedicated unit test
(`fitment_phrase_is_discoverable_by_the_real_compile_lexicon`,
`crates/issue38-e2e3-eval/src/lib.rs`) now proves, against the real
`compile()`/`compile_lexicon`, that a fitment query resolves both a
`Structural(ProductType(_))` entity constraint and a
`Constraint::MultiEnumContains` fitment constraint from real query text
-- not merely asserted from reading source.

**Setup**: 3,000 products, 10 product types, 57 queries across 7
templates, real production pipeline (`ir::query::compile` ->
`plan::plan` -> `plan::execute_planned`, `PlannerPolicy{selectivity_threshold:
0.05, delegate_oversample: 20}`), `phase9_eval::bitmap_delegate::BitmapTantivyDelegate`
reused unmodified as the lexical delegate. 5 independent runs.

### Results (genuinely byte-identical across all 5 runs for every correctness/routing/taxonomy number; only latency varies) -- CORRECTED TWICE, see below

| metric | value |
|---|---|
| fitment_exact NDCG@10 (n=8) | mean **0.9472**, min 0.7211 |
| aggregate NDCG@10, excluding exact_lookup (n=26) | mean **0.9135** |
| aggregate NDCG@10, all templates (n=57) | mean **0.4167** (see finding below) |
| routing | 35.1% FastPath, 0% Hybrid, 64.9% Punt |
| failure taxonomy | 35.1% entity_resolved_structural, 56.1% no_structural_signal_punt, 8.8% vocabulary_gap_demoted_to_punt |
| candidate-set size | p50 3,000 (full catalog), p10 17.2 |
| latency (execute_planned, p50) | ~0.032ms (single-threaded Tantivy indexing, see second correction below -- materially faster than the earlier multi-threaded figures, not a regression) |

The `vocabulary_gap_no_entity` template (`"ceramic front pads"`, n=2,
mean NDCG 0.58) is a deliberate regression check reproducing P9-E05's
resolution-priority scenario against fully unrelated automotive
vocabulary -- confirms the entity-corroboration demotion rule still
fires correctly on unseen vocabulary, not just on WANDS's own.

### Post-hoc adversarial-review correction (disclosed, not silently replaced)

An adversarial review (per this project's own established discipline,
matching E1's precedent of an adversarial pass catching real
methodology bugs before finalizing) found that `automotive.rs`'s
`fitment_exact` template -- Template 1 -- iterated a **fixed**
`VEHICLES[..4] x [2015, 2018]` candidate list to build its 8 queries,
with nothing guaranteeing any specific (make, model, year) combination
was ever actually assigned to a generated Brake Pads product's
`compatible_fitment` set. The original 3,000-product E2 run happened
not to trigger this (all 8 combinations had a real match by chance), so
it read as a clean result; a new regression test added specifically to
close this gap
(`ground_truth::assert_every_query_of_template_has_an_exact_match`,
wired into `tests::automotive_ground_truth_is_self_consistent`) caught
it concretely at this crate's own smaller test catalog size (120
products), where one fitment query's claimed `Exact` case had in fact
never been generated. **Fixed** by deriving the 8 fitment queries from
real generated Brake Pads products' own actual `compatible_fitment`
values instead (the same "ground truth by construction" discipline
`automotive.rs`'s own part-number template, and now `mixed_merchant.rs`'s
`size_schema_conflict` template, already followed) -- eliminating the
possibility by construction rather than hoping a large enough `N`
avoids it. The pre-correction figures were fitment_exact NDCG@10 mean
**0.9913** (min 0.9306), aggregate excluding exact_lookup mean **0.9271**,
aggregate all-templates mean **~0.422-0.425**; both the corrected and
uncorrected figures support the same positive generalization finding --
the correction changes which 8 concrete fitment queries are asked, not
the underlying conclusion.

The same review also found, and this pass fixed, two related lower-severity
instances of the identical defect class: `apparel.rs`'s
`size_numeric_keyword` template hard-coded `["32", "34"]` directly (now
derived from real generated Jeans products' own actual `size` values),
and both `apparel.rs`'s and `furniture_synth.rs`'s color+type query loops
iterated a type's first two *candidate* colors rather than colors actually
present on generated products of that type (a `Partial` fallback kept
`ground_truth::assert_self_consistent`'s "non-empty judgments" check from
ever catching this, since a same-type-different-color product is always
`Partial` -- only a *specifically-Exact* check, added as part of this
correction, could). These two templates are not currently exercised by
any experiment binary (see `apparel.rs`'s own corrected doc comment); the
fix keeps their generator code correct and matches this crate's
established discipline, since they are still exercised by this crate's
own self-consistency unit tests.

A separate `e3_mixed_category_eval.rs` diagnostic bug (unrelated defect
class, same review) is described in I38-E3's own correction note below.

### Named finding: `exact_lookup` near-zero NDCG is a delegate-scope gap, not a generalization failure

`exact_lookup` (part-number search, `"part number IA-1234-BP"`, 31/57 =
54% of the workload) scores NDCG ~0. Root-caused directly (not
speculated): `part_number` is a *variant*-level `AttributeValue::Text`
attribute, but `phase9_eval::bitmap_delegate::build_index`'s own doc
comment states it indexes only *product*-level `Text` attributes into
its Tantivy fields -- an existing, documented Phase 9 scope decision, not
a new defect found here. `commerce_core::index::CatalogIndex`'s own
internal `lexical_postings` *does* include variant-level text (merged in
via `effective_attributes`), but `plan::execute_planned`'s `Hybrid`/
`Punt` outcomes never consult it at all -- they rank purely via whatever
external `LexicalDelegate` is wired in (confirmed by direct read of
`plan/mod.rs`). This is disclosed as a real schema-management-relevant
finding: a production `LexicalDelegate` implementation must explicitly
decide whether to index variant-level identifiers (SKU/part number
commonly vary per variant in real catalogs), which the reused reference
delegate today does not. Not patched here -- `bitmap_delegate.rs` is
shared, already-validated Phase 9 infra, out of E2's scope to modify.
Filed as GitHub Issue #41.
Reported separately (both an "excluding exact_lookup" aggregate and the
per-template breakdown) so it cannot silently read as evidence against
the fitment/schema-generalization question E2 exists to answer.

### I38-E2 verdict

The architecture generalizes cleanly to this unseen vertical, including
its genuinely new structural relationship (fitment NDCG 0.9472, no
production code changes) -- a positive generalization result. The
disclosed `exact_lookup` finding is a real, useful, but *distinct*
finding about lexical-delegate scope, not a mark against generalization.

## I38-E3: mixed-category merchant catalog, schema-management diagnostic

**Methodology pivot** (same as E2 above): the original decision doc
scoped E3 as combining WANDS with a different vertical. The user's
governing instruction for this pass asked instead for a realistic
mixed-category *merchant* catalog built from scratch: several product
families with incompatible schemas, shared ambiguous fields, sparse
attributes, noisy titles, and cross-category queries -- explicitly
including "cases that test whether ingestion can decide which features
deserve bitmap indexing without requiring search-time category
intelligence."

**Dataset** (`crates/issue38-e2e3-eval/src/mixed_merchant.rs`, `SEED =
0x38E3_C0DE`): furniture (self-contained synthetic, deliberately noisy
titles -- promotional junk, typos, inconsistent casing) + apparel +
automotive, 1,000 products each, ingested as **one undifferentiated
catalog** (3,000 products, 18 product types). Shared ambiguous field
*names* across families with different vocabularies: `color`
(furniture/apparel only -- automotive has none, a real sparse-field
case), `material` (furniture/apparel/automotive all define it, "Leather"
genuinely overlaps furniture and apparel), and the deliberate schema
conflict: `size` is always `AttributeValue::Enum` in apparel (`"34"` for
jeans waist) and always `AttributeValue::Numeric` in automotive Wiper
Blades -- arising naturally from realistic per-family modeling choices
(a wiper blade's size is a measured length in inches; a jeans size is a
catalog label), not an injected fixture. `material` is sparse (~15-20%
missing per family).

### Schema-management diagnostic: catalog-agnostic ingestion, confirmed by direct measurement

`crates/issue38-e2e3-eval/src/ingest.rs`'s `build_catalog` never reads
`SynthProduct::family` (confirmed by grep, not merely by convention) --
the family tag exists purely for this crate's own reporting. Measured
against the real pipeline: 18 product types and 15 brands discovered
purely from the ingested `Catalog` + registries; `CatalogIndex::build`
(`crates/commerce-core/src/index/mod.rs`) bitmap-indexes every
`Enum`/`MultiEnum`/`Boolean` attribute value and sorts every `Numeric`
value purely by that value's own `AttributeValue` variant tag -- there is
no per-family or per-category conditional anywhere in that function.
Ingestion's schema decisions are, verifiably, type-tag-driven and
catalog-agnostic, directly answering the question this experiment was
asked to test.

### Finding 1 (the size schema conflict), measured directly

`lexicon.resolve(<jeans_anchor>)` returns **exactly 1 candidate**
(apparel's `Enum` source only), where `<jeans_anchor>` is this run's
actual apparel-jeans size value (`"34"` at this `SEED`/`N_PER_FAMILY`),
derived from a real generated Jeans product via
`mixed_merchant::size_conflict_anchors` -- **not** a hard-coded literal
(see the correction note after Finding 1 below). Root cause, verified by
direct read of
`crates/commerce-core/src/cold_start/profile.rs`: `Numeric` values are
profiled into a completely separate `numeric_values` map that
`compile_lexicon` never reads at all -- automotive's Numeric `size`
values never reach the lexicon, so there is no "ambiguous candidate list"
for the profiler to arbitrate. The actual conflict lives entirely inside
`ir::query::compile`'s hard-coded `"size N"` keyword branch
(`crates/commerce-core/src/ir/query.rs`), which (a) never consults the
lexicon for this token pattern at all, and (b) writes directly into
`result.constraints` rather than the `lexicon_attribute_matches` list
P9-E05's entity-corroboration demotion rule inspects -- so a bare
`"size N"` query is *not* demoted to a preference the way an equivalent
bare lexicon-derived attribute match would be. `Constraint::matches`'s
own `_ => false` catch-all (`crates/commerce-core/src/domain/constraint.rs`)
still makes this safe -- a `Numeric` constraint checked against an
`Enum`-valued attribute always safely returns `false`, never a
false-positive cross-type match.

Measured consequence on `size_schema_conflict` queries: ground truth
spans both families (apparel: 64 relevant variants, automotive: 10), but
returned hits are **100% automotive, 0% apparel** (0/64 = 0.0% apparel-side
recall, 10/10 = 100% automotive-side recall) -- reproduced identically
across 5 runs. A real, disclosed **recall gap**, not a correctness
violation. Filed as a scoped design question (should `compile()`'s
numeric keyword branches consult the lexicon and participate in the
entity-corroboration demotion rule before becoming hard filters?) for a
future dedicated design cycle, per the P9-E05 precedent of not rushing an
undermotivated heuristic patch into `compile()`'s resolution algorithm.
Filed as GitHub Issue #40.

**Post-hoc adversarial-review correction (disclosed, not silently
replaced)**: `e3_mixed_category_eval.rs`'s own schema-management
diagnostic originally queried `lexicon.resolve("34")` with a **hard-coded**
literal, completely decoupled from whatever value the
`size_schema_conflict` workload's own RNG-derived anchor (in
`mixed_merchant.rs`) actually was. The two happened to agree at this
crate's current `(SEED, N_PER_FAMILY)` -- nothing enforced that
agreement, and the coincidence was asserted above as a "measured fact"
before an adversarial review caught it (the exact same defect class as
the fitment-query bug described in I38-E2's own correction note, just
in a diagnostic print rather than a query template). **Fixed** by adding
`mixed_merchant::size_conflict_anchors`, a single public function both
the workload builder and the measurement binary now call, so the
diagnostic can never diverge from the workload again. The underlying
finding (exactly 1 lexicon candidate) is unchanged; only its provenance
is now trustworthy.

### Finding 2 (residual-lexical veto), found by measurement, not anticipated

While building this experiment's own "clean per-family control query"
template, direct measurement (not code reading) surfaced a second,
distinct finding: `plan::execute_planned`'s `Hybrid`/`Punt` outcomes let
the lexical delegate's *residual* free-text term veto an otherwise
well-formed, correctly-narrowed structural query. The original template
prepended a family label to the product-type phrase (e.g. `"furniture
sofas"`); `"furniture"` and `"automotive"` appear in **zero** generated
titles, so the delegate returns zero raw hits and the *entire query*
returns nothing -- even though the `ProductType` structural constraint
alone identifies hundreds of correct candidates already sitting in the
index (`execute_planned` never falls back to the structural candidate
set when the delegate itself returns nothing). `"apparel sneakers"`
appeared to work, but only by coincidence: one of this crate's own
apparel brand names, `"Cascade Apparel"`, literally contains the word
"apparel" -- kept deliberately (not renamed away), since a brand name
containing its own category word is a realistic real-catalog pattern,
and its presence is exactly what isolates the effect as a real recall
veto rather than "every residual term always fails."

This reads as stronger than `plan/mod.rs`'s own doc comment implies ("the
delegate ranks free text only within that narrowed set" suggests a
ranking signal, not an additional hard filter). Measured:
`residual_veto_probe` mean NDCG@10 = **0.3333** (n=3: 0, 0, 1.0). The
template was split in two once this was found: `same_catalog_control`
(bare product-type phrase, confound removed) now scores a clean mean
NDCG@10 = **1.0000** (n=3, all `FastPath`) -- both reproduced identically
across 5 runs. Filed as a second scoped design question, not patched
here (this is production `plan`/`execute_planned` behavior; changing it
needs its own dedicated design cycle per this project's established
discipline, not a same-pass heuristic fix). Filed as GitHub Issue #40
(alongside Finding 1 above -- both are resolution-safety-net gaps in the
same two functions).

### Full results (byte-identical across all 5 runs for every correctness/routing/diagnostic number)

| metric | value |
|---|---|
| aggregate NDCG@10 (n=11) | mean 0.7273 |
| cross_category_bare_attribute (bare color, "leather black") | mean NDCG@10 = 1.0000 (n=3, all Punt) -- correctly spans every matching family, no hidden single-family bias |
| same_catalog_control (clean) | mean NDCG@10 = 1.0000 (n=3, all FastPath) |
| residual_veto_probe | mean NDCG@10 = 0.3333 (n=3, hybrid:1/punt:2) -- see Finding 2 |
| size_schema_conflict | mean NDCG@10 = 0.5000 (n=2, both FastPath) -- see Finding 1 |
| routing | 45.5% FastPath, 9.1% Hybrid, 45.5% Punt |
| candidate-set size | p50 250 |
| latency (execute_planned, p50) | ~0.019ms (single-threaded Tantivy indexing, see second correction below) |

### I38-E3 verdict

Ingestion's schema-management decisions are confirmed, by direct
measurement, to be catalog-agnostic and type-tag-driven -- the positive
result the experiment was asked to establish. Both named findings (the
size recall gap, the residual-lexical veto) are real, safe (never a
false-positive hard filter or wrong result), disclosed, and scoped as
design questions for a future dedicated cycle rather than patched in this
pass, consistent with this project's established discipline (P9-E05) of
not rushing heuristic changes into `compile()`/`plan()`'s resolution
logic without one.

## Adversarial review summary

An independent adversarial review (background agent, per this project's
Ultracode/E1 precedent of catching real methodology bugs before
finalizing) examined every ground-truth judge closure, every query
template's derivation, the two experiment binaries' claims against the
production code they measure, and the latency methodology. It found and
this pass fixed three real issues (the E2 fitment-query and E3
diagnostic-decoupling bugs detailed in their own correction notes above,
plus `apparel.rs`/`furniture_synth.rs`'s lower-severity instance of the
same defect class), and flagged one minor, latent, already-documented
ordering choice in `failure_taxonomy::classify` (ambiguity-disclosure
priority over routing-success classification, clarified with a comment,
no behavior change -- it never manifested in either real run). Every
ground-truth judge's semantics, the crate's determinism (confirmed:
`AttributeMap` is a `BTreeMap`, not a `HashMap`, so there is no hidden
iteration-order non-determinism), and the latency methodology (single-call,
post-warmup, millisecond-scale -- correctly not needing E1's
batching/`black_box` treatment) were verified clean.

## Second correction round: Issue #42's pre-merge independent review

Issue #42 established a stricter governing rule for this repository
("do not trust the experiment author") and required an independent
review of PR #39 before it could be merged and frozen as the E1-E3
baseline. That review found three further real issues, all confirmed by
direct reproduction (not accepted on the reviewer's word alone) before
being fixed:

1. **`failure_taxonomy::classify` misclassified successfully-routed,
   non-entity hard constraints as "demoted to Punt."** The function's
   final `else` branch keyed only on `has_entity == false` and
   `constraints`/`preferences` non-emptiness, never checking the actual
   routing `outcome` the way the `has_entity` branch above it does. A
   query resolving a non-entity hard constraint (e.g. `compile()`'s
   `"size N"` keyword branch, which never goes through the P9-E05
   demotion path at all) and routing `FastPath` was bucketed under a
   class literally named "demoted to Punt" -- reproduced directly: an E3
   rerun showed `outcome fast_path: 5` alongside
   `vocabulary_gap_demoted_to_punt: 5` in the same printout, silently
   folding 3 genuine Punts together with 2 unrelated FastPath queries.
   **Fixed** by adding two new classes,
   `NonEntityConstraintResolved`/`NonEntityConstraintPunted`, checked
   before the demotion-path branch; a fresh unit test in
   `failure_taxonomy.rs` proves the corrected behavior directly (not
   only via a rerun). E3's corrected breakdown now reads
   `non_entity_constraint_resolved: 2 (18.2%)` /
   `vocabulary_gap_demoted_to_punt: 3 (27.3%)`, consistent with its own
   routing table for the first time.
2. **`automotive.rs`'s remaining `attribute_plus_entity` sub-templates**
   (material_grade+position, thread_size, oem/aftermarket) shared the
   same "fixed candidate list, no generation guarantee" defect class
   already found and fixed once in this crate's other templates --
   inconsistently left unfixed here, though it happened to pass at this
   crate's own smaller test catalog size (verified by adding the same
   `assert_every_query_of_template_has_an_exact_match` check used
   elsewhere: it passed at n=120, meaning this specific instance was not
   currently producing an ungrounded case, but nothing guaranteed that).
   **Fixed** the same way as the other templates, deriving from real
   generated products.
3. **Non-deterministic Tantivy indexing** (found by this session's own
   continued verification, not by the reviewer): rerunning E2 five times
   and diffing raw output byte-for-byte -- rather than only comparing
   summary statistics as earlier verification passes had done -- found
   `vocabulary_gap_no_entity`'s mean NDCG genuinely differing between
   runs (0.5386 vs. 0.5869) against the identical seeded catalog. Root
   cause: `phase9_eval::bitmap_delegate::build_index` used Tantivy's
   default multi-threaded indexing (`index.writer(...)`, `min(num_cpus,
   8)` threads); Tantivy's own source states plainly that only
   single-threaded indexing gives a deterministic DocId allocation, and
   `TopDocs`' tie-breaking among equally-scored documents can depend on
   that allocation. This directly contradicted this log's own repeated
   "byte-identical across 5 runs" claim. **Fixed** in the shared
   `bitmap_delegate.rs` (single-threaded indexing -- costs nothing
   measurable for catalogs this size, and was measurably *faster* here
   with no thread-coordination overhead). Verified: reran E2 and E3 five
   times each after the fix and diffed every run byte-for-byte; every
   correctness/routing/taxonomy number is now genuinely identical.
   **Scope note**: this fix covers Issue #38's own E2/E3 binaries only.
   Phase 9's already-published results (P9-E01 through P9-E06) were
   measured against the unfixed, multi-threaded version of this same
   shared module and have **not** been re-audited here -- filed as
   GitHub Issue #43, not silently left unmentioned.

Both the pre- and post-this-round figures are named explicitly, per this
project's own established discipline: fitment_exact NDCG@10 was 0.9913
(min 0.9306) after the *first* correction round, then 0.9472 (min
0.7211) after automotive's other templates were also grounded in this
round -- unchanged again by the determinism fix, since `fitment_exact`
never depends on Tantivy tie-breaking (routes via `MultiEnumContains`,
not free-text search). The aggregate-excluding-exact_lookup figure moved
from a range (0.9134-0.9183 across runs, itself already a symptom of the
determinism bug) to a single exact value, 0.9135, now that the
non-determinism is fixed. Every number in the Results tables above is
this round's final, corrected figure.

## E2/E3 quality gate

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `cargo test --workspace --all-features`
(0 failures, including 6 `issue38-e2e3-eval` tests: determinism across
every family generator, ground-truth self-consistency for all four
workloads -- strengthened post-adversarial-review with
`assert_every_query_of_template_has_an_exact_match` checks that would
have caught the fitment-query and color/size-literal bugs directly --
and the fitment-phrase-discoverability proof), `cargo build --workspace
--release` -- all clean.

## External-validity check (non-blocking, per the governing instruction)

A background research pass searched for a license-compatible, reachable
public commerce dataset from a vertical materially different from
furniture, to use only as an external-validity check -- explicitly not
allowed to block the synthetic E2/E3 work above, and explicitly not to be
overstated as "real-world validation" if used. Findings, disclosed in
full rather than silently resolved:

- This sandbox can only reach `github.com` and its raw-content/API
  subdomains; Kaggle, HuggingFace, Zenodo, and static Open Food Facts
  hosts all return 403.
- The one dataset with real, non-synthetic content confirmed
  byte-fetchable right now (`raw.githubusercontent.com/SayamAlt/E-Commerce-Text-Classification/main/ecommerceDataset.csv`,
  50,425 rows, Electronics/Clothing & Accessories/Books/Household labels)
  has an **unconfirmed license** -- its origin (a Zenodo record) could
  not be reached from this sandbox to check. Per the governing
  instruction's explicit preference for license-compatible sources, this
  is flagged rather than used.
- Open Food Facts (grocery/CPG) has the clearest permissive license
  (ODbL + CC-BY-SA) and the best vertical distance from furniture, but
  both its primary host and its HuggingFace mirror are unreachable from
  this sandbox.
- Home Depot's product-search-relevance dataset (the closest structural
  match to WANDS -- real human relevance judgments) is Kaggle-only,
  competition-rules-licensed, and unreachable; no GitHub mirror with real
  committed data was found (10 candidate "solution" repos checked, all
  require the user's own Kaggle download).

**No external-validity check was performed.** This is disclosed
explicitly rather than silently substituted with synthetic evidence:
every E2/E3 result above is synthetic, and remains synthetic evidence
only, not real-world validation. Resolving this (either by re-attempting
the unconfirmed-license candidate from an environment that can verify
its license, or by fetching Open Food Facts from an unblocked network)
is named as follow-up work, not done here.
