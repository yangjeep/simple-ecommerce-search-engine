# Issue #55 A1 — a candidate/RIB → PROMOTE/UNRESOLVED gate for ProductType hyponym relations

Full log entry: `docs/experiments/ISSUE55_SEMANTIC_PROMOTION_LOG.md`.

## Governing directive

A repository-owner closure directive on PR #56 (also posted on Issue
#55 itself) required, before any further work on that PR: "explicit
candidate/RIB → validation/adjudication → PROMOTE/REJECT/UNRESOLVED →
FIB lifecycle for inferred ProductType relations; the known `beds ->
cat beds / dog beds & mats` relation must no longer be auto-installed
as a hard default semantic route." This is A1.

## The defect this closes

`product_type_hyponym_groups` (`cold_start/profile.rs`) is, and always
was, a pure syntactic *candidate* generator — a real "RIB" in this
project's own terms: every whole-word-superset pair it produces is
nothing more than an unvalidated hypothesis. But `compile_lexicon`
(and its 3-arg predecessor `compile_lexicon_with_product_type_hyponyms`)
installed **every** candidate that function produced as a live
`ProductTypeAny` serving route, unconditionally, whenever
`enable_product_type_hyponyms` was `true` — which `compile_lexicon`
itself always passed. There was no gate between "syntactically valid
candidate" and "hard filter route" at all.

This is exactly how the confirmed, previously-disclosed cross-family
false positive `"beds"` → `"cat beds"`/`"dog beds & mats"`
(`ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`,
`ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md`) shipped as default
production behavior: it is one of the 149 live groups /
317 pairs `product_type_hyponym_groups` produces on real WANDS data,
and had no promotion gate to catch it before `compile_lexicon` wired it
into a hard `StructuralConstraint::ProductTypeAny`.

## The fix

A new module, `control_plane::hyponym_promotion`
(`crates/commerce-core/src/control_plane/hyponym_promotion.rs`),
deliberately reuses `control_plane::implication`'s own
`RuleProvenance`/`RuleStatus` (`Candidate`/`Promoted`/`Withdrawn`,
evidenced by `Catalog`/`QueryLog`/`Behavioral`/`Model`/`Manual`
provenance) rather than inventing a parallel lifecycle — the shape is
identical, just relating two product-type names instead of a trigger
phrase to an implied fact:

- `HyponymRelation { broader, narrower, provenance, confidence, status }`
  with `.candidate()`/`.promote()`/`.withdraw()` builders (names
  lowercased on construction).
- `PromotedHyponyms { version, pairs }`, compiled via
  `PromotedHyponyms::compile(version, relations)`, which **silently
  drops any `Candidate` or `Withdrawn` relation** — mirroring
  `ImplicationTable::compile`'s own structural guarantee that an
  unvalidated or retracted rule is structurally incapable of reaching
  `.contains()`, not merely excluded by convention at each call site.
  `Default` is the empty set.

`cold_start::profile::compile_non_brand_lexicon` now filters every
syntactic candidate `product_type_hyponym_groups` produces through
`promoted_hyponyms.contains(broader_name, narrower_name)` before it may
contribute to a `ProductTypeAny`; a broader type with zero *promoted*
narrower IDs falls back to plain `ProductType` matching, exactly as
"hyponym expansion disabled" always has.

`compile_lexicon`'s public 2-argument signature is **completely
unchanged** (60+ existing callers across the workspace are unaffected
at the call site), but its internal default is now
`&PromotedHyponyms::default()` — the empty set — instead of
unconditionally trusting every syntactic candidate. **This is the
actual fix**: a catalog with no recorded PROMOTE verdict now gets safe,
per-id `ProductType` matching only, by construction, not by a
special-cased exclusion of the two known-bad pairs.

The old 3-arg `compile_lexicon_with_product_type_hyponyms(profile,
min_enum_frequency, bool)` is renamed to
`compile_lexicon_with_promoted_hyponyms(profile, min_enum_frequency,
&PromotedHyponyms)`, so evaluation tooling can still build a
"treatment" lexicon from an explicit, inspectable promotion set instead
of an opaque boolean. Its one external caller
(`issue55-eval`'s `i55_e14_paired_comparator_freeze`, which measures
the leaf-only hyponym *expansion mechanism* itself, not promotion
adjudication) is updated to use a new, deliberately-unsafe-by-name
helper, `promote_all_hyponym_candidates_unadjudicated`, that reproduces
the old unconditional-auto-install behavior explicitly and only for
that one frozen, pre-A1 comparator experiment — never exposed as
anything resembling a production default.

## Verification

- 5 new unit tests for `hyponym_promotion` (candidate never reachable
  via `contains`; promoted relation reachable, case-insensitively;
  withdrawn-after-promoted never reachable; default is empty; unrelated
  pairs never leak).
