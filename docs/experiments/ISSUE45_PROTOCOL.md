# Issue #45 Preregistered Protocol — E2c: deterministic semantic
# canonicalization for stochastic LLM proposals

Committed before any E2c canonicalizer code is written and before any
held-out measurement is run, per Issue #45's own governance ("preregister
protocol, treatments, thresholds, datasets, splits, and failure conditions
before held-out runs"). This document is frozen at commit time; any
amendment needed once implementation starts is added as a dated addendum
below, before the held-out evaluation runs, never after — mirroring
`ISSUE42_PROTOCOL.md`'s own discipline.

## 0. What this is testing

Issue #42/PR #44 established: LLM-assisted feature discovery materially
outperforms a statistics-only floor (macro F1 0.7697 vs 0.5366), zero
confirmed unsafe accepted structural classifications, but repeated-run
agreement on accepted physical primitive selection landed at **87.60%**
(1095/1250 pairwise comparisons), below the preregistered 90% bar, even
after a 10x-larger rerun. The question this issue poses is **not** "how do
we prompt the LLM to raw-agree more" — it is whether a deterministic
canonicalization layer, downstream of the same stochastic proposals, can
produce a **stable, safe, compiled** result regardless of that raw
instability. Raw disagreement is expected and permitted; what must shrink
is disagreement in the compiled serving semantics.

## 1. A disclosed limitation on "held-out," stated up front

Issue #45 itself requires this experiment to "explain the 87.60% result...
for every unstable field, classify the disagreement into categories" —
which is impossible without reading every one of the 20 frozen
`dataset_cache/export/e2b_llm_proposals_*.json` artifacts across all 4
configurations (`automotive`, `wands_baseline`, `wands_anonymized`,
`wands_noisy`). That analysis (§6 below) was performed before this
protocol's canonicalization rules (§4) were finalized, and necessarily
means **there is no config or run subset this document can honestly call
"never seen."** A clean blind train/test split at the configuration or run
level is not available here, unlike E2b's own original 8-vs-12-run
stability design.

What **is** still a real, enforceable commitment, and what this protocol
actually relies on instead:

