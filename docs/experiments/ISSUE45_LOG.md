# Issue #45 Experiment Log — E2c: deterministic semantic canonicalization

## I45-E2c: deterministic canonicalization of stochastic LLM proposals

**Hypothesis** (`docs/experiments/ISSUE45_PROTOCOL.md`): even when raw
LLM proposals disagree (E2b's own 87.60% raw repeated-run agreement,
below the 90% bar), a deterministic canonicalization and validation
layer can map them to stable, safe, compiled descriptors — the
architecture succeeds only if instability decreases substantially as
information moves toward the serving plane.

**Method**: reused E2b's 20 already-frozen `dataset_cache/export/e2b_llm_proposals_*.json`
raw LLM proposal artifacts unmodified (4 configurations × 5 runs, no new
LLM calls of any kind). `CandidateDescriptor` is `e2b_schema::Descriptor`
reused verbatim. Implemented 11 deterministic canonicalization rules
(R1-R11, `crates/issue42-eval/src/e2c_canonicalizer.rs`) and three
treatments: B (naive plurality vote, `e2c_majority_vote.rs`, kept in its
own module so it structurally cannot share the canonicalizer's
evidence-aware logic), C (the canonicalizer), D (C plus a stricter
majority-not-plurality admission bar for structural roles). Reproduction:
`cargo build --release -p issue42-eval && ./target/release/e2c_canonicalization_eval
[out.json]` and `./target/release/e2c_serving_overhead_eval [out.json]`.

### Explaining the raw 87.60% result (§6 of the protocol; summarized here)

Independently reproduced E2b's own 1095/1250 (87.60%) byte-for-byte
(`scripts/e2c_disagreement_taxonomy.py`). Of the 155 disagreeing pairs:
primitive-selection ambiguity 65.2% (`candidate_physical_primitive`
flip-flops at identical stats — resolved by R1), value-type ambiguity
18.1% (Enum-vs-Numeric, resolved by R3; Enum-vs-FreeText, resolved by
plain R2 plurality — see the Addendum 1 correction below), model
hallucination/error 14.2% (`color`'s spurious `Relationship` reading of
a junk placeholder value — blocked by R7), insufficient evidence 2.6%
(`compatibledrainassemblypartnumber`'s sample-vs-aggregate-statistics
contradiction — resolved by R8/abstention). A second, 158-pair
scope-disagreement pool is invisible to E2b's own role+primitive-only
metric entirely, root-caused to WANDS's real lack of per-row Variant
identity — resolved by R6's dataset-structural default. Combining all
four descriptor fields, true raw agreement is 74.96%, not 87.60%
(independently reproduced twice: once by the Python taxonomy script,
once by `e2c_canonicalization_eval`'s own Rust "raw (Treatment A,
extended)" computation — 937/1250 both times).

### Results, round 1

| Metric | Raw (A) | B (majority vote) | C (canonicalizer) | D (conservative) |
|---|---|---|---|---|
| Leave-one-out full-descriptor stability | 74.96% | 81.68% | 100.00% | 100.00% |
| Leave-one-out primitive stability | 89.12% | 92.80% | 100.00% | 100.00% |
| Unsafe accepted | n/a | 0 | 0 | 0 |
| Retrieval-significant recall | n/a | 97.37% | 89.47% | 89.47% |
| Abstention rate | n/a | 13.21% | 20.75% | 20.75% |

A self-administered stricter check (not required by the leave-one-out
design in §9, added because 100% looked too clean to trust without a
harder test): canonicalizing each of the 5 raw runs **individually**
(N=1, so R2's cross-run plurality and R3's cross-run evidence
arbitration can never engage — only R1/R4/R5/R6/R7/R8's single-proposal
rules can act) and comparing those 5 single-run canonicalizations
pairwise: **full-descriptor stability 95.20%**, primitive stability
95.20% — both below the preregistered 98%/99% bars, unlike the
leave-one-out reading. Worst-case unsafe-accepted across every
individual single-run canonicalization: still 0.

Two keys are canonically stable (100% leave-one-out agreement) but
disagree with the oracle's own hand-authored role: `productwarranty`
(canonicalizer: Enum, matching a real 4/5 raw plurality; oracle:
FreeText) and `heat_range` (canonicalizer: Numeric, matching a real 5/5
raw unanimous vote; oracle: Enum). Both are genuine reasonable-disagreement
cases — the *raw proposal consensus itself* disagreed with the oracle
author's own single judgment call — not canonicalizer defects.

