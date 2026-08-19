# ADR 0004: Versioned semantic context + structural coverage measurement

## Status

Accepted (Gate 4, Issue #2).

## Context

Gate 4 asks for two distinct things: (1) "a versioned compiled semantic
context supporting aliases/canonical IDs and deterministic query
resolution," and (2) a measurement of "what fraction of a representative
ecommerce query set resolves without model inference." CLAUDE.md's
priority order ranks "structural coverage" (priority 2) above "physical
advantage" (priority 3, already given an initial answer in Gate 3/E003),
so this measurement was picked as the next highest-value unresolved
question over repeating/extending the Gate 3 benchmark.

## Decision

- **`ir::SemanticContext` is a thin, immutable wrapper**: `{ version: u32,
  source: &'static str, lexicon: SemanticLexicon }`, built once via `new`
  with no mutation API. This is deliberately *not* a bigger rewrite of
  `SemanticLexicon` itself — Gate 4 only asks for version/provenance
  metadata around deterministic resolution, which the Gate 2 lexicon
  already provides. Promotion/replay workflows (Gate 5) will need to
  compare two `SemanticContext` values by version; nothing about that
  requires the lexicon's internal representation to change yet.
- **Aliases are lexicon entries pointing at the same canonical
  `ResolvedConstraint`, not a separate alias table.** `"sneakers"` and
  `"trainers"` both resolve to `StructuralConstraint::ProductType(ProductTypeId(1))`
  — the same value `"running shoes"` resolves to — at confidence 0.9
  rather than the canonical phrase's 1.0. Confidence is recorded but,
  consistent with ADR 0002's ambiguity rule, does not change whether the
  term resolves: a single candidate (however lower-confidence) still
  resolves deterministically. `tests/coverage.rs::aliases_resolve_to_the_same_canonical_id_as_the_canonical_phrase`
  asserts `compile("sneakers", ...) == compile("running shoes", ...)`
  constraint-for-constraint. A dedicated alias-table type was considered
  and rejected: it would duplicate `SemanticLexicon`'s
  phrase-to-candidate machinery for no behavioral difference.
- **The "representative ecommerce query set" is a hand-authored, 20-query
  fixture with a known-exact expected outcome per query**, not a random
  or informally-eyeballed sample. `fixtures::REPRESENTATIVE_QUERY_SET` is
  split by construction into 12 queries using only vocabulary the lexicon
  resolves (including the alias path and a stopword/reordering stress
  case), 2 queries containing the deliberately ambiguous "leather" term,
  and 6 queries containing at least one token absent from the lexicon
  entirely (unmodeled brand, unmodeled color, informal multi-word
  phrasing). This makes `tests/coverage.rs` assert *exact* counts
  (12/2/6, fraction 0.6) rather than a vague threshold — the fixture's
  doc comment documents the classification so the test doubles as a
  worked proof that `measure_coverage` classifies queries the way a human
  reading them would expect.
- **`measure_coverage` classifies "fully resolved" as zero ambiguity AND
  zero residual**, matching Gate 4's literal wording ("resolves without
  model inference"): a query with only a soft `Preference` and hard
  constraints still counts as fully resolved (nothing was left
  unmodeled), but any ambiguous span or residual token means a human or a
  future control-plane model would need to intervene.

## Consequences

- **Measured result: 60% (12/20) of the constructed representative query
  set resolves fully deterministically** against a 9-entry hand-curated
  lexicon covering one brand, two colors, one product type (plus two
  synonyms), two boolean/preference attributes, and the numeric/price
  patterns. This is the first quantitative answer to CLAUDE.md's
  "structural coverage" priority-2 question, but it measures the
  lexicon's own hand-picked vocabulary, not a catalog-independent
  ecommerce query distribution — the 60% number describes this fixture,
  not a general claim. See `docs/experiments/LOG.md` E004 for the full
  interpretation and its limits.
- The alias mechanism generalizes directly to Gate 6's cold-start
  profiling: a catalog-derived lexicon builder would populate the same
  `SemanticLexicon` structure `shoe_lexicon` populates by hand, and
  `measure_coverage` needs no changes to evaluate it.
- `SemanticContext.source` is a free-text provenance string today, not a
  structured build record; Gate 5's promotion workflow will likely need
  more (e.g. build timestamp, catalog snapshot id, replay pass/fail
  counts) — deferred until Gate 5 actually needs it, per CLAUDE.md's
  guidance against designing for hypothetical future requirements.

## Alternatives considered

- **Auto-generate the representative query set from catalog data**
  (product titles, attribute value combinations). Rejected for this gate:
  Gate 6 explicitly owns "generate shopper-like query cases" from catalog
  profiling; hand-authoring a small, exactly-labeled set now gives an
  honest, verifiable baseline to compare Gate 6's generated set against
  later, rather than conflating the two gates' evidence.
- **Threshold-style test** (`assert!(fraction > 0.5)`) instead of exact
  counts. Rejected: a threshold test would still pass if the classifier
  silently mis-resolved individual queries as long as the aggregate
  cleared the bar, which is exactly the kind of quiet correctness drift
  CLAUDE.md's benchmark rules warn against ("never improve benchmark
  numbers by weakening correctness"). Exact counts make any behavior
  change in `compile`/`measure_coverage` a visible, explained test
  failure.
