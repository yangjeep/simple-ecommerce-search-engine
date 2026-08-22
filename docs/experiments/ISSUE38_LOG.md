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