- Two existing `profile.rs` unit tests updated (not silently preserved
  under a misleading name): `promoted_hyponyms_empty_matches_compile_lexicon`
  proves the empty-set path is still byte-identical to `compile_lexicon`;
  `default_promoted_hyponyms_never_produces_product_type_any` proves the
  default never emits `ProductTypeAny` even where a genuine syntactic
  candidate exists, and that explicitly promoting that exact relation is
  what activates the expansion — demonstrating the mechanism via a real
  promotion, not merely asserting the negative.
- One existing end-to-end integration test in
  `crates/commerce-core/tests/cold_start.rs`
  (`clean_whole_word_subset_product_types_now_merge_via_leaf_only_hyponym_expansion`)
  asserted the *old*, now-incorrect default behavior (plain
  `compile_lexicon` on a clean "Boots"/"Hiking Boots" pair produces
  `ProductTypeAny`) and failed immediately after the fix, exactly as
  expected. Rewritten as
  `clean_whole_word_subset_product_types_require_explicit_promotion_to_merge`:
  first asserts the production default now resolves `"boots"` to its own
  type only, then explicitly promotes the relation and reproduces the
  original expansion assertion against a `compile_lexicon_with_promoted_hyponyms`
  lexicon built from that real `PromotedHyponyms`.
- Full workspace quality gate rerun clean: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features` (all green, no other test
  anywhere in the workspace depended on the old auto-install default),
  `cargo build --workspace --release`.

## What this deliberately does NOT establish, disclosed rather than smoothed over

- **This closes the gate, not the oracle.** As of this commit, current
  production has **zero** `ProductTypeAny` hyponym expansions active —
  `compile_lexicon`'s default `PromotedHyponyms` is empty, and nothing
  in this checkpoint populates it with any real promoted relations. The
  previously-measured recall gains from the leaf-only hyponym mechanism
  (checkpoint 14/22: +24.06pp candidate-set recall pre-leaf-fix, the
  "recliners" win, 79/149 reachability-audit-confirmed groups) are
  **not currently realized in production** until Issue #55 A2 supplies
  a real, adjudicated `PromotedHyponyms` set and something wires it into
  whatever compiles the live `SemanticContext`.
- **This tradeoff is deliberate and directive-mandated, not a hidden
  regression.** It is exactly what the owner's directive asked for, and
  matches this project's own stated severity asymmetry ("a promotion
  error is substantially more serious than leaving a relation
  unresolved" — Issue #55's own text): falling back to the pre-
  checkpoint-14 safe baseline is the correct default while no
  adjudicated evidence exists, not a defect to apologize for.
- **Several existing eval/diagnostic binaries will report different
  numbers if rerun now**, with no code defect on their part — they were
  built to measure the hyponym-expansion mechanism under the old
  unconditional-auto-install default, which no longer describes
  `compile_lexicon`'s behavior: `phase9-eval`'s `p9_e02_wands_physical_advantage`,
  `p9_e03_lexicon_coverage_diagnostic`, `p9_e07_ambiguous_routing_diagnostic`,
  `p9_e08_hyponym_group_false_family_audit`, `i55_hyponym_reachability_audit`,
  `i55_hyponym_candidate_set_export`; `issue55-eval`'s
  `i55_e15_automotive_hybrid_gap_probe`. None of these are broken or
  produce wrong answers — they now correctly report the mechanism as
  fully gated off by default, which is a real, disclosed consequence of
  this fix, not a bug in those binaries. Reconciling their own doc
  comments/headline claims with this new default is folded into A4
  (root/architecture doc refresh), not fixed ad hoc here.
- **Not a claim that the underlying candidate-generation mechanism
  (`product_type_hyponym_groups`, leaf-only restriction) changed at
  all.** It did not; every existing pure-function-level test of that
  generator (`cold_start::profile::hyponym_tests`) is untouched and
  still passes.
- **Not A2.** No adjudication oracle was built or run here; the two
  updated `profile.rs`/`cold_start.rs` tests use small, hand-constructed
  `PromotedHyponyms` fixtures to prove the *mechanism* works, not a real
  adjudicated set for the live WANDS candidate pool. That is the next,
  separate piece of work this same directive names.

## Next step (named, not implemented here)

A2: build a credible, auditable promotion oracle/adjudication set
(positives, negatives, ambiguous/unresolved, reachable triggers) for
the full live 149-group/317-pair WANDS candidate set, reusing the
already-validated ancestor-structure category-hierarchy-overlap
evidence source (`ISSUE55_PROMOTION_GATE_FULL_SET_DECISION.md`'s named
next step, confirmed GO in the unnamed follow-up row directly below it
in `docs/decisions/README.md`: 67.6%/65.6% recall, zero false
promotions against the two known-bad pairs) as the actual PROMOTE/
UNRESOLVED signal, persisted as a structured, durable, individually-
justified, reviewable artifact — not inferred from only the two
inherited known-bad pairs, per the directive's own explicit
instruction. Only once that adjudicated `PromotedHyponyms` set exists
does wiring it into whatever builds the live `SemanticContext` become
meaningful; this checkpoint intentionally stops short of that so the
gate (A1) and the oracle (A2) are each independently reviewable.