Serving overhead (`e2c_serving_overhead_eval`, comparing oracle-compiled
vs Treatment-C-compiled `commerce_core::index::CatalogIndex` bundles over
the real 42,994-product WANDS catalog): `indexed_candidates` P50
INCONCLUSIVE (below the pre-declared timer floor); `execute_ranked`
P50 INCONCLUSIVE, P95 -0.21%, P99 -0.97% — both clear the ≤5% bar.
Combined verdict: PASS.

### Fresh adversarial review

Three independent reviewer agents, no implementation mandate, no access
to each other's output or this session's own conclusions (raw reports:
`docs/research/artifacts/i45_e2c_adversarial_review_run1/reviewer_{A,B,C}.md`).
Full findings and the fixes they required are documented in
`docs/experiments/ISSUE45_PROTOCOL.md`'s own "Addendum 1" section, not
duplicated here; summarized:

1. **Confirmed defect, fixed with a regression test**: R3/R9's original
   condition engaged on any pairwise Enum/Boolean-vs-Numeric conflict
   anywhere among the raw proposals, not only when the real plurality
   winner was itself contested — capable of silently overwriting a real
   majority. All three reviewers independently verified this did not
   corrupt any of the 20 real frozen artifacts' own measured numbers;
   confirmed a fourth time by re-running both eval binaries after the
   fix (`docs/research/artifacts/i45_e2c_canonicalization_run2/`,
   `i45_e2c_serving_overhead_run2/`) — every number byte-identical to
   the pre-fix run.
2. **Confirmed documentation defect, corrected**: this protocol's own
   §6 table claimed R3 resolves Enum-vs-FreeText disagreement
   (`productwarranty`/`warrantylength`); false — R3's own gating
   condition structurally cannot fire there. Corrected in place, rule 9.
3. **A genuinely humbling, measured finding, not a defect**: R3 fires on
   exactly 1 of 125 (config, real-key) groups in the whole dataset and
   never once changes the outcome from what plain R2 plurality alone
   already gives (`bin/e2c_r1_r6_attribution_diagnostic.rs`, output
   preserved at `docs/research/artifacts/i45_e2c_r1_r6_attribution_run1/`).
   Treatment C's real differentiation from naive majority voting (B)
   comes from R1 (primitive as a deterministic function of role — a
   genuine, non-vote-derived mechanism), R6 (a disclosed, tautological
   dataset-structural scope default, not evidence integration), and
   R5/R7 (real safety mechanisms that each blocked one actual
   unsafe-shaped promotion Treatment B's own naive vote would have
   made) — not from R2/R3 being meaningfully smarter arbitration of the
   same votes B already counts.
4. Oracle leakage: ruled out by all three, independently (grep +
   inspection of every `e2c_*.rs` decision function).
5. Abstain-Abstain-counts-as-agreement: a real, gameable metric
   convention, but measured directly to affect only 2 of 27
   raw-unstable (config, key) groups in this run — not the mechanism
   behind the headline numbers.
6. GO-gate criterion 5's boolean does not itself consult
   `e2e_check_reliable` — inherited unchanged from E2b's own identical,
   already-reviewed limitation (`ISSUE42_LOG.md`'s own "Fresh
   adversarial review" section); this decision record treats criterion
   5 as PASS-with-caveat, never unqualified, matching that precedent.
7. Treatment D never diverges from Treatment C on any of the real 20
   artifacts (every real unstable key has a landslide, not a
   contested, role split) — D's own distinguishing mechanism is
   validated only by a synthetic unit test in this checkpoint, not by
   real data.
8. Arithmetic: independently recomputed by all three reviewers from
   the raw JSON; no discrepancies found.

### Results, final (post R3 fix; identical to round 1's own numbers — the
defect did not affect any real measured value, only the rule's
soundness on hypothetical future data)

Unchanged from the table above. See
`docs/research/artifacts/i45_e2c_canonicalization_run2/summary.json`
and `i45_e2c_serving_overhead_run2/summary.json` for the byte-identical
confirmatory re-run.

### GO gate, final

See `SCALE_UP_DECISION.md`-style treatment in `ISSUE45_DECISION.md`
(repo root) for the full seven-criterion table under both the
leave-one-out and single-run readings, and the final verdict.

Full workspace `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `cargo test --workspace
--all-features`, `cargo build --workspace --release` all green after
every fix in this section.
