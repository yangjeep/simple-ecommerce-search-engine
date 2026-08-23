# Issue #42 Preregistered Protocol — R1, R2, R3, E2b

**Committed before any treatment implementation or production behavior
change.** This document is itself the deliverable Issue #42 rule 1
requires: hypotheses, competing treatments, datasets, query templates,
calibration/test separation, metrics, thresholds, stop conditions, and
known confounders, all fixed in advance. Nothing in the "Metrics and
gate" sections of R1/R2/R3/E2b below may be adjusted after treatment
results are seen. Any change made after that point must be recorded as a
deviation with a reason, not silently edited in.

## Governing rule: do not trust the experiment author

Per Issue #42 verbatim: Claude/LLM output, fixture generators, benchmark
code, documentation, and conclusions are all untrusted until
independently checked. Concretely, in this protocol:

- The **ground-truth oracle** (`issue42-eval::oracle`) is a structurally
  separate module from every catalog generator it evaluates. It
  re-derives relevance directly from `commerce_core::domain::{Product,
  Variant, AttributeValue}` fields on the actually-materialized catalog
  for the current run — it never reads a generator's own
  `generate_workload`/judgment map, and never imports
  `issue38_e2e3_eval`'s per-family `attr_str`/`attr_numeric` helper
  closures (a fresh, independent reimplementation, even though the logic
  is simple — the point is that a bug shared between generator and
  evaluator cannot hide).
