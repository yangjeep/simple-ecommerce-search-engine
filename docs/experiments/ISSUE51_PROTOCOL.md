# Issue #51 Preregistered Protocol — precomputed typed corroboration (Treatment E)

Committed before any Treatment E code is written or measured, per this
repository's governance and Issue #51's own "Done when: protocol is
preregistered" requirement.

## 0. What this is testing

Issue #42's R1 (`docs/experiments/ISSUE42_LOG.md#i42-r1`,
`docs/decisions/ISSUE42_DECISION.md`) found Treatment D passes every
correctness/wrong-family/fallback criterion for corroborated typed-
ambiguity resolution (corroborated mean NDCG@10 = 1.0000, 0 wrong-family
false positives) but fails the preregistered <=5% latency-overhead bar
(13.6%-17.8% measured, later reproduced at 10.2%-18.0% across a combined
run set), attributable to `constraint_kind_registered_on_product_type`'s
O(catalog-size) per-query scan
(`crates/issue42-eval/src/r1_experimental.rs:127-159`, `catalog.products.iter().filter(...).any(...)`
called at query time for every ambiguous constraint). Issue #51 asks: can
the *same* corroboration semantics be preserved while moving that scan to
ingestion/compile time, so query-time lookup is O(1)?

## 1. Hypothesis

**H0 (fixable)**: a registry `(ProductTypeId, attribute) -> Set<AttrKind>`,
built once from the catalog before any query runs, and consulted via a
single `HashMap` lookup at query time, reproduces Treatment D's exact
decision (same numeric_registered/matching_alts logic, same inputs, same
outputs) while eliminating the query-time catalog scan, bringing overhead
under the 5% bar. **H1 (not fixable at this scope)**: the registry
construction itself, or some other cost inside `resolve_d`'s broader
logic (not just the flagged scan), dominates measured overhead, so this
change does not clear the bar either — a genuine negative result, not
assumed away.

## 2. Baseline

Current branch HEAD. Treatments A/B/C/D (`crates/issue42-eval/src/r1_experimental.rs`)
are **not modified** — they remain the frozen historical reference R1
measured. A new function `build_attribute_kind_registry` and a new
treatment `resolve_e` are added alongside them (new code only, matching
Issue #51's own "implemented behind an experimental boundary"
requirement — `crates/issue42-eval` is itself an eval crate, not
`commerce_core`). `commerce_core::ir::query::compile` and
`commerce_core::plan::execute_planned` are called exactly as-is by E too,
matching every other treatment's own discipline.

## 3. Dataset

The exact same frozen R1 fixture: `issue42_eval::r1_workload::build_typed_ambiguity_catalog`
(5 products: Jeans/Wiper Blades/Brake Pads plus price/identifier
regression guards) and the exact same 9-row query workload
(`crates/issue42-eval/src/bin/r1_typed_ambiguity_eval.rs` rows 1-10,
row 8 already absent in the original). No dataset change — this
experiment is purely a mechanism change, matching Issue #51's own "carry
forward R1's original correctness requirements... do not relax them."

## 4. Treatment

**Treatment E** (new): identical decision logic to Treatment D
(`resolve_d`) — same fallback to demotion when there is no corroborating
`ProductType` constraint or when corroboration does not disambiguate
cleanly — but `constraint_kind_registered_on_product_type(catalog,
product_type, constraint)`'s per-call linear scan is replaced by a single
`HashMap<(ProductTypeId, String), HashSet<AttrKind>>` lookup against a
registry built **once**, outside the measured per-query path, from the
same catalog Treatment D scans live. `AttrKind` is a plain 5-variant
enum mirroring `AttributeValue`'s discriminant (Enum/MultiEnum/Boolean/
Numeric/Text) — no new generic schema/DSL, matching Issue #51's explicit
prohibition.

The registry-build step is timed and reported separately (an ingestion/
compile-time cost, not part of the <=5% *query-time* serving-overhead
bar Issue #51's gates apply to) — disclosed, not hidden, even though it
does not count against the gate.

## 5. Metrics

Identical to R1's own harness (`r1_typed_ambiguity_eval.rs`, reused
directly, not reimplemented): per-treatment wrong-family false-positive
count, row-1 silent-single-family check, negative-row hard-constraint
checks, corroborated-row mean NDCG@10, and latency (median of 7
independent `black_box`-guarded batched trials, matching R1's own
methodology exactly since a single-batch measurement was found to hit
the timer floor). Treatment E's registry-build time, separately.

## 6. Preregistered gates (identical to R1's own, per Issue #51's instruction to carry them forward unchanged)

- zero known wrong-family hard-filter false positives;
- corroborated queries recover the intended typed interpretation
  (mean NDCG@10 >= 0.95, R1's own bar);
- genuinely ambiguous uncorroborated queries do not fabricate a unique
  meaning (row 1 check);
- fallback/demotion behavior remains safe (negative-row checks);
- query-time serving overhead <=5% vs. Treatment A (the frozen baseline),
  measured above the timer floor (R1's own median-of-7-batched-trials
  discipline, reused unchanged);
- no query-time LLM inference (trivially satisfied — no treatment here
  or in R1 ever called a model).

**GO**: Treatment E passes every gate above. Recorded as a candidate for
a future production-compilation decision (not itself production
integration — Issue #51 explicitly scopes this experiment to a GO/REVISE/
STOP recommendation, not a merge).
**REVISE**: E passes correctness but still fails the overhead bar, or
passes overhead but a correctness gate regresses versus D — a genuine,
disclosed negative or partial result, not massaged into a GO.
**STOP**: E fails correctness gates D already passed (would indicate the
registry either misrepresents the catalog or the refactor introduced a
behavioral divergence from D — a defect in this experiment's own
implementation, not evidence against the underlying corroboration
mechanism, which R1 already established works when unconstrained by
cost).

## 7. Scope boundary (per Issue #51's own text)

Does not fold into E2d/Issue #47. Does not modify the E2c canonicalizer/
model-comparison experiment. Does not add a generic dynamic schema/query
DSL — the registry is a fixed-shape lookup table over a closed 5-variant
kind enum, not a general schema mechanism.

## 8. Adversarial review checklist (applied before a GO/REVISE/STOP verdict is recorded)

- Does Treatment E produce byte-identical `Resolution`s to Treatment D
  for every one of R1's 9 rows (not just pass the same aggregate gates,
  which could mask a compensating pair of divergences)?
- Is the registry actually built once (outside the timed per-query loop),
  or does a bug rebuild it per query, silently reproducing D's own cost
  under a different name?
- Does the registry correctly handle a `(ProductTypeId, attribute)` pair
  that has zero products of that type at all (must return "not
  registered," matching D's `.any()` over an empty iterator returning
  `false`)?
- Is the latency measurement methodology (batch size, trial count,
  median) identical to R1's own, so the two overhead numbers are a fair,
  like-for-like comparison?
