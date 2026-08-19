# ADR 0002: Commerce IR shape and Gate 2 compiler prototype

## Status

Accepted (Gate 2, Issue #2).

## Context

Gate 2 requires that queries compile into typed constraints/preferences,
with explicit ambiguity/confidence rather than silently flattening
everything into lexical terms, and must handle the representative query
`black Nike waterproof running shoes size 9 under $150`. Gate 1
(`docs/experiments/LOG.md` E001) already established a variant-safe
attribute matcher (`domain::Constraint` over a per-variant merged
`AttributeMap`), but three things that query needs are not attributes at
all: brand, product type, and price are already typed fields on
`Product`/`Variant`.

## Decision

- **Two constraint kinds, one execution path.** `ir::StructuralConstraint`
  covers brand/product-type/category/price — fields already typed on
  `Product`/`Variant` — while `domain::Constraint` continues to cover the
  generic attribute map. `ir::ResolvedConstraint` wraps both, and
  `CommerceQuery::execute` evaluates every constraint against one
  variant's combined view (product attributes merged with variant
  attributes via the existing `effective_attributes`, plus the variant's
  own structural fields) in the same loop `Catalog::search` uses. This
  means mixing structural and attribute constraints cannot reopen the
  cross-variant matching bug Gate 1 closed — enforced by
  `representative_query_does_not_cross_variant_match` in
  `tests/ir_compiler.rs`, which compiles the representative query and
  proves it matches nothing in the Gate 1 fixture (black is size 8 there,
  not size 9).
- **Ambiguity is a first-class output, not an error or a guess.**
  `SemanticLexicon::insert` accepts a `Vec<Candidate>` per phrase; when a
  phrase resolves to exactly one candidate it becomes a constraint or
  preference, but when it resolves to more than one, the compiler emits an
  `AmbiguousSpan` carrying every candidate and its confidence instead of
  picking the highest-confidence one. This is a direct reading of
  CLAUDE.md's "preserve ambiguity explicitly when confidence is
  insufficient" — the compiler does not compute a single winning
  confidence threshold at all; multiplicity of candidates *is* the
  ambiguity signal. `tests/ir_compiler.rs::ambiguous_term_is_preserved_not_silently_resolved`
  exercises this against a deliberately double-mapped fixture term
  ("leather" -> material text match OR a feature tag).
- **Unrecognized terms become `residual_lexical`, not silent drops.** A
  token with no lexicon entry (e.g. an unmodeled brand name) is kept
  verbatim on the compiled query rather than discarded, so a later lexical
  index (Gate 3) has something to search against — CLAUDE.md: "lexical
  retrieval handles residual uncertainty".
- **Preferences exist but are not consumed yet.** Descriptive terms
  ("cushioned", "breathable") compile to `ir::Preference::Boost` rather
  than hard constraints, because requiring them would wrongly exclude
  matching products that just weren't tagged with that feature. Nothing
  currently reads `CommerceQuery::preferences` — ranking is explicitly
  Gate 3 scope ("dense ranking feature arrays"), so consuming preferences
  now would be building ahead of the gate that needs them.
- **The lexicon is a hand-built prototype, not the Gate 4 FIB.** `ir::SemanticLexicon`
  is an in-memory `BTreeMap<String, Vec<Candidate>>` populated by
  `fixtures::shoe_lexicon()`, with no version number and no promotion
  workflow. It intentionally does not satisfy Gate 4 ("versioned compiled
  semantic context") — it exists only to prove the compiler's shape
  (constraints/preferences/ambiguity/residual) against a concrete,
  falsifiable query before building a compiled/versioned artifact around
  it.
- **Phrase matching is a bounded greedy longest-match**, trying windows
  from `lexicon.max_phrase_words()` down to 1 at each token position (so
  `"running shoes"` resolves as one structural constraint, not two
  unrelated single-word misses), plus two hand-written numeric patterns
  (`size <n>`, `under/over [$]<n>`) checked before phrase lookup, since
  numbers cannot be enumerated in a fixed lexicon. This is a prototype
  tokenizer, not a general parser — no boolean operators, no negation, no
  handling of numbers written as words.

## Consequences

- `ir` depends on `domain`; `domain` has no dependency on `ir`. Adding
  Gate 3's physical indexes should not require `domain::Constraint` or
  `Catalog` to know anything about query compilation.
- The representative query's own catalog fixture
  (`fixtures::representative_query_catalog`) is new and separate from
  `fixtures::variant_safety_catalog`, so Gate 1's adversarial fixture
  (black size-8 / red size-9, on purpose) is not silently made to also
  "work" by adding a black-size-9 variant to it, which would have erased
  the negative-control property that fixture exists to test.
- `CommerceQuery::execute` is the same `O(products × variants)` linear
  scan as `Catalog::search`; no index exists yet. Fine at fixture and
  benchmark scale (Gate 0's 5k-product bench), not a claim about larger
  scale-ladder tiers.

## Alternatives considered

- **Auto-resolve ambiguous phrases to the highest-confidence candidate.**
  Rejected: this is exactly the "silently flattening" behavior Gate 2
  forbids, and it would hide real semantic gaps that Gate 5/6's
  control-plane loop is supposed to resolve through replay evidence, not
  a hardcoded tiebreak.
- **Model every query term as an attribute `Constraint`, including
  brand/product-type/price.** Rejected: brand/product-type/category are
  already `Product` fields with dedicated typed IDs (Gate 1), and price is
  a `Variant` field; re-expressing them as attribute-map entries would
  duplicate state and reopen the "everything is a generic
  document/attribute" failure mode CLAUDE.md warns against.