1. Every canonicalization rule in §4 is **principle-derived**, not
   data-fit: each rule is either (a) reused verbatim from
   `e2b_validator.rs`'s own already-shipped, already-governed thresholds
   (frozen under Issue #42's own process, before this experiment existed),
   (b) copied near-verbatim from Issue #45's own preregistered example
   text ("values `{S,M,L,XL}`, numeric parse rate ~0%, low cardinality ->
   Enum/Bitmap"), or (c) derived from an audited, measured fact about
   `commerce_core` itself (which physical primitives exist; how
   `CatalogIndex::build` actually derives structure from `AttributeValue`
   kind; whether WANDS's own ingestion produces real per-row Variant
   identity) — never from searching for the threshold that happens to
   maximize agreement on any specific disagreeing key.
2. The rules (as Rust code) are committed in the same PR checkpoint as
   this protocol, **before** the measurement binary is run for the first
   time and before any GO-gate number is looked at. No rule is edited
   after seeing a measurement result unless this document is amended with
   a dated note explaining why, per Issue #42/#45's own rule 9 discipline.
3. Per-configuration reporting (§7) still separates `wands_baseline`/
   `automotive` from `wands_anonymized`/`wands_noisy`, mirroring E2b's own
   `CANONICAL_CONFIGS` split — not as a blind test, but as a
   **generalization check**: if canonical stability is similarly high
   across all four despite the rules never referencing config-specific
   thresholds, that is still real (if weaker than a true blind split)
   evidence the rules are not overfit to name-visibility.

This limitation is recorded here, not glossed over, per this repo's own
"do not trust the experiment author" governance (Issue #42 rule applied
directly to this document's own author).

## 2. CandidateDescriptor schema — reused, not reinvented

Issue #45 asks for "a strict versioned `CandidateDescriptor` schema" with:
source/relationship identifier, proposed role, value type, scope,
supported operators, aliases, retrieval-vs-ranking importance, physical
primitive, confidence, evidence/provenance, and an explicit abstain state.

**`crates/issue42-eval/src/e2b_schema::Descriptor` already is exactly
this schema, field for field** (`key`/`real_key`, `semantic_role`,
`value_type`, `scope`, `supported_operators`, `aliases`,
`relationship_semantics`, `retrieval_significance`,
`candidate_physical_primitive`, `confidence`, `evidence`, `abstain`) — it
is what every one of the 20 frozen raw LLM proposal artifacts already
deserializes into. Per CLAUDE.md's "prefer typed domain concepts over
generic JSON/document abstractions" and "do not add abstractions beyond
what the task requires": **`CandidateDescriptor` is defined as a type
alias for `e2b_schema::Descriptor`, not a new parallel struct.** No new
raw-proposal schema is introduced. This also means E2c can reuse the 20
already-frozen artifacts directly with zero reformatting, satisfying
Issue #45's "reuse the frozen E2b catalog artifacts."

`e2b_schema::Descriptor` has no explicit machine-checked `schema_version`
field (a preexisting E2b gap, not introduced here) — `e2c_schema.rs` (§8)
adds one only to its own new `CanonicalDescriptor`/`CanonicalOutcome`
types, not retroactively to the reused `CandidateDescriptor`.

## 3. Canonical semantic space — audited against real `commerce_core`

Per Issue #45's own instruction ("audit existing `commerce_core`
primitives first and keep the semantic space no broader than the engine
actually supports"), a full audit was performed (file:line detail
preserved in this checkpoint's own agent transcripts, summarized here):

**Real physical primitive classes that exist and are wired into a live
query path in `commerce_core::index::CatalogIndex`:** Enum-equality
bitmap (`enum_bitmaps`), MultiEnum-contains bitmap (same bitmap
structure, `Constraint::MultiEnumContains`), Boolean-equality bitmap
(`bool_bitmaps`), sorted numeric-range index (`numeric_index`,
`Constraint::Numeric`), typed-ID equality bitmaps (Brand/ProductType/
Category — out of scope for E2c, these are structural `Product` fields,
never attribute descriptors), the R3 `IdentifierDictionary` (exact/
normalized lookup, gated by `IdentifierClassifier::accepts`), and
narrow-then-verify substring matching for `Text` (no index structure at
all — `attribute_bitmap` returns `None` for `Constraint::Text`, forcing a
linear re-check over the already-narrowed candidate set).

**Exists in code but is never invoked by any live query path today:**
`lexical_postings` (an attribute-agnostic token-postings structure,
`index/mod.rs`'s own doc comment discloses this — "exists for
experimentation... not wired into any query path"). E2c still uses this
as the canonical primitive for `FreeText`-role descriptors (matching
E2b's own choice), disclosing explicitly that a descriptor compiling to
`LexicalPostings` today gets structure built but not exercised by
`execute()` — an existing, disclosed engine gap, not something E2c
introduces.

**Does not exist anywhere in `commerce_core`:** a `RelationshipIndex` (or
any graph/cross-product-reference primitive). Confirmed by exhaustive
grep: the only place "Relationship" exists at all in this workspace is as
an `issue42-eval`-only oracle/proposal-schema label, never a
`commerce_core` type, field, or even a stub. This directly matches E2b's
own serving-contract-closure finding that WANDS's two oracle-labeled
Relationship fields are "never materialized or exercised anywhere in the
E2b pipeline."

**Consequence for the canonical vocabulary**: E2c reuses
`e2b_schema::{SemanticRole, ValueType, Scope, Operator, Significance,
PhysicalPrimitive}` **verbatim, with zero new variants**, for the
canonical output too. Issue #45's own illustrative role list (Brand,
ProductType, Category, Color, Size, Fitment, Quantity, ...) is
deliberately **not** adopted: Brand/ProductType/Category are structural
`Product` fields the descriptor pipeline never touches; Color/Size are
ordinary instances of `Enum`; `Fitment` corresponds to nothing in
`commerce_core`'s type system; `Quantity` is not a distinct
`AttributeValue` kind (`Numeric` already covers it). Adopting them would
violate "no broader than the engine actually supports" for no compiling
benefit. This is a deliberate, documented scope decision, not an
oversight.

**A structural rule that follows directly from the primitive audit**:
`PhysicalPrimitive::Relationship` does not exist as a variant at all (it
never did, in E2b's own schema) — and canonical `Scope::Relationship` /
`SemanticRole::Relationship` descriptors can only ever compile to
`PhysicalPrimitive::None`, `promotion_blocked: true`, because no serving
primitive exists to hold them. This is asserted as a hard, universal rule
in §4, grounded in an audited engine-capability fact, not a per-field
evidentiary judgment.

## 4. Deterministic canonicalization rules (frozen before measurement)

All rules below operate on the set of non-`abstain` `CandidateDescriptor`
proposals for one real key **within one configuration** (never merged
across configurations — merging `wands_baseline`'s name-visible evidence
with `wands_anonymized`'s name-blind evidence would not be a fair test of
absorbing one proposal mechanism's own stochasticity). They consult only:
(a) the raw proposals' own stated fields, (b) real measured
`UnifiedFieldStats`/`FieldStats` already computed deterministically from
the catalog (the same statistics every raw LLM pass itself was given, and
the same ones `e2b_validator.rs` already consults), and (c) audited,
static facts about `commerce_core`'s own primitive set (§3). **Never**
`e2b_oracle.rs` — the canonicalizer has no access to oracle labels at any
stage, matching E2b's own validator discipline.

### R1 — Physical primitive is a deterministic function of canonical role, not a free choice

```text
Enum       -> BitmapEnum
Boolean    -> BitmapEnum   (bool_bitmaps is a bitmap keyed by (field,bool); same physical family)
Numeric    -> NumericRange
Identifier -> IdentifierDictionary   (only if it also clears R5 below)
FreeText   -> LexicalPostings
Relationship, Ignore -> None
```

This is not a new judgment call invented for E2c — it is exactly how
`commerce_core::index::CatalogIndex::build` already behaves today: the
concrete `AttributeValue` variant (and therefore the concrete physical
structure `CatalogIndex` builds) is driven entirely by `semantic_role`;
`candidate_physical_primitive` is **never** consulted by
`e2b_ingest::build_catalog` (confirmed by direct reading of that
function). Raw E2b proposals could and did disagree on primitive
independent of role (e.g. `Enum`+`BitmapEnum` vs `Enum`+`LexicalPostings`
at identical statistics) purely because nothing forced the proposal's
stated primitive to track its own stated role. Making R1 an explicit,
enforced rule closes that gap and — per the taxonomy in §6 — accounts for
the largest single share (65.2%) of E2b's own measured raw disagreement.
This directly answers Issue #45's falsification criterion "physical
primitive choice is still materially controlled by free-form model
wording instead of measured semantics": under R1 it is never controlled
by wording at all; it is a fixed function of role, which is itself
resolved by R2-R6 from measured evidence.

### R2 — Role plurality vote among non-abstaining raw proposals, evidence-checked

Start from a plain plurality vote of `semantic_role` across all
non-abstaining raw proposals for this (config, real key). Ties are broken
by a fixed, disclosed precedence favoring the more conservative
interpretation: `Ignore < FreeText < Enum < Numeric < Boolean <
Identifier < Relationship` (i.e. on a genuine tie, prefer the answer with
the least structural commitment). This plurality result is then
overridden by R3-R6 below when real measured evidence contradicts it —
plurality alone is never final.

### R3 — Enum-vs-Numeric resolved by cardinality + parse rate, not vote

Per Issue #45's own example text verbatim: if `numeric_parseable_fraction`
is low (< 0.5) **and** `distinct_values` is bounded (<= 50 — a small,
disclosed constant chosen to cover exactly the "S/M/L/XL"-shaped and
"10mm/12mm/14mm"-shaped cases this protocol's own taxonomy in §6 found,
without reaching WANDS's own legitimate high-cardinality Enum fields like
`color` at 4,686 distinct), canonical role is forced to `Enum` regardless
of any `Numeric` votes. This directly resolves automotive's documented
`thread_size` case (unit-suffixed values defeat naive parsing; 4 of 5 raw
runs already agreed) without requiring any unit-stripping/normalization
machinery — deliberately the smallest rule that resolves the observed
case, per CLAUDE.md's "don't design for hypothetical future
requirements."

Conversely, if `numeric_parseable_fraction` is high (>= 0.9) and role
votes split Enum/Numeric, canonical role is forced to `Numeric` (the
symmetric case; not observed in the 20 frozen artifacts, included for
completeness and to keep the rule a total function).

### R4 — Zero/near-zero variance overrides everything to non-discriminating

If `distinct_values <= 1` (a real constant in the measured data, e.g.
automotive's `voltage` at `distinct_values=1`), canonical
`candidate_physical_primitive` is forced to `None` and
`retrieval_significance` to `Ignore`, regardless of R1/R2's own role
determination — a field with zero discriminating power has no serving
value no matter how confidently its type is known. This is the rule that
resolves `voltage`'s own disagreement (2 runs proposed `NumericRange`, 3
proposed `None`, all five agreeing on the same underlying fact and
disagreeing only about what to conclude from it).

### R5 — Identifier promotion requires the same statistical bar R3/production already uses

An `Identifier` canonical role is only permitted (else demoted to
`Enum` if cardinality allows, else `Abstain`) when the real measured
statistics clear `commerce_core::index::identifier::IdentifierClassifier::accepts`
(`uniqueness_ratio >= MIN_UNIQUENESS_RATIO`, `total_occurrences >=
MIN_IDENTIFIER_SAMPLE_SIZE`) — reusing the exact real production
classifier, never a reimplementation, matching E2b's own validator
discipline. `variant_scoped` is intentionally **not** required here
(unlike production `FieldStats`) because WANDS's own ingestion (R6 below)
never produces real per-row variant identity to measure it against; this
relaxation is disclosed, not silent.

### R6 — Scope defaults deterministically to the dataset's own real structure, never to a vote

`e2b_ingest::build_catalog`'s own doc comment already discloses: "every
WANDS record becomes exactly one `Product` with exactly one `Variant`...
[WANDS] has no real variant-grouping concept." Given that structural
fact, **Product vs Variant scope is not observable from any bounded
per-field statistic for WANDS as ingested here** — no amount of
canonicalizer cleverness can recover information the ingestion pipeline
itself never produces. Per-run scope votes for WANDS keys are therefore
never used at all: canonical scope for every WANDS-derived key is fixed
to `Product`, deterministically, as a dataset-structural default, not a
per-field evidentiary conclusion. `automotive`'s own generator is audited
the same way before this protocol is frozen: `issue38_e2e3_eval::automotive`
is likewise a flat per-row generator with no independent per-variant
attribute distribution to measure (confirmed: none of automotive's 3
unstable keys showed scope disagreement in the raw data at all, and
automotive's oracle assigns `Scope::Product` to all 17 fields), so the
same default applies there for consistency, not because it was separately
derived. A future dataset with genuine per-row Variant identity would
need this rule replaced with a real measured-variance-across-variants
check — explicitly out of scope here (no such dataset is used).

`Scope::Relationship` is never assigned by this rule (R6 only ever
produces `Product`) — a proposal whose evidence points to Relationship is
handled by R7, never by R6.

### R7 — Relationship is demoted or abstained, never promoted to a compiled primitive

Per §3's audit (no `RelationshipIndex` primitive exists anywhere in
`commerce_core`): if canonical role resolves (via R2) to `Relationship`,
the outcome is always `candidate_physical_primitive: None,
promotion_blocked: true`, with an explicit reason citing the audited
absence of a serving primitive — regardless of how many raw runs proposed
it, regardless of confidence. This is a hard rule, not a threshold, and
is the direct fix for the one confirmed hallucination case in §6 (a
single `wands_anonymized` run proposing `Relationship` for `color` off a
junk placeholder sample value `"[ tied to : color ]"`): even in the
hypothetical worst case where a majority proposed Relationship, R7 still
blocks promotion, because promotion here is bounded by engine capability,
never by vote count.

### R8 — Validator gate, applied to the canonical descriptor, not each raw one

Once R1-R7 produce a single canonical `(role, value_type, scope,
primitive)` tuple, `e2b_validator::validate()` is run against it exactly
as E2b already runs it against a raw proposal (parseability,
cardinality-ceiling downgrade, `Identifier` uniqueness/sample-size reject,
scope-consistency reject, operator-semantics downgrade, workload-evidence
downgrade, memory estimate). A `Reject` finding forces the outcome to
`Abstain`. This reuses the exact same, already-governed function — no
parallel validator is written. The one disclosed discrepancy already
present in the existing codebase (`ISSUE42_PROTOCOL.md`'s own doc text
states an Enum cardinality ceiling of 500; the shipped
`e2b_validator.rs::ENUM_CARDINALITY_CEILING` constant is `6000`) is
inherited as-is — the shipped code's behavior (`6000`) is what actually
executes and is what E2c's canonicalizer output is validated against;
this discrepancy predates E2c, is not introduced by it, and is recorded
here rather than silently resolved either direction.

### R9 — Cross-run type conflict still forces abstention (inherited, not replaced)

`e2b_validator::cross_run_type_conflict` (categorical vs Numeric,
non-abstaining on both sides) is still checked pairwise across the raw
proposals feeding one canonicalization; if it fires for the canonical
role's own category, the outcome is `Abstain`. This existing check is a
strict subset of what R2/R3 already resolve (R3 specifically targets the
Enum-vs-Numeric case with real evidence rather than merely blocking it),
so in practice R9 should rarely fire once R3 is applied — it is kept as a
defense-in-depth safety net, not removed, per "prefer abstention over
fabricating a structural meaning" applying to the canonicalizer's own
design, not only its output.

### R10 — Aliases and operators: union, never intersection or single-source

Canonical `aliases` = the deduplicated union of every non-abstaining raw
proposal's aliases for this key. Canonical `supported_operators` = the
union of operators consistent with the canonical `PhysicalPrimitive`
under R1 (e.g. `Enum` -> `{Eq, Contains}`, `Numeric` -> `{Eq, Range}`,
`Identifier` -> `{ExactLookup}`), not merely voted. A superset of query
phrases/operators can only improve recall, never introduce an unsafe
structural match, since every one is still gated by the same R8 validator
and the same real `Catalog::search` correctness machinery downstream —
consistent with Issue #45's "measure whether deterministic
canonicalization absorbs [disagreement]" without silently discarding
retrieval-significant signal a single run happened to contribute.

### R11 — Confidence is recomputed, never copied from a raw proposal

Canonical `confidence` = (count of non-abstaining raw proposals whose
`semantic_role` matches the final canonical role) / (count of
non-abstaining raw proposals) — a real, deterministically computable
agreement fraction, not any single run's self-reported (and, per this
repo's own governance, untrusted-by-default) confidence value.

## 5. Treatments (all four preregistered, per Issue #45)

All four treatments run **per configuration**, over the same 5 raw runs
E2b's own stability rerun already produced and froze
(`dataset_cache/export/e2b_llm_proposals_<config>_run{1..5}.json`) — no
new LLM calls, per Issue #45's "reuse the frozen E2b catalog artifacts."

**A. Raw LLM proposal.** E2b's own existing measurement is cited directly
(87.60% aggregate, per-config breakdown in `ISSUE42_LOG.md`), not
recomputed, since it is already frozen, byte-verified evidence (this
checkpoint's own reproduction script confirmed it byte-for-byte from the
raw artifacts before this protocol was written — see §6). For safety/
recall/relevance comparability, Treatment A's "accepted" set is E2b's own
Baseline 2 (LLM proposal, no validator, first-canonical-run-wins per real
key) exactly as `e2b_pipeline::build_baselines_2_and_3` already computes
it.

**B. Majority/plurality vote baseline.** A deliberately naive
multi-run consensus: for each (config, real key), plurality-vote every
`Descriptor` field independently (`semantic_role`, `value_type`,
`scope`, `candidate_physical_primitive` all voted **directly on the raw
proposals' own stated values** — critically, `candidate_physical_primitive`
is voted on as-is, R1 is **not** applied here, since testing whether a
real canonicalizer beats naive voting requires B to be genuinely naive
about the role<->primitive relationship). Ties broken by first-run-order
(a genuinely arbitrary, non-evidence-aware tiebreak, disclosed as such).
No validator, no catalog-evidence consultation beyond what raw proposals
already state, no engine-capability audit (B does not know
`RelationshipIndex` doesn't exist — if a plurality of raw runs propose
`Relationship`, B accepts it, an intentional, disclosed safety gap this
treatment exists to expose per Issue #45's own framing: "evaluation-only
unless proven safe; it exists to test whether a real canonicalizer beats
naive voting").

**C. Deterministic canonicalizer + validator.** Rules R1-R11 above,
applied in the order listed (R6/R7's hard structural rules and R4's
zero-variance rule take precedence over R2/R3's evidence-based role
resolution; R8/R9 gate the final result; R10/R11 are always applied to
whatever survives).

**D. Conservative canonicalizer with abstention.** Identical to C, with
one stricter admission bar layered on top: R2's plurality must be a
genuine **majority** (> 50% of non-abstaining raw proposals, not merely a
plurality) for the canonicalizer to promote a structural role
(`Enum`/`Numeric`/`Boolean`/`Identifier`); a role that only clears
plurality-but-not-majority under C is downgraded to
`abstain: true, semantic_role: Ignore` under D, with
`promotion_blocked: true` and a recorded reason
(`"role plurality below D's majority bar"`). R3/R4/R6/R7 (the
evidence/structural rules, not the vote-counting rule) are **unchanged**
between C and D — D is stricter specifically about how much raw-proposal
agreement is required before trusting a vote-driven role determination,
not about re-deriving the evidence rules themselves. Per Issue #45: "do
not assume D wins... measure the recall/abstention tradeoff" — both C and
D are measured and reported, neither is presumed correct in advance.

## 6. Explaining the 87.60% raw agreement result (measured, before §4/§5 code exists)

Reproduced independently from the raw artifacts (script preserved at
`scripts/e2c_disagreement_taxonomy.py`, output preserved at
`docs/research/artifacts/i45_e2c_disagreement_taxonomy_run1/`):
**1095/1250 (87.60%) exactly**, matching `ISSUE42_LOG.md`'s own committed
number byte-for-byte, confirming this reproduction is methodologically
identical to the original (same pairwise, same-config-only, role+primitive
equality definition).

Of the **155 disagreeing pairs** (12.40% of all 1,250), classified by
which real key disagreed and why, weighted by pair count:

| Category | Pairs | % of disagreement | Root cause |
|---|---|---|---|
| Primitive-selection ambiguity | 101 | 65.2% | `candidate_physical_primitive` (`bitmap_enum` vs `lexical_postings`, or `numeric_range` vs `bitmap_enum`) flip-flops **at identical measured statistics** for the same key — a genuinely free, evidence-independent choice under E2b's own schema, fully resolved by R1 (§4) once role is stable, since R1 removes primitive as an independent degree of freedom entirely. Affects `basecolor`, `finish`, `upholsterycolor`, `upholsterymaterial`, `primarymaterial`, `voltage`, `heat_range`. |
| Value-type ambiguity | 28 | 18.1% | Genuine Enum-vs-Numeric (`thread_size`: unit-suffixed values defeat naive numeric parsing) or Enum-vs-FreeText (`productwarranty`, `warrantylength`: low distinct-value count but a few outlier full-sentence values) boundary cases. ~~resolved by R3~~ **Corrected (Addendum 1 below): only the Enum-vs-Numeric half is resolved by R3. R3 structurally cannot engage on Enum-vs-FreeText at all** (`e2b_validator::cross_run_type_conflict`, which gates R3's every engagement, only ever fires Enum/Boolean-vs-Numeric) — `productwarranty`/`warrantylength` are resolved by plain R2 plurality alone, same as most of the dataset. This was a confirmed factual error in this table, found by this checkpoint's own fresh adversarial review (all three independent reviewers converged on it), corrected here per rule 9 rather than silently. |
| Model hallucination/error | 22 | 14.2% | `color` alone: one `wands_anonymized` run reads a literal junk placeholder sample value (`"[ tied to : color ]"`, almost certainly a WANDS export artifact) as evidence of a real cross-product relationship and proposes `Relationship`/`Relationship` scope; the field's own genuine data-quality noise (empty strings, bare `'0'`/`';'`, multi-value composite strings) drives further role churn (`enum`/`free_text`/`ignore`) independent of the hallucinated-relationship case. Addressed by R7 (hard-blocks Relationship promotion regardless) and by R8's existing cardinality/parseability checks for the residual enum/free_text churn. |
| Insufficient/contradictory evidence | 4 | 2.6% | `compatibledrainassemblypartnumber` only: the 5 sample values shown to each pass are numeric-looking, but the field's own aggregate `numeric_parseable_fraction` is only 0.13 — a real conflict between a small visible sample and the true underlying distribution that different runs resolved differently (2 abstained outright, 3 did not). A legitimate case for R8/validator-driven abstention, not a defect in any single run. |

**Not present as a measured category**: "semantic synonym only" (e.g. two
runs naming the same real concept with different labels/wording) does
**not** appear in this breakdown, because E2b's own stability metric
already joins by *real key* before comparing — synonym drift across
differently-named proposals for the *same* underlying concept is
structurally invisible to a per-real-key pairwise comparison. A related
but distinct phenomenon **is** visible in several raw proposals' own
evidence text as a cross-*field* observation, not a cross-*run*
disagreement: multiple runs independently flag `basecolor`/`color`/
`finish` as sharing near-identical vocabulary ("acacia", "acrylic", "aged
brass" appear in samples for all three), i.e. the *proposing model
itself* suspects real-world redundancy between these WANDS columns. This
is an alias/redundancy-merging question, not something the within-key
canonicalizer in §4 addresses (R10's alias union only merges aliases
*for the same real key*, never merges two different real keys) — recorded
here as an explicit scope boundary: E2c does not attempt cross-field
alias/redundancy merging, only within-key canonicalization.

**A second, larger instability pool E2b's own metric never counts at
all**: 158 additional pairs (12.64% of all 1,250 — nearly as large as the
155 *measured* disagreeing pairs) show `scope` disagreement **while role
and primitive fully agree** — invisible to the official role+primitive-only
metric entirely. Concretely: `overallproductweight`,
`overallwidth-sidetoside`, `overallheight-toptobottom`,
`overalldepth-fronttoback`, `weightcapacity`, `dswoodtone`, and
`samplepartnumber` each show **perfect** role/type/primitive agreement
across every one of their 10 within-config pairs, and their *entire*
measured instability is a Product-vs-Variant scope coin flip. Root cause,
confirmed structurally (not inferred): WANDS as ingested by
`e2b_ingest::build_catalog` has no real per-row Variant concept at all
(§4 R6) — nothing in the bounded statistical input given to any proposing
pass could possibly disambiguate Product from Variant scope, so this is
not LLM unreliability, it is **irreducible ambiguity from a genuine
dataset limitation**, exactly matching E2b's own already-published
WANDS-qualification-audit finding (criterion 6, NOT ESTABLISHED) from a
different angle. If scope were included in a combined "any structural
field disagrees" metric, aggregate raw agreement would fall from 87.60%
to **74.96%** (937/1250 pairs with zero disagreement across all four
compared fields) — reported here as new, disclosed context for how much
worse *un*-canonicalized instability really is once scope is counted, not
as a retroactive change to E2b's own already-committed 87.60% number.

## 7. Datasets, splits, and adversarial fixtures

**Reused, frozen, unmodified**: all 20 `dataset_cache/export/e2b_llm_proposals_*.json`
artifacts (4 configs x 5 runs), `e2b_oracle.rs`'s 53-descriptor hand-authored
ground truth (WANDS 36 + automotive 17), and the same
`UnifiedFieldStats`/`FieldStats` computation E2b's own validator already
uses. Per §1, there is no genuinely blind held-out subset of this data;
§7's reporting split (canonical configs vs perturbed configs) is a
generalization check, not a blind test.

**New, hand-authored, deterministic adversarial fixtures** (not
LLM-sourced — authored directly against the required-adversarial-cases
list in Issue #45's own text, each fixture's *expected safe outcome*
written before the canonicalizer code that must satisfy it, matching
CLAUDE.md's "add a failing test/benchmark first where practical"):
committed in `crates/issue42-eval/src/e2c_adversarial_fixtures.rs`,
covering: (1) same concept, different LLM labels/wording (synthetic
`CandidateDescriptor` sets with divergent `aliases`/evidence phrasing but
consistent stats); (2) same field name, different real type across two
synthetic "product families"; (3) Enum-vs-Numeric ambiguity (a fixture
deliberately shaped like `thread_size`); (4) Identifier-vs-high-cardinality-Enum
ambiguity; (5) Product-vs-Variant scope ambiguity with a fixture that
*does* have real measurable per-variant value variance (the one case R6
explicitly says it cannot handle from WANDS/automotive alone — this
fixture exists specifically to prove the canonicalizer's scope logic is
extensible, not to change R6's WANDS/automotive default); (6)
sparse-but-important attributes; (7) misleading field names; (8) units
and quantities with inconsistent formatting; (9) a relationship-like
field that must not collapse into an ordinary attribute (must abstain or
demote, never silently become `Enum`); (10) proposals that agree with
each other but are contradicted by real catalog statistics (a
majority-agrees-but-wrong case — this is where Treatment B is expected to
fail and C/D are expected to catch it via R8); (11) proposals where
majority vote would be unsafe (a synthetic 3-of-5-runs-propose-Relationship
case, directly testing R7 against Treatment B); (12) semantically
equivalent proposals choosing different but operationally equivalent
primitives (directly testing R1); (13) a genuinely unresolved case where
abstention is the only defensible result (contradictory evidence with no
tie-break rule that applies — must abstain under both C and D, never
fabricate an answer).

## 8. Types and code location (implementation boundary)

- `crates/issue42-eval/src/e2c_schema.rs`: `CandidateDescriptor` (type
  alias for `e2b_schema::Descriptor`), `CanonicalDescriptor` (the R1-R11
  output shape: `real_key`, `semantic_role`, `value_type`, `scope`,
  `supported_operators`, `aliases`, `retrieval_significance`,
  `candidate_physical_primitive`, `confidence`, `provenance:
  Vec<RunProvenance>` — one entry per contributing raw proposal, run
  index + its own role/primitive/confidence/evidence, never discarded),
  `CanonicalOutcome = Promoted(CanonicalDescriptor) | Abstain { reason:
  String, contributing_runs: Vec<u32> }`, `schema_version: u32 = 1`.
- `crates/issue42-eval/src/e2c_canonicalizer.rs`: R1-R11 (§4), pure
  functions of `(&[CandidateDescriptor], &UnifiedFieldStats) ->
  CanonicalOutcome`, one for Treatment C and a thin wrapper adding the
  majority-bar check for Treatment D.
- `crates/issue42-eval/src/e2c_majority_vote.rs`: Treatment B, kept
  structurally separate from the canonicalizer (different module) so it
  cannot accidentally share R1/R6/R7's logic — a real risk this protocol
  explicitly guards against, since sharing code would quietly make B not
  naive anymore.
- `crates/issue42-eval/src/e2c_compile.rs`: compiled-primitive pipeline —
  ingests `CanonicalOutcome::Promoted` descriptors into a real
  `commerce_core::domain::Catalog` (`Identifier`-role descriptors are
  additionally ingested as `AttributeValue::Text` so
  `CatalogIndex::build`'s own automatic `IdentifierClassifier` scan can
  reach them — E2b's own `accepted_typed_keys` structurally excludes
  `Identifier`/`Relationship` from ever being ingested at all, which is
  the reason `commerce_core`'s real identifier machinery was never
  exercised by any E2b baseline; this is a disclosed, deliberate, small
  extension of `issue42-eval`'s own ingestion helper, not a
  `commerce_core` production change), then builds a real,
  unmodified `commerce_core::index::CatalogIndex`.
- `crates/issue42-eval/src/e2c_metrics.rs`: raw/canonical/compiled
  stability (role, type, scope, primitive, full-descriptor-exact,
  matching Issue #45's own required breakdown, computed via the same
  leave-one-out pairwise design as §9), safety (unsafe-accepted count,
  reusing E2b's own corrected definition), recall, abstention rate,
  unstable->stable conversion rate, stable-but-wrong rate.
- `crates/issue42-eval/src/bin/e2c_canonicalization_eval.rs`: the
  reproducible entry point, one command, per CLAUDE.md's engineering
  quality gate.

## 9. Canonical-stability measurement design

Raw stability was measured pairwise across the 5 raw runs directly. A
canonicalizer consumes multiple raw runs at once, so "does canonicalizing
reduce instability" cannot be asked the same way by running the
canonicalizer once. Instead: for each (config, real key, treatment in
{C, D}), compute **5 leave-one-out canonicalizations**, each using 4 of
the 5 raw runs (dropping a different one each time) — then compute the
same `C(5,2)=10` pairwise agreement E2b's own stability metric already
uses, over these 5 canonical outputs instead of 5 raw proposals. This is
the direct structural analogue of E2b's own metric, letting raw-vs-canonical
stability be compared apples-to-apples at the same sample size and same
pairwise-comparison count (1250 total pairs, matching §6 exactly).
Treatment B is measured the identical way (5 leave-one-out majority
votes, pairwise-compared) for a fair three-way comparison. Treatment A
(raw) is E2b's own already-measured 10-pairs-per-config number, unchanged.

## 10. Metrics (Issue #45's own list, mapped to this implementation)

**Proposal layer** (unchanged from E2b, cited not recomputed): raw role/
type/scope/primitive agreement (§6 above extends E2b's own role+primitive
number with type and scope, computed by the same script), disagreement-
by-field breakdown (§6's table), disagreement concentration by semantic
class (§6's category column).

**Canonicalized descriptor layer**: canonical role/type/scope/primitive
agreement and full-descriptor exact agreement, all via §9's leave-one-out
design, for Treatments B/C/D; abstention rate (fraction of (config, real
key) canonicalizations that resolve to `Abstain` rather than `Promoted`);
unstable->stable conversion rate (of the 21 real keys §6 found raw-unstable,
what fraction become canonically stable — 100% leave-one-out pairwise
agreement — under C and under D); stable-but-wrong rate (of the canonically
*stable* keys, what fraction disagree with `e2b_oracle.rs`'s own hand-authored
role — the oracle is consulted only for this after-the-fact scoring step,
never during canonicalization itself, matching E2b's own oracle-independence
discipline).

**Safety and utility**: unsafe accepted hard classifications (E2b's own
corrected definition: an accepted/promoted descriptor whose oracle-confirmed
real role is `Identifier`/`Relationship`), retrieval-significant feature
recall (oracle-labeled retrieval-significant keys that end up `Promoted`),
accepted structural precision/recall vs oracle, oracle primitive agreement
(canonical primitive vs the primitive R1 would assign to the oracle's own
role — since R1 is deterministic, this reduces to oracle role agreement),
end-to-end Recall@10/NDCG@10/zero-result behavior (reusing
`e2b_ingest::naive_constraints_for_query` and `phase9_eval::wands_relevance`
exactly as E2b's own closure pass did, with the same disclosed
`e2e_check_reliable` caveat carried forward, not silently dropped),
compiled bundle size and serving overhead (P50/P95/P99 of
`indexed_candidates`/`execute_ranked`, reusing `e2b_serving_overhead_eval`'s
own measurement discipline — `bench_harness::round_robin_schedule`,
`REPS_PER_QUERY=30`, batched `black_box`, a pre-declared timer floor,
comparing the oracle-compiled bundle against Treatment C's and D's own
compiled bundles), canonicalization CPU/latency (wall-clock of the
canonicalizer binary itself over all (config, key) pairs — informational,
not gated, matching R3's own "reported, not disqualifying" precedent for
memory estimates), incremental recompilation behavior (informational: does
changing one raw proposal for one key change only that key's own canonical
output, never another key's — asserted by a unit test, not a full
incremental-index benchmark, which is out of this issue's own scope
boundary).

## 11. GO gate (frozen before any held-out measurement)

Per Issue #45's own proposed starting gate, adapted only where the
Reasoning is disclosed:

1. **Zero confirmed unsafe accepted structural classifications** (any
   treatment) — unchanged from Issue #45's own text.
2. **>=99% compiled physical-primitive agreement** across the 5
   leave-one-out canonicalizations, on `Promoted` (non-abstain) canonical
   descriptors, for Treatment C — unchanged.
3. **>=98% full canonical descriptor agreement** on `Promoted` descriptors,
   for Treatment C — unchanged.
4. **Retrieval-significant recall no more than 5 relative percentage
   points below** E2b's own LLM+validator path (Baseline 3, 86.84%) —
   unchanged; applies to Treatment C.
5. **No material relevance regression** vs E2b's own validated path on
   the same held-out queries (naive end-to-end check, same
   `e2e_check_reliable` caveat) — unchanged; applies to Treatment C.
6. **Serving overhead within the existing fast-path budget** — operationalized
   identically to E2b's own closure-pass criterion 5 (<=5% vs the
   hand-authored oracle bundle on P95/P99, with P50 correctly reported
   INCONCLUSIVE rather than rounded to PASS if it sits below this
   measurement's own pre-declared timer floor) — unchanged.
7. **Every unresolved conflict is either deterministically resolved from
   permitted evidence or explicitly abstained — no hidden last-writer/
   majority winner.** Operationalized as: Treatment C's own canonicalizer
   code contains no code path that silently picks "whichever run's proposal
   happened to be encountered first/last" without going through R1-R11;
   verified by a dedicated unit test asserting canonicalization output does
   not depend on input-vector order (shuffled-order fuzz test over the
   adversarial fixtures).

**GO** requires all seven to pass for Treatment C. Per Issue #45: "do not
assume D wins" — D's own results are reported against the same seven
criteria independently, and if D passes where C does not (most likely on
criteria 2/3/7 at the cost of criterion 4's recall), that is itself a
reportable, legitimate finding about the stability/recall tradeoff, not a
tie-break in either direction.

## 12. Falsification criteria (unchanged from Issue #45, restated for this checkpoint)

Record REVISE or STOP, not GO, if: canonicalization merely reproduces
majority vote (checked directly — Treatment C's output is diffed against
Treatment B's output per key; if they are identical on every promoted key,
R1-R11 are not adding anything beyond B); post-canonicalization stability
remains close to raw stability; high stability is achieved only by
rejecting most retrieval-significant features (checked via the recall gate,
criterion 4, applied jointly with the abstention rate — a treatment that
abstains its way to 100% stability on a shrunken accepted set fails this
even if criteria 2/3 pass); the canonicalizer converts unstable proposals
into *consistently wrong* structural semantics (the stable-but-wrong rate,
§10); unsafe hard classifications appear; oracle information leaks into
compilation (checked by grep: no canonicalizer or compiler function may
import or reference `e2b_oracle`); canonicalization requires another
unconstrained LLM call (it does not, by construction — R1-R11 are pure
Rust functions over already-frozen data); the internal semantic vocabulary
grows into a generic document/schema system (checked against §3 — zero new
enum variants were added); physical primitive choice is still materially
controlled by free-form model wording (checked against R1 — it is a fixed
function of role, never of `evidence` text or `aliases`).

## 13. What this issue does not authorize (restated from Issue #45's scope boundary)

No E4/E5/E6 implementation. No R1b corroboration-optimization work. No
production compilation of E2c's canonical descriptors into
`commerce_core`'s actual serving path (E2c's `CatalogIndex` builds are
experimental, evaluation-only, exactly matching E2b's own
`e2b_serving_overhead_eval` precedent — real `commerce_core` code,
exercised from an experimental crate, never modified). No claim of real
Product/Variant/relationship generalization beyond what R6/§7's fixture 5
already discloses as a synthetic extensibility check, not a real-feed
result. No prompt-tuning of the LLM to raise raw agreement — the 20 frozen
artifacts are used exactly as E2b produced them, unmodified, no new LLM
calls of any kind in this experiment.

## Addendum 1 (dated, after the first held-out run and a fresh three-reviewer adversarial review, per this document's own section 1 commitment: "no rule edited after seeing a measurement result unless this document is amended with a dated note explaining why")

Three independent reviewer agents, none with an implementation mandate
and none shown this document's own conclusions in advance, converged
strongly (raw findings preserved at
`docs/research/artifacts/i45_e2c_adversarial_review_run1/reviewer_{A,B,C}.md`).
All three independently confirmed one real implementation defect; all
three independently ruled out oracle leakage; all three independently
found the same substantive nuance about which rules do the real work.

**Confirmed defect, fixed**: R3/R9's original condition
(`e2c_canonicalizer.rs`) checked `cross_run_type_conflict` across every
pair of raw proposals regardless of whether either side was R2's own
plurality winner. A real plurality (e.g. 3 of 5 raw proposals agreeing
on `FreeText`) could be silently overwritten by R3 to `Enum`/`Numeric`
whenever an unrelated 1-Enum + 1-Numeric minority pair happened to
coexist, discarding the genuine majority. All three reviewers
independently verified this did not corrupt any of the 20 real frozen
artifacts' own measured numbers (confirmed a fourth time here,
independently, by re-running both eval binaries after the fix: every
number is byte-identical to the pre-fix run —
`docs/research/artifacts/i45_e2c_canonicalization_run2/` and
`i45_e2c_serving_overhead_run2/` vs the original `_run1/` directories,
diffed and confirmed unchanged except for measurement-inherent latency
jitter in the serving-overhead binary). Fixed by scoping R3/R9's
engagement to only the raw proposals that actually contest R2's own
plurality winner (`e2c_canonicalizer.rs`'s own updated doc comment has
the full before/after). A new regression test,
`r3_does_not_hijack_a_plurality_winner_unrelated_to_its_own_conflict`,
reproduces the exact confirmed scenario.

**Confirmed documentation defect, corrected**: this document's own
"Explaining the 87.60%" table (§6) claimed the Enum-vs-FreeText half of
the "value-type ambiguity" category (`productwarranty`, `warrantylength`)
is "resolved by R3." This is false: `cross_run_type_conflict` — the sole
gate on R3's every engagement — only ever fires Enum/Boolean-vs-Numeric,
structurally incapable of touching an Enum-vs-FreeText disagreement.
Corrected in place in §6, original claim preserved and struck through,
per rule 9.

**A genuinely important, humbling finding, not a defect but a
correction to this document's own explanatory emphasis**: a new
diagnostic (`bin/e2c_r1_r6_attribution_diagnostic.rs`, output preserved
at `docs/research/artifacts/i45_e2c_r1_r6_attribution_run1/`) measured
directly, on the real frozen data, whether R3's evidence-based
resolution ever changes an outcome from what plain R2 plurality alone
would already give. Result: `cross_run_type_conflict` fires on exactly
**1 of 125** (config, real-key) groups across the entire dataset
(`automotive/thread_size`), and even there R3's resolved role is
**identical** to plain plurality's own answer (a real 4/5 Enum
majority). R3 — the rule this document's own §4 and §6 present most
prominently as the mechanism that "resolves" Enum-vs-Numeric
disagreement — is empirically inert on this specific dataset: it never
once changes a real outcome. R4 (zero-variance override) is similarly
vote-concordant in its own showcased case (`voltage`: raw primitive
votes 2 `numeric_range` / 3 `none`, a plurality already picks `none`).

This does **not** mean Treatment C merely reproduces Treatment B (naive
majority vote) under a different name — role-level stability is nearly
identical between them (B=99.68%, C=100.00%, leave-one-out), but
full-descriptor stability is not (B=81.68%, C=100.00%), and that gap is
real, measured, and not an artifact of R3/R4's near-zero engagement:

- **R1** (primitive is a deterministic function of role, never voted)
  provably accounts for the bulk of the primitive/full-agreement gap —
  Treatment B votes `candidate_physical_primitive` directly and gets it
  wrong exactly on the `bitmap_enum`/`lexical_postings` coin-flip cases
  §6's own primitive-selection-ambiguity category names; R1 makes that
  disagreement structurally impossible once role is stable. This is a
  genuine, non-vote-derived, non-reducible-to-plurality mechanism.
- **R6** (scope defaults to `Product` for a dataset with no real
  per-row Variant identity) accounts for the rest of the full-agreement
  gap. This is disclosed, in this document's own §4 and §6, as a
  dataset-structural default, not an evidence-integration result — it
  is tautologically 100% stable by construction (it never varies), not
  an empirically resolved conflict. The distinction matters: R6 is a
  legitimate, principled, disclosed architectural choice that correctly
  eliminates an irreducible ambiguity (§6's own 158-pair scope pool, which
  no vote of any kind could resolve, since the bounded inputs genuinely
  contain zero disambiguating signal) — but its contribution to the
  "100% full agreement" headline should be read as "a structural default
  correctly applied," not "evidence-based conflict resolution," and this
  document's own earlier framing did not sufficiently distinguish the two.
- **R5** (Identifier promotion gate) and **R7** (Relationship hard-block)
  are real, demonstrated, non-vote-derived safety mechanisms: both
  prevented an actual unsafe-shaped promotion Treatment B's own naive
  vote would have made on this real data — R7 blocks `color`'s spurious
  `Relationship` hallucination (Treatment B, run with a constructed
  3-of-5-vote share in this document's own §7 fixture 11, promotes it
  unconditionally); R5 blocks `compatibledrainassemblypartnumber`'s
  Identifier claim at a real uniqueness ratio of 0.4845, far below the
  production classifier's 0.95 bar, regardless of vote count.
- **R2** (plain plurality) does almost all of the real work at the
  *role* level specifically — the 0.32-percentage-point role-stability
  gap between B and C is not large. Treatment C's genuine differentiation
  from naive voting comes from R1+R5+R6+R7 (structural/safety mechanisms
  that do not depend on vote counting at all), not from R2/R3 being
  meaningfully "smarter" arbitration of the same votes B already counts.

**A disclosed, non-preregistered addition**: the "single-run (stricter
self-check)" comparison (§9 of this document describes only the
leave-one-out design; the single-run comparison was added during
implementation, after the first leave-one-out result — 100% full
agreement — looked too clean to trust without a harder test). This is
disclosed here as exactly what it is: a post-freeze addition, not a
preregistered measurement, and therefore **not** part of this
experiment's own formal GO-gate criteria 2/3 evaluation in the same
sense the leave-one-out numbers are. It was added and run once, before
being reported, with no iteration on its own design after seeing its
result (95.20%, both times it has been run, pre- and post- the R3 fix).
It is reported here not because the protocol required it, but because
omitting a harder test that was already run and already known would
itself be a disclosure failure. The final verdict below treats the
single-run reading as material evidence, explicitly not preregistered,
weighed alongside the preregistered leave-one-out reading — not as a
silent substitute for it either direction.

**Two findings the reviewers raised as real but did not find exploited
in this run, recorded for completeness**: (1) `pairwise_stability`
counts two `Abstain` outcomes as agreeing on every axis, which is
gameable in principle; measured directly (§ above diagnostic), only
2 of 27 raw-unstable (config, key) groups stabilized via
Abstain-Abstain rather than genuine Promoted-Promoted agreement — not
the mechanism behind this run's headline numbers. (2) GO-gate criterion
5's boolean does not itself consult `e2e_check_reliable`
(`check_reliable=false` in this run, inherited unchanged from E2b's own
same near-floor-NDCG limitation) — the decision record below treats
criterion 5 as PASS-with-an-unreliable-check-caveat, never as an
unqualified pass, matching how `ISSUE42_DECISION.md` already treats the
identical inherited caveat.

Full workspace `cargo fmt`/`clippy`/`test`/`build` re-run clean after
the R3 fix — see the completion bar at the end of this checkpoint's own
commits.