- Every "positive" (claimed exact/relevant match) query in every workload
  below is checked, at run time, against the *actually generated*
  catalog for that run — not merely asserted in a doc comment. Every
  "negative"/adversarial query is checked to confirm no product satisfies
  the forbidden interpretation. Both checks are regression tests, not
  informal review (Issue #42 rule 4).
- RED-before-GREEN (rule 6) applies to production changes: before
  `commerce_core::ir::query::compile` or `commerce_core::plan::execute_planned`
  is modified, a test demonstrating the current defect must fail against
  today's code, then pass after the fix. It does not apply to the new
  experimental treatment implementations themselves (explicitly exempted
  by rule 6's own parenthetical) — those are new isolated code with no
  prior "current behavior" to be RED against.
- No threshold in this document may be tuned after seeing R1/R2/R3/E2b
  results (rule 7). Where this protocol needs a calibration pass (e.g.
  R3's identifier-classifier feature-statistic cutoffs), the calibration
  set and the held-out evaluation set are named explicitly and disjoint
  *in this document*, before any treatment runs.
- Every methodology defect a later adversarial review confirms gets a
  regression test, and both the pre-correction and corrected numbers are
  retained and labeled (rule 9) — the same discipline already
  established across E1/E2/E3 (`docs/experiments/ISSUE38_LOG.md`'s
  correction sections) and continued here without exception.

## Phase 0 — frozen baseline

**`baseline_sha = fe2e52e0fe872a0f4ab86c63ccc839e61de8f3e6`** — the merge
commit of PR #39 into `claude/issue-34-phase9-defect-fixes-wands` (Issue
#38's own base branch), merged after PR #39's CI ran fully green on its
final commit (`449c22f`, this session's second independent-review
correction round) and no unresolved correctness/methodology issue
remained. Recorded here and in `ISSUE38_DECISION.md`'s baseline note; every
manifest produced from this point forward must cite this same
`baseline_sha`, and it must not be silently changed. R1/R2/R3
implement every treatment as new, additive code reachable only through
explicit experimental entry points (`issue42-eval`'s own binaries) —
`commerce_core::ir::query::compile` and `commerce_core::plan::execute_planned`
are not edited until a treatment wins its gate and a RED-first production
change is made in its own checkpoint commit. Until then, "the baseline"
means: these two functions' behavior, unmodified, at the frozen SHA.

## Shared infrastructure (built before R1 starts, used by R1/R2/R3)

### Ground-truth oracle (`issue42-eval::oracle`)

A pure-function module keyed on a hand-written `QueryIntent` describing
what a query *should* mean in domain terms (e.g. "the automotive Wiper
Blades product type, Numeric `size` = 34.0" vs. "the apparel Jeans
product type, Enum `size` = \"34\""), never on a generator's own
internal representation of "the query I happened to build." For a given
`QueryIntent` and the actually-materialized `Catalog` for the run, the
oracle computes, independently:

- the set of `(ProductId, VariantId)` pairs that satisfy the intent
  exactly (`RelevanceLabel::Exact`);
- the set that partially satisfy it (`Partial`, e.g. right product type,
  wrong attribute value);
- everything else is `Irrelevant` by omission.

The oracle module has zero dependency on `issue38_e2e3_eval`'s
`generate_workload`/`ground_truth` modules or on any per-family helper
closure defined there. It is unit-tested against small, hand-built
`Catalog` fixtures constructed directly in `oracle`'s own test module
(not reusing any synthetic-catalog generator), so its own correctness
does not depend on the thing it is meant to check.

### Regression-check layer (`issue42-eval::regression`)

For every workload query below marked "positive": after materializing
the catalog for a given seed, assert the oracle finds at least one
`Exact` match in the *actual* generated catalog (not merely that some
generator-internal judgment map is non-empty — the oracle recomputes
this independently, per above). For every query marked "negative": assert
the oracle finds zero matches of the forbidden interpretation. Both are
`#[test]`-level regression checks that run in CI (`cargo test`), not
one-off scripts — the same enforcement pattern
`issue38_e2e3_eval::ground_truth::assert_every_query_of_template_has_an_exact_match`
established, generalized to also verify negatives and to check against
the independent oracle rather than the generator's own labels.

### Datasets

Per Issue #42 ("Use the existing synthetic catalogs only as controlled
evidence"): R1/R2/R3 reuse `issue38_e2e3_eval`'s existing generators
(`automotive`, `apparel`, `furniture_synth`, `mixed_merchant`) to
materialize catalogs, since they are already deterministic, seeded, and
committed. Every relevance judgment used for R1/R2/R3's own metrics comes
from the independent oracle above, never from those generators' own
`generate_workload` judgment maps — reuse is of the *catalog*
(products/variants/attributes), not of the *labels*. Where the
preregistered workload below needs a case those generators do not
produce (e.g. a genuinely collision-prone identifier, or a
same-token-four-ways case), `issue42-eval` adds a small, seeded,
purpose-built extension generator, following the same determinism
discipline (`ChaCha8Rng::seed_from_u64`, fixed constant, no wall-clock).

### Calibration / test separation

R1 and R2 have no tunable numeric parameters beyond the treatments
themselves (each treatment is a fixed algorithm, not a parameterized
one), so no calibration split is needed for them — all workload queries
are held-out test queries from the start, and the thresholds below are
final. R3's identifier classifier (Treatment C) does have tunable
feature-statistic cutoffs (entropy, uniqueness ratio, collision rate);
its calibration set and held-out test set are named explicitly in R3's
own section below, disjoint by construction (different seeded generator
runs), before Treatment C is implemented.

### Reproduction command and manifest

One command per experiment, run from the repo root after
`cargo build --release -p issue42-eval`:

```bash
./target/release/r1_typed_ambiguity_eval
./target/release/r2_residual_lexical_eval
./target/release/r3_identifier_primitive_eval
```

Each writes a JSON summary (`docs/research/artifacts/i42_r{1,2,3}_run1/summary.json`)
and a paired manifest (`benchmarks/manifests/i42_r{1,2,3}_*.yaml`,
`artifacts/manifests/i42_r{1,2,3}_*.json`), matching the two-file
convention `benchmarks/README.md` already establishes, recording
`baseline_sha`, treatment identifiers, seeds, config, and reps.

### Known confounders (named before any treatment runs)

- **P9-E06's ranking-cost-vs-candidate-set-size scaling** (frozen, open,
  `PHASE9_DECISION.md`): any latency comparison across treatments that
  changes candidate-set size (e.g. R1's Treatment B, unconditional union,
  can only ever grow the candidate set relative to A) must report
  candidate-set size alongside latency, since a latency delta confounded
  with a candidate-set-size delta is not attributable to the treatment
  mechanism alone.
- **Synthetic-catalog selectivity is not representative of a real
  catalog's**: `issue38_e2e3_eval`'s generators were built for E2/E3's
  own purposes; reusing them for R1/R2/R3 inherits whatever selectivity/
  cardinality distribution they happen to have (e.g. `min_enum_frequency`
  effects), which is a controlled-evidence limitation restated in every
  R1/R2/R3 decision record, not a defect of this protocol.
- **`phase9_eval::bitmap_delegate::BitmapTantivyDelegate`'s product-level-only
  text indexing** (issue #41): R2/R3 both touch residual-lexical and
  identifier behavior, where this delegate's scope choice is a
  confounder if not controlled for explicitly — R3 in particular must
  distinguish "the delegate doesn't index variant text" from "the
  identifier primitive itself doesn't work," which is exactly R3's own
  question, so this is named, not hidden.
- **Reviewer independence**: the adversarial reviewer for each of
  R1/R2/R3/E2b is a fresh subagent with no implementation task in this
  session's history visible to it beyond what the review prompt states,
  per Issue #42 rule 10 — but it is still launched by the same overall
  session that implemented the treatments, so its independence is
  bounded, not absolute. This limitation is recorded in the final
  decision record, not treated as fully resolving the "do not trust the
  author" concern on its own.

---

## R1 — typed ambiguity and corroborated resolution

### Hypotheses (each independently falsifiable)

- **H1-A**: Treatment A (current hard-coded numeric resolution) produces
  at least one wrong-family false positive or one missed corroborated
  match on the preregistered workload below (this is expected to be
  TRUE and is the reason R1 exists — E3/issue #40 already measured one
  concrete instance; R1 broadens and formalizes it).
- **H1-B**: Treatment B (unconditional multi-match union) eliminates
  missed matches but introduces wrong-family false positives on at least
  one negative/adversarial query (i.e., resolving both typed
  interpretations as hard constraints simultaneously produces an
  incorrect hard filter when the two interpretations are mutually
  exclusive for a given product).
- **H1-C**: Treatment C (uncorroborated ambiguous demoted to preference)
  eliminates wrong-family false positives but scores materially lower
  NDCG on genuinely corroborated queries than Treatment D, since it never
  uses available corroborating context to prefer one interpretation.
- **H1-D**: Treatment D (entity/category-corroborated typed selection)
  eliminates wrong-family false positives, recovers corroborated queries
  at high NDCG, and does not fabricate a unique meaning for genuinely
  ambiguous uncorroborated queries (falls back to C's demotion
  behavior), at <=5% serving overhead vs the frozen baseline (A).

None of these are assumed true; each is measured and the GO gate below
decides which treatment (if any) is adopted.

### Treatments — implemented behind experimental boundaries first

All four live in `issue42-eval::r1_experimental` as free functions over
a `CommerceQuery`-shaped intermediate, never inside
`commerce_core::ir::query::compile` until Treatment D (or another) wins
its gate:

- **A**: calls the real, unmodified `commerce_core::ir::query::compile`
  directly — the current hard-coded numeric branch, reused verbatim
  (not reimplemented, so "current behavior" cannot itself be a source of
  divergence).
- **B**: a from-scratch compiler pass that, for a numeric-shaped token
  registered as *both* a `Constraint::Numeric` candidate (via the
  existing "size N"/"under"/"over" keyword shape) *and* an `Enum`/
  `Identifier`-typed lexicon candidate, resolves *all* typed
  interpretations simultaneously as separate hard constraints ORed at
  the top level (a disjunction of fully-typed sub-queries), rather than
  today's "numeric always wins, unconditionally" behavior.
- **C**: reuses B's multi-interpretation discovery, but every
  interpretation lacking a corroborating entity/category constraint
  elsewhere in the same query is demoted to a `Preference::Boost`
  instead of a hard constraint — generalizing the existing P9-E05
  demotion rule (today applied only to lexicon-derived attribute
  matches) to numeric-keyword-derived ones too.
- **D**: like C, but when a corroborating entity/category constraint IS
  present, uses it to select exactly one typed interpretation as the
  hard constraint (the one whose attribute is actually registered on
  that entity's product type in the current catalog profile), rather
  than either keeping all of them (B) or demoting all of them (C).

### Workload

Built from `issue38_e2e3_eval::mixed_merchant::generate_mixed_catalog`
(reused catalog) plus a small `issue42-eval::r1_workload` extension
generator for cases the existing generators do not produce. Every
"positive"/"negative" label below is enforced by the regression-check
layer against the real materialized catalog, not asserted in prose:

| # | query | class | intent |
|---|---|---|---|
| 1 | `size {v}` | ambiguous, corroboration absent | matches apparel Enum size="{v}" AND automotive Numeric size={v}.0 in this catalog (both real) |
| 2 | `size {v} jeans` | corroborated -> apparel | "jeans" entity corroborates Enum interpretation |
| 3 | `size {v} wiper blades` | corroborated -> automotive | "wiper blades" entity corroborates Numeric interpretation |
| 4 | `under $34` | distinct existing keyword path | PriceUnderCents, must not be affected by any R1 treatment (regression guard) |
| 5 | `over $34` | distinct existing keyword path | PriceOverCents, same guard |
| 6 | `2015 honda civic brake pads` | number-as-year, corroborated | fitment MultiEnum match (reuses E2's fixed fitment-phrase mechanism) |
| 7 | `part number IA-1234-BP` | number-as-identifier | must not be captured by any numeric-typed interpretation at all (an identifier is not a Numeric attribute) |
| 8 | a single token verified present as Enum, Numeric, Identifier-shaped text, and plain lexical text across the catalog (constructed by the r1_workload extension generator; documented with its concrete value at run time, not fixed in this doc) | same-token-four-ways | stress case for all four treatments |
| 9 | `size purple` | negative | no valid numeric OR enum interpretation exists for "purple" as a size; must resolve to zero hard constraints / residual text only |
| 10 | `size 999999` | negative | outside any real generated size range in either family; must not match anything, must not error |

Single-category runs use `issue38_e2e3_eval::apparel`/`automotive`
alone; mixed-category runs use `mixed_merchant`. Both are run for every
query above where applicable.

**Correction (found while implementing the workload, before any
treatment ran)**: rows 1-3 as originally drafted here assumed a query
text/value that either could never be produced by the real generators
or did not actually exercise `compile()`'s numeric keyword branch at
all, caught by direct source reading (`crates/commerce-core/src/ir/query.rs`)
rather than by running anything first:

- **Row 3's original text, `"34 inch wiper blade"`, does not contain
  the literal `"size"` keyword `compile()`'s numeric branch requires**
  (confirmed directly against `query.rs`'s `tokens[i] == "size"` check)
  -- it would never have exercised the mechanism R1 exists to test at
  all. Corrected to `"size {v} wiper blades"`, matching row 2's shape,
  and using the registered `ProductType` phrase's exact plural text
  (`compile()`'s phrase lookup is an exact, case-insensitive,
  space-joined string match -- no stemming -- confirmed directly
  against `query.rs`'s window-scan loop).
- **Row 1's original claim -- a single value "34" real in both an
  apparel Jeans Enum `size` and an automotive Wiper Blades Numeric
  `size` -- is unachievable with the existing generators**: apparel's
  Jeans sizes are drawn from `["30","32","34","36","38"]`
  (`apparel.rs`), automotive's Wiper Blades `size` is
  `rng.gen_range(16..=28)` (`automotive.rs`) -- two disjoint numeric
  ranges that can never produce a shared value, confirmed by reading
  both generators directly rather than assumed. `mixed_merchant`'s own
  `size_conflict_anchors` (used by E3) independently confirms this: it
  returns the *first* Jeans and Wiper-Blades product's own size value
  as two separate, independently-drawn anchors, never asserting they
  are equal, and E3's own workload queries them as two separate `"size
  {anchor}"` queries for exactly this reason. Rows 1-3 are corrected to
  use a small, purpose-built `issue42-eval::r1_workload` fixture (not a
  modification of the frozen `apparel`/`automotive` generators, which
  back the already-merged E1-E3 baseline and must not change) with a
  deliberately-constructed overlapping value (`v = 22`, in automotive's
  16-28 range and given directly to a hand-built Jeans-type product's
  Enum `size`), so rows 1-3 test a case that is genuinely, verifiably
  real rather than one that happened to look plausible in prose.

This correction was made before any R1 treatment was run or measured,
so there are no pre-correction R1 figures to retain alongside it (rule
9 applies to a measured result changing after correction; here nothing
had been measured yet).

### Metrics (per query class, all four treatments)

Recall@10, Precision@10, NDCG@10 (oracle-labeled, per above); wrong-family
rate (fraction of returned hits belonging to a product family the
oracle's `QueryIntent` does not name as a target); false-positive hard
constraints (a hard constraint present in the compiled query that the
oracle would not have asserted for that query); zero-result rate; `plan`
outcome distribution (FastPath/Hybrid/Punt); compile and `execute_planned`
P50/P95/P99 latency; allocation count per compile call (matching E1's
own allocation-counting method).

### GO gate (preregistered thresholds, final)

**GO** for a treatment only if, on the full workload above:

- **zero** wrong-family false positives among accepted hard constraints
  (hard requirement, not a percentage);
- corroborated queries (rows 2, 3, 6) achieve mean NDCG@10 >= 0.95
  against the oracle;
- ambiguous uncorroborated queries (row 1) do not resolve to a single
  hard constraint naming only one family — either both interpretations
  remain reachable (as preferences, as an OR, or as an explicit
  `ambiguous` span) or the query is demoted entirely; a treatment that
  silently picks one family as a hard filter for row 1 fails this gate
  regardless of its other numbers;
- negative queries (rows 9, 10) produce zero hard constraints from the
  ambiguous-numeric mechanism (an unrelated structural constraint from a
  different mechanism, e.g. an unrecognized product type phrase, is not
  a violation);
- `execute_planned` P50 serving overhead vs Treatment A <= 5%, matching
  E1's own bar, measured with E1's batching+`black_box` discipline if
  the per-call cost is sub-millisecond.

If more than one treatment clears the gate, prefer the simplest
(fewest new mechanisms) that clears it — do not adopt D over C merely
because D is more sophisticated if C already clears every threshold.
If none clear it, record REVISE (name the closest treatment and what
specifically fails) or STOP (if no treatment resolves the wrong-family
requirement at all) — do not retroactively loosen a threshold to
manufacture a GO.

---

## R2 — residual lexical semantics

### Hypotheses

- **H2-A**: Treatment A (current strict veto — a delegate returning zero
  raw hits for a non-empty residual term zeroes the whole query) fails
  to recover benign zero-result cases (issue #40's own
  `residual_veto_probe` finding, generalized).
- **H2-B**: Treatment B (unconditional structural fallback when the
  delegate returns nothing) recovers benign cases but also recovers
  incompatible/adversarial ones (a residual term that is genuinely
  disqualifying gets ignored just as readily as one that is merely
  uninformative).
- **H2-C**: Treatment C (residual lexical never a hard filter, ranking
  only) has the same over-recovery failure mode as B for adversarial
  cases, since ranking-only still returns the full structural set.
- **H2-D**: Treatment D (compiled residual policy: `required`/
  `preferred`/`contextual`/`unknown` per token, decided at ingestion
  time from catalog statistics, not at query time) recovers benign cases
  at high rate and rejects adversarial cases at low false-recovery rate,
  without any query-time model call.

### Treatments

All four in `issue42-eval::r2_experimental`, operating on
`plan::execute_planned`'s already-computed `(PlannedQuery, residual_lexical)`
pair without modifying `commerce_core::plan` until a treatment wins:

- **A**: the real, unmodified `execute_planned` (residual lexical is an
  effective hard AND against the delegate's index).
- **B**: if the delegate returns zero raw hits for a `Hybrid`/`Punt`
  outcome with a non-empty residual, fall back to the structural
  candidate set alone (unranked by residual text, ranked only by
  whatever default signal `execute_ranked` uses for FastPath).
  Unconditional — never checks whether the residual term was actually
  disqualifying.
  - **C**: residual lexical text is *always* advisory: delegate results, if
  any, are used only to re-order the structural candidate set (never to
  filter it); if the delegate returns nothing, behave exactly like B for
  that query.
- **D**: at ingestion time (`CatalogProfile`-adjacent, new code, not
  modifying `compile_lexicon`/`CatalogProfile` themselves until this
  treatment wins), classify every token/short-phrase ever observed as
  residual lexical text into one of four classes from catalog
  statistics alone: `required` (token's presence in a match is load-bearing
  — e.g. co-occurs with disqualifying negation elsewhere, or the
  catalog has products that structurally match but lexically conflict);
  `preferred` (token correlates with relevance but is not necessary —
  most residual attribute-ish tokens); `contextual` (token only makes
  sense combined with specific entities, e.g. unit words like "inch");
  `unknown` (token never observed with enough frequency to classify —
  falls back to C's advisory behavior for safety). At query time, a
  `required`-classified residual with zero delegate hits keeps the
  query at zero results (A's behavior for that token only); a
  `preferred`/`unknown` one triggers B's fallback; `contextual` is
  evaluated per its associated entity.

### Workload

Paired benign/adversarial cases, built from `mixed_merchant`'s catalog
plus small additions where needed:

| # | query | class | expectation |
|---|---|---|---|
| 1 | `furniture sofas` | benign, zero-delegate-hit residual | should recover the Sofas structural set (residual "furniture" is uninformative, not disqualifying) |
| 2 | `banana sofas` | adversarial | "banana" is a genuinely incompatible/absurd residual; a treatment that recovers this the same way as #1 is over-recovering |
| 3 | `waterproof hiking boots` | benign, real attribute absent from title | if "waterproof" is a real but sparsely-titled attribute, should still recover Boots-typed structural set |
| 4 | misspelling of a real registered entity word (e.g. one transposed letter) | benign-adjacent | documents current behavior (misspelling correction is explicitly out of scope for R2 — measured, not required to pass) |
| 5 | a real collection/marketing term absent from every title (e.g. "bestseller") | benign | should recover the structural set the entity alone identifies |
| 6 | a genuinely incompatible attribute requirement for the matched entity (e.g. a color that catalog data shows is never sold for that product type) | adversarial | must NOT recover — this is exactly what `required` classification (Treatment D) exists to protect |
| 7 | structurally valid query, delegate returns zero (constructed directly, not relying on title coincidence) | benign | must recover |
| 8 | purely lexical query, no structural anchor at all | benign, distinct case | must not be affected by any residual-policy treatment (regression guard: Punt-with-no-constraints path is untouched) |

### Metrics

Zero-result recovery rate on benign cases (1, 3, 5, 7); false recovery
rate on adversarial cases (2, 6); Precision/Recall/NDCG@10 across all
cases; `plan` outcome transitions (does a treatment change FastPath/
Hybrid/Punt distribution, not just final hits); latency; candidate-set
growth (B/C can only grow the returned set relative to A — report by
how much).

### GO gate (preregistered thresholds, final)

**GO** for a treatment only if: >=90% recovery on preregistered benign
residual cases (1, 3, 5, 7 — at least 4 of these count toward the
denominator; more may be added before treatments run, never after);
<=1% false recovery on preregistered incompatible/adversarial cases (2,
6); zero query-time model/LLM calls in the treatment's own code path
(structural requirement, not just a metric — a treatment implementation
that calls out to a `ModelProvider` at query time fails this gate
regardless of its numbers, per CLAUDE.md's hard rule). If no treatment
can distinguish benign from adversarial deterministically at query time
at all (i.e., B and C's over-recovery is inherent to any query-time-only
mechanism), record that finding explicitly: ingestion must compile a
residual policy (D's shape) and an unconditional query-time fallback
(B or C) must not ship, regardless of its recovery numbers looking
superficially good — recovery alone does not justify shipping an
over-recovering treatment.

---

## R3 — identifier serving primitive

### Hypotheses

- **H3-A**: Treatment A (current: product-level lexical delegate only)
  cannot find variant-level identifiers at all (issue #41's own
  finding, restated as a falsifiable claim to re-confirm, not assumed).
- **H3-B**: Treatment B (index variant-level Text via the lexical
  delegate, i.e. fix `phase9_eval::bitmap_delegate::build_index`'s scope
  in an experimental copy, not the shared module) recovers exact
  identifier lookup but at a measurable index-size/build-time cost
  proportional to variants-per-product, and remains vulnerable to
  partial/prefix false matches (a general-purpose text index has no
  notion of "this token is a complete identifier").
- **H3-C**: Treatment C (a dedicated identifier dictionary/hash
  primitive, selected only for fields whose measured statistics look
  identifier-like) achieves higher Recall@1 and lower false-match rate
  than B, with a lower incremental update cost, but only for fields the
  classifier actually accepts — fields it correctly abstains on must
  fall back to B or A.

### Treatments

All three in `issue42-eval::r3_experimental`:

- **A**: real, unmodified `phase9_eval::bitmap_delegate::BitmapTantivyDelegate`
  (product-level Text only), reused verbatim as the baseline.
- **B**: an experimental copy of `build_index` that additionally indexes
  every variant's own Text attributes (via `effective_attributes`, the
  same merge `CatalogIndex` already performs internally for
  `lexical_postings`) into the same Tantivy fields, tagged with the
  owning `VariantId` so a match resolves to the correct variant, not
  merely the parent product. **Correction (found while designing R3's
  treatments, before any code was written)**: `commerce_core::plan::LexicalHit`
  (`{ product: ProductId, score: f64 }`) has no `VariantId` field at
  all, and `commerce_core::plan::verify_and_truncate` resolves a hit's
  variant via `product.variants.iter().find(|v| query.matches_variant(product, v))`
  — `matches_variant` is `self.constraints.iter().all(...)`, vacuously
  `true` when `query.constraints` is empty (exactly the case for an
  identifier-only query like a bare `part_number` value, since
  `commerce_core::ir::query::compile` has no dedicated keyword branch
  for "part number" at all — confirmed directly against `query.rs`;
  such a query's entire text becomes `residual_lexical`, routing to
  `Punt` with zero structural constraints). So today, for exactly this
  query shape, `verify_and_truncate` always returns a product's *first*
  variant, regardless of which variant's text a delegate actually
  matched — the existing `LexicalHit`/`verify_and_truncate` pipeline
  structurally cannot carry "this specific variant is what matched"
  information at all, not merely omits doing so for product-level-only
  indexing (H3-A's own framing). Treatment B as originally drafted here
  ("tagged with the owning VariantId so a match resolves to the correct
  variant") is therefore not reachable by reusing `execute_planned`/
  `LexicalHit` the way R1/R2's treatments reuse them: `issue42_eval::r3_experimental`
  implements Treatment B as its own self-contained index-and-lookup path
  (a richer, variant-aware hit type returned directly by the
  experimental index, not routed through `LexicalDelegate`/
  `execute_planned` at all) rather than a `LexicalDelegate` implementation
  passed into the real `execute_planned`. This is consistent with
  R1/R2's own experimental-boundary discipline (new, additive code,
  `commerce_core::plan` untouched) — it is a difference in *how* B is
  wired, not a departure from "reuse real production primitives where
  they exist": no real production primitive for variant-resolved
  lexical hits exists yet to reuse.
- **C**: a dedicated `HashMap<String, SmallVec<(ProductId, VariantId)>>`
  (or equivalent exact/normalized-key structure) built directly from
  fields a deterministic classifier accepts as identifier-shaped. The
  classifier (calibrated per below) inputs only measured field
  statistics — uniqueness ratio, character-entropy, format-consistency
  (e.g. fixed-length/fixed-charset ratio), collision rate, Product vs.
  Variant scope, and observed exact-query rate from the workload's own
  calibration split — never the field's name. A field the classifier
  rejects or abstains on is not given a Treatment-C primitive at all;
  queries against it fall back to B.

### Calibration / held-out split (named before Treatment C is built)

Calibration set: `automotive::generate_catalog(1500)` with
`SEED.wrapping_add(1000)` (a distinct seed from the one used everywhere
else in this protocol, so calibration data is never reused as held-out
evidence) plus a small `issue42-eval` extension adding two deliberately
non-identifier-like fields (a low-cardinality Enum re-labeled with a
misleadingly identifier-sounding name, and a free-text field with
high entropy but no real per-variant uniqueness) specifically to check
the classifier does not key off field *names*. Classifier
cutoffs (minimum uniqueness ratio, minimum entropy, etc.) are chosen by
inspecting only this calibration set's statistics before any held-out
metric is computed.

Held-out test set: `automotive::generate_catalog(3000)` at the
protocol's normal seed (`automotive::SEED`, the same catalog R1/E2 already
use) plus `mixed_merchant`'s 3,000-product mixed catalog — both
disjoint generator invocations from the calibration set above. All R3
metrics reported below are computed only on this held-out set.

**Correction (found while designing R3's fixture, before any code was
written)**: the calibration/held-out text above assumes
`automotive::generate_catalog` accepts a seed parameter it can be
called with (`generate_catalog(1500)` "with `SEED.wrapping_add(1000)`").
Confirmed by direct source reading
(`crates/issue38-e2e3-eval/src/automotive.rs`): the real signature is
`generate_catalog(n_products: usize) -> Vec<SynthProduct>` — no seed
parameter at all. Internally it reseeds a fresh `ChaCha8Rng` from the
crate's own `automotive::SEED` constant on every call. This is more
than an inconvenient signature mismatch: since `generate_catalog(1500)`
and `generate_catalog(3000)` both reseed from the *identical* `SEED`,
and every product's fields are a pure function of its loop index
(`PRODUCT_TYPES[i % PRODUCT_TYPES.len()]`, then sequential RNG draws),
`generate_catalog(1500)`'s entire output is a byte-identical *prefix*
of `generate_catalog(3000)`'s own first 1500 products. Calling both
literally as originally specified here would make the "calibration"
set a strict subset of the "held-out" set — silently violating this
section's own disjointness requirement ("its calibration set and the
held-out evaluation set are named explicitly and disjoint... before
any treatment runs"), not a cosmetic issue.

**Corrected split**: call `automotive::generate_catalog(4500)`
(1500 + 3000) exactly once, and partition its own output by index
range — products `[0, 1500)` are the calibration set, products
`[1500, 4500)` are the held-out set (3000 products). These are
genuinely disjoint (non-overlapping products from one deterministic
generation), preserving the spirit of "a calibration draw distinct
from held-out evidence" without requiring a seed parameter the frozen
generator does not have. `mixed_merchant`'s 3,000-product mixed
catalog (`generate_mixed_catalog(1000, 1000, 1000)`, matching E3's own
established split, `crates/issue38-e2e3-eval/src/bin/e3_mixed_category_eval.rs`)
is reused as-is for the held-out set's general-lexical-retrieval
regression check only (does adding B's/C's mechanism regress ordinary
furniture/apparel/automotive free-text queries) — never for the core
identifier-classification metrics (Recall@1, false-match rate,
collision/normalization/absent-identifier behavior, build time, index
size, RSS, lookup-latency percentiles), which are computed only on the
split-index held-out automotive set above. This scoping matters
because `mixed_merchant`'s own automotive sub-portion
(`automotive::generate_catalog(1000)`, indices `[0, 1000)`) overlaps
this correction's *calibration* range (`[0, 1500)`), not its held-out
range — reusing it only for the non-calibration-sensitive regression
check, never for a classifier-calibration-affected metric, keeps that
overlap from being a real disjointness violation rather than merely an
undisclosed one.

### Workload

Exact match (real `part_number` values); case/punctuation normalization
(`"ia-1234-bp"` vs `"IA-1234-BP"` vs `"IA 1234 BP"`); prefix/partial
strings (`"IA-1234"` alone — must not silently resolve to a false exact
match); collisions (an extension generator deliberately assigns the
same identifier string to two different variants, to prove collisions
are surfaced explicitly, never silently arbitrated); reused manufacturer
codes (the same code appearing as a legitimate cross-reference on
multiple unrelated products — distinct from an in-catalog collision);
absent identifiers (a product with no part number attribute at all —
must not error, must not false-match); many variants per product (a
product type extension with 5-10 variants each carrying a distinct
identifier, to stress B's per-variant indexing cost and C's dictionary
size); deliberate adversarial near-matches (a single-character edit from
a real identifier that must NOT match).

### Metrics

Recall@1 and false-match rate (exact query resolves to the correct
variant, and only the correct variant, at rank 1); collision behavior
(both colliding variants surfaced, never one silently dropped);
normalization behavior (case/punctuation-insensitive exact match, no
false prefix match); build time and incremental update cost (single
new-variant insertion, not full rebuild); index size and RSS; P50/P95/P99
lookup latency; scaling by variants-per-product (build time and lookup
latency as a function of variant count, not just product count); effect
on general (non-identifier) lexical retrieval quality (does adding B's
or C's mechanism regress unrelated free-text queries' NDCG at all).

### GO gate (preregistered thresholds, final)

A dedicated primitive (Treatment C) enters the architecture only if,
on the held-out set: Recall@1 >= 0.99 with false-match rate == 0 on
fields it accepts (collisions reported explicitly, not counted as
false matches, since a real collision is a correct "more than one
match" answer, not a wrong one); build time and incremental update cost
are lower than Treatment B's for the same field at every tested
variants-per-product level; RSS/index-size overhead is not disqualifying
on its own (report it, do not gate on it, since a correctness-and-speed
win at acceptable memory cost is still a legitimate win); and it causes
no measurable regression (>1% relative NDCG drop) to general lexical
retrieval. If Treatment C's classifier abstains on a field, that field's
queries are measured under B (or A) instead, and the report states the
abstention rate explicitly — abstention is not a failure of the gate,
silently mis-classifying a non-identifier field as one would be.

---

## E2b protocol summary (see body for full detail; preregistered here)

E2b's own baselines (statistics-only, LLM-no-validator, LLM+validator,
blinded oracle), datasets (existing synthetic catalogs plus at least one
real structured external feed), descriptor schema, and GO gate are as
specified verbatim in Issue #42's own E2b section — that section is
itself sufficiently precise to serve as E2b's preregistration and is not
restated here to avoid two documents silently drifting apart; any
refinement needed once implementation starts will be added as a dated
amendment to this document's own E2b section below, before the held-out
evaluation runs, never after.

### E2b amendments (dated, added before the held-out run each time)

*(none yet — this section exists so any necessary refinement is
recorded here, in one place, rather than only in a commit message)*
