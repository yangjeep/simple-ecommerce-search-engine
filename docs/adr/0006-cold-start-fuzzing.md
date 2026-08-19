# ADR 0006: Cold-start catalog profiling and shopper-query fuzzing

## Status

Accepted (Gate 6, Issue #2).

## Context

Gate 6 asks for, given a catalog fixture: profile/compress semantic
problems; infer or fixture candidate mappings; generate shopper-like query
cases; replay them against the compiled context; identify semantic
coverage holes — explicitly "do not perform one LLM call per SKU." This
follows directly from the open thread in E004/E005: both flagged that
coverage had only ever been measured against a hand-authored query set
built from the same lexicon being measured, and Gate 6 is the natural way
to get an independent data point.

## Decision

- **`cold_start::CatalogProfile::build` makes zero model/LLM calls and one
  pass over the catalog.** It reads `Brand`/`ProductType`/`Category` name
  registries (supplied separately, since `Catalog` itself only stores
  typed IDs, not names — matching the existing domain model) plus every
  product/variant's attributes, and produces deduplicated collections:
  distinct lowercase brand/product-type/category names, distinct boolean
  attribute names seen `true`, and — the interesting case — a map from
  each distinct *lowercase* enum/multi-enum value string to the set of
  `(attribute, original-cased value, is_multi)` triples that produced it.
  This is the "compress" half of "profile/compress semantic problems":
  `cold_start_catalog`'s 4 products / 7 variants collapse to 14 distinct
  values (`tests/cold_start.rs::profile_compresses_ten_variants_into_a_small_distinct_vocabulary`).
- **A value seen under more than one attribute becomes a genuinely
  ambiguous lexicon entry, not a silently-picked one.**
  `cold_start::compile_lexicon` builds one `Candidate` per source and
  inserts them all under the shared lowercase key;
  `ir::SemanticLexicon`/`ir::query::compile`'s existing "multiple
  candidates = ambiguous" rule (ADR 0002) does the rest with no new
  mechanism. `fixtures::cold_start_catalog` was deliberately built to
  contain exactly one such collision — "green" is both a `color` (Nike
  hiking boot) and a `features` tag meaning eco-friendly material
  (Aerowalk hiking boot) — so the profiler's ambiguity path is exercised
  by real (if fixture) data, not just asserted to exist in principle.
- **Every profiler-derived mapping is a hard `Constraint` candidate, never
  an `ir::Preference`.** The hand-curated `fixtures::shoe_lexicon` (Gate
  2/4) chose to make "cushioned"/"breathable" soft preferences because a
  human curator judged them descriptive rather than decisive. The
  profiler has no such judgment available from catalog data alone — a
  MultiEnum value's "hardness" isn't recoverable structurally — so it
  defaults to hard constraints uniformly. This is a known, stated
  limitation, not an attempt to reproduce the hand-curated lexicon's
  judgment calls automatically.
- **Query generation is template x profile-vocabulary, not random.** Five
  fixed templates (`"{brand} {product_type}"`, `"{value}
  {product_type}"`, `"{boolean_attr} {product_type}"`, `"{product_type}
  size {n}"`, `"{product_type} under ${threshold}"`) are instantiated
  against every product type crossed with the profile's own brands,
  values, boolean attributes, and observed sizes, plus one price
  threshold just above the catalog's most expensive item (guarantees at
  least one real match exists). Iteration is over `BTreeMap`/`BTreeSet`
  keys, so output is byte-identical across runs
  (`tests/cold_start.rs::generated_queries_are_deterministic_across_runs`)
  — required by `docs/EXPERIMENT_LOOP.md`'s "keep benchmark inputs
  deterministic and versioned," and necessary because this is exactly the
  kind of input a random generator would make hard to reproduce or
  debug.
- **`coverage_holes` reuses `ir::compile` and the existing
  ambiguous/residual classification** rather than a new evaluator — a
  "hole" is precisely a generated query that is not fully resolved,
  exactly the definition `ir::coverage::measure_coverage` already uses.

## Consequences

- **Self-consistency result: 28/30 (93.3%) of catalog-derived queries
  fully resolve against the catalog-derived lexicon**; the 2 holes are
  exactly the two product-type variants of the deliberate "green"
  collision (`tests/cold_start.rs::coverage_holes_are_exactly_the_deliberate_green_collision`).
  This is expected to be high by construction (queries are built from the
  lexicon's own vocabulary) — the number that matters is that the *only*
  holes are the deliberately-planted collision, meaning the mechanism
  finds real semantic problems without also manufacturing spurious ones.
- **Cross-lexicon result (measured, not predicted — see
  `docs/experiments/LOG.md` E006): the catalog-derived lexicon resolves
  11/20 (55%) of Gate 4/5's hand-authored `REPRESENTATIVE_QUERY_SET`**,
  versus the hand-curated lexicon's 12/20 (60%, E004) on the same set —
  close, from two lexicons built by entirely different processes with
  only partial vocabulary overlap. Neither number is "better"; they
  measure different things (hand judgment vs. structural derivation) and
  are reported as a comparison point, not a competition.
- The two lexicons are provably not equivalent
  (`tests/cold_start.rs::hand_curated_and_catalog_derived_lexicons_are_independently_comparable`
  asserts `CoverageReport` inequality on the same query set), confirming
  the cross-check is measuring something real rather than two paths that
  happen to converge.
- `CatalogProfile`/`compile_lexicon` do not yet feed into
  `control_plane`'s propose/replay/promote loop (Gate 5) — that
  integration (a `ModelProvider` backed by catalog profiling instead of a
  fixed table) is a natural next step but is not built here, to keep this
  gate's surface to profiling + generation + hole-finding as scoped.

## Alternatives considered

- **Randomly sample attribute-value combinations to generate queries**
  (e.g. `rand` with a fixed seed, as the Gate 0/3 performance benches
  do). Rejected: this is a relevance/coverage fixture, not a performance
  one, and `docs/EXPERIMENT_LOOP.md` explicitly separates "synthetic
  expansion...for performance scaling" from relevance claims. A fixed,
  exhaustive template x vocabulary cross-product is also easier to reason
  about when a hole appears — the failure is attributable to a specific
  (template, value) pair, not "one of many possible random draws."
- **Have the profiler infer soft vs. hard automatically** (e.g. treat
  `MultiEnum` values as preferences, `Enum` values as constraints).
  Rejected for now: this fixture's `features` MultiEnum contains both
  clearly-hard tags (implicitly, none in this catalog, but plausible in
  general) and clearly-soft ones; a syntactic rule (attribute *kind*
  implies hard/soft) would be asserting a judgment the catalog data
  doesn't actually support. Left as a stated limitation rather than a
  plausible-looking heuristic with no evidence behind it.
