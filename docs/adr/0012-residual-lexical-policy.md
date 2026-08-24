# ADR 0012: Residual-lexical policy — a classify-then-selectively-bypass fallback for the strict residual veto

## Status

Accepted (Issue #42 R2, `docs/experiments/ISSUE42_LOG.md#i42-r2`).

## Context

ADR 0009 established `commerce_core::plan::execute_planned`'s three-outcome
execution contract (`FastPath`/`Hybrid`/`Punt`) and its unconditional
correctness rule: every delegate hit is re-verified against
`CommerceQuery::matches_variant` before being returned. That verification
rule has a sharp edge ADR 0009 did not resolve, because the mechanism to
fix it did not exist yet: for `Hybrid`/`Punt`, when the
[`plan::LexicalDelegate`] finds literally zero raw hits for
`query.residual_lexical`, `verify_and_truncate` necessarily has nothing to
verify, so the query collapses to zero results — with no distinction
between "the residual text is a broadly generic descriptive word the
delegate simply never indexed for this narrow slice" (should not veto a
real, matching structural candidate set) and "the residual text is a
specific, disqualifying attribute value" (should veto). Every non-empty
residual with zero raw hits was treated identically, regardless of which
case it actually was.

Issue #42 R2 (`docs/experiments/ISSUE42_LOG.md#i42-r2`) measured this
directly: Treatment A (today's unmodified `execute_planned`) recovers 0 of
4 preregistered benign rows on a purpose-built fixture where a real,
matching structural candidate set exists but the delegate's restricted
search for the residual word finds nothing. R2 also measured two
unconditional fixes (Treatments B/C — always fall back to the structural
set, or always treat residual text as advisory-only) and found both
over-recover: 3 of 3 adversarial rows are indiscriminately recovered too,
including a case where the residual word is real and genuinely
disqualifying. Treatment D — compile a `ResidualPolicy` once per catalog
(ingestion time, not query time) classifying every residual token by
cross-type breadth, and consult it only when a raw-empty `Hybrid`/`Punt`
result has a corroborating `ProductType` constraint — recovered all 4
benign rows and rejected all 3 adversarial rows, clearing every
preregistered R2 GO-gate criterion (benign recovery, adversarial
false-recovery, zero query-time model calls) after two independent rounds
of adversarial review found and fixed one real defect in Treatment D
itself (below). This ADR records the production port of that mechanism.

`commerce_core::admission.rs`'s `admit`/`admit_lexically_narrowed`/
`admit_structurally_anchored_lexical` strict-veto functions are a
separate, structurally unrelated decision (native-vs-Solr-fallback
admission, evaluated before `execute_planned` is ever reached) and are
explicitly out of scope: R2's evidence says nothing about that module, and
this change does not touch it.

## Decision

**A new `plan::residual` submodule compiles a `ResidualPolicy` once per
`Catalog`, at ingestion time, never at query time.** It scans every
product's title words plus every registered `Enum`/`MultiEnum` attribute
value and true-`Boolean` attribute name, recording which `ProductTypeId`s
each lowercased token is observed under anywhere in the catalog. A token
observed under `CROSS_TYPE_BREADTH_THRESHOLD = 2` or more *distinct*
product types classifies `ResidualClass::Preferred` (a broadly-used,
generic word); anything else — never observed at all, or observed under
fewer than 2 other product types — classifies `ResidualClass::Required`
(the safest default: zero or narrow positive evidence keeps today's veto).
This is a structural fact about the catalog, computed with zero model
calls, matching CLAUDE.md's "No LLM/model call in the default query hot
path."

**`execute_planned` gains one new, additive, trailing parameter:
`residual_policy: Option<&ResidualPolicy>`.** `None` reproduces today's
exact behavior in every case, byte-for-byte — every existing call site in
the workspace (`phase2-eval`, `phase9-eval`, `issue38-e2e3-eval`,
`issue42-eval`, and `commerce-core`'s own test suite) passes `None` and is
otherwise untouched. When `Some`, a `Hybrid`/`Punt` outcome whose delegate
returns zero raw hits can bypass the strict veto and fall back to the
structural candidate set (ranked via `index.execute_ranked`, the same
signal `FastPath` already uses) instead of returning empty — but **only**
when a small, private helper (`residual_fallback_hits`) confirms all four
of:

1. the delegate's raw hits are empty;
2. `residual_policy` is `Some` (the caller opted in);
3. a *corroborating* `ProductType` constraint is present in
   `query.constraints` (checked by a new private
   `corroborating_product_type` helper);
4. **every** token in `query.residual_lexical` classifies
   `ResidualClass::Preferred`.

Any `Required` token, or no corroborating `ProductType` constraint at all,
keeps today's exact `verify_and_truncate` call and today's exact (empty)
output. This mirrors R2's own winning Treatment D precisely, with one
deliberate implementation deviation from the eval prototype: the eval
version's `ResidualPolicy::classify` accepted an unused `_product_type`
parameter; production drops it from the signature entirely
(`classify(&self, token: &str) -> ResidualClass`), since the eval
prototype's own doc comment (and a dedicated regression test) confirms it
never affected any classification decision. The "is there a corroborating
`ProductType` constraint at all" check still happens — it is
`corroborating_product_type`'s job in the *caller*, not something
`classify` itself evaluates.

**This decision rests on two rounds of adversarial review, not one team's
self-assessment.** Per Issue #42's "do not trust the experiment author"
governance, a fresh reviewer with no implementation task in R2's own
session history independently tried to falsify the protocol, fixture,
code, arithmetic, and every written claim before any production change was
made on the strength of R2's GO verdict (`docs/experiments/ISSUE42_LOG.md#i42-r2`'s
"Second correction round"). That review found a real, confirmed defect:
`ResidualPolicy::classify`'s original logic also treated "observed
anywhere under the query's own product type" as sufficient evidence a
token was safe to bypass — untested by R2's original 8-row workload, which
only ever compiled a single `ProductType` constraint. A direct
reproduction (`compile("velvet blue sofas", ...)`, a compound
`ProductType(Sofas) AND Enum(color=Blue)` constraint whose real candidate
set is one product only) showed Treatment D recovering the *wrong*
variant — a genuine false positive no adversarial row could otherwise
catch. The fix — removing that condition entirely, keeping only
cross-type breadth as the signal — is exactly what this ADR's Decision
describes above, and is why `classify` never inspects the query's own
product type at all, compound or otherwise. A second finding (a real,
previously-total test-coverage gap on Treatment C's only distinguishing
code path) was also found and fixed in that same round. Both findings
were independently reproduced before being accepted; production adopts the
corrected mechanism, not the original one.

## Consequences

- `commerce_core::plan` gains one new submodule (`residual`, holding
  `ResidualPolicy`/`ResidualClass`) and re-exports both from `plan` itself.
  No new external dependency: the module uses only `commerce_core`'s own
  `domain` types.
- `execute_planned`'s signature changed (one new trailing parameter),
  which the compiler used to find every affected call site. **34**
  existing call sites across 11 files were migrated to pass `None` — a
  compile-fix only, zero behavior change, since `None` is provably
  equivalent to the prior signature everywhere:
  `crates/phase2-eval/src/bin/planner_integration_eval.rs` (1 site),
  `crates/phase2-eval/src/bin/punt_path_adversarial_eval.rs` (2 sites),
  `crates/phase2-eval/src/bin/alias_enforcement_eval.rs` (1 site),
  `crates/phase2-eval/src/bin/p1d_physical_advantage_eval.rs` (3 sites),
  `crates/phase2-eval/src/bin/prefill_eval.rs` (1 site),
  `crates/issue38-e2e3-eval/src/bin/e2_unseen_vertical_eval.rs` (2 sites),
  `crates/issue38-e2e3-eval/src/bin/e3_mixed_category_eval.rs` (2 sites),
  `crates/phase9-eval/src/bin/p9_e02_wands_physical_advantage.rs`
  (2 sites), `crates/issue42-eval/src/r1_experimental.rs` (1 site),
  `crates/commerce-core/tests/plan.rs` (10 sites), and
  `crates/issue42-eval/src/r2_experimental.rs` (9 sites — R2's own
  already-committed, already-verified evidence artifact, included in this
  same compile-fix-only category: only its internal `execute_planned`
  calls were touched to keep it compiling, with zero change to its logic,
  tests, or doc comments — `cargo test -p issue42-eval --release`
  confirms every one of its existing tests still passes unchanged). Sum:
  1+2+1+3+1+2+2+2+1+10+9 = 34, confirmed by direct grep, not merely
  asserted (a fresh adversarial review of this production merge found the
  original version of this paragraph undercounted the total by more than
  2x while still correctly stating "11 files" — a real disclosure defect,
  independently reproduced and fixed here; every one of the 34 real sites
  was independently re-verified to actually pass `None`).
- A new `crates/commerce-core/src/fixtures.rs` fixture
  (`residual_policy_catalog`) and four new regression tests live directly
  in `plan::mod`'s own `#[cfg(test)]` block, alongside
  `restrict_to_independently_excludes_a_constraint_satisfying_hit` — the
  all-Preferred recovery case, the `residual_policy: None` byte-identical
  case, the Required-token veto case, and the
  no-corroborating-`ProductType` case.
- No new production dependency anywhere in this change.

## Alternatives considered

- **Corroborate against the bare `ProductTypeId` a query names, without
  narrowing for any *other* constraint present in the same query.**
  Rejected: this is exactly the defect Issue #42 R2's own second
  adversarial-review round found and fixed in `ResidualPolicy::classify`
  itself (see Decision above) — a compound constraint's real candidate set
  can be a strict subset of "every product of this type," and cross-type
  breadth (not "observed under this bare type") is the signal that stays
  correct regardless of how narrow the query's actual candidate set is.
  `corroborating_product_type` deliberately only answers "does a
  `ProductType` constraint exist at all," leaving all narrowness handling
  to `classify`'s own cross-type-breadth signal, not to a second,
  redundant narrowing check in the caller.
- **Make `residual_policy` a required parameter rather than an
  `Option`.** Rejected: this would force every existing caller —
  `phase2-eval`'s and `phase9-eval`'s benchmark binaries, `issue38-e2e3-eval`'s
  fitment-generalization evaluations, `issue42-eval`'s own R1/R2
  experimental harnesses, and `commerce-core`'s own test suite — to compile
  and thread a `ResidualPolicy` through code paths this fix is entirely
  irrelevant to, or to construct one over a throwaway catalog just to
  satisfy the signature. An `Option`, defaulting to `None`, keeps every
  unaffected caller's code and behavior untouched while still making the
  new mechanism available to whichever caller opts in.
