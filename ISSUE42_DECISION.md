# Issue #42 Decision — R1/R2/R3/E2b

**Decision: REVISE overall, with two evidence-supported serving-contract
changes already merged.** Issue #42's own immediate execution order (its
"Immediate execution order" section, steps 1-9) is complete through step
9: baseline frozen, protocol preregistered, R1/R2/R3 each run
independently with their own fresh adversarial review, the three
serving-contract decisions reviewed together and merged where
evidence-supported, E2b implemented and run, and a fresh adversarial
review + correction cycle performed on E2b. Per step 9's own instruction
("stop and request direction with GO/REVISE/STOP findings"), this
document is that stop point — E4/E5/E6 remain explicitly unauthorized
(step 10), and no further implementation proceeds without direction.

**Addendum (E2b serving-contract closure pass)**: a follow-up pass
audited this document's own E2b GO-gate accounting against Issue #42's
preregistered protocol, found and corrected a gate-counting error (5 of
6 criteria were enumerated, not 6), measured the one criterion
(serving overhead) that error had left unmeasured (now PASS), re-tested
repeated-run agreement on a materially larger sample (now confirmed FAIL
at 87.60%, up from 85.60%, not a small-N artifact), and audited whether
WANDS satisfies the real-structured-feed dataset requirement as actually
used (now NOT ESTABLISHED, downgraded from an unaudited PASS). E2b's
overall verdict remains REVISE. Full detail throughout the E2b sections
below, each updated in place with the correction and the original text
preserved, quoted and labeled superseded, per Issue #42's own rule 9.

## What this covers

- **Phase 0**: PR #39 (Issue #38's E1-E3) merged cleanly into
  `claude/issue-34-phase9-defect-fixes-wands` as merge commit
  **`fe2e52e0fe872a0f4ab86c63ccc839e61de8f3e6`**, recorded as the
  immutable baseline every R1/R2/R3/E2b manifest cites. Full detail:
  `ISSUE38_DECISION.md`.
- **R1 (typed ambiguity and corroborated resolution)**: four treatments
  (A: current hard-coded resolution; B: unconditional union; C: demote
  to preference; D: entity-corroborated selection) measured against a
  hand-built fixture spanning Enum/Numeric/Identifier/lexical
  interpretations of the same token. No treatment clears every
  preregistered gate — D is closest (passes every correctness/
  wrong-family/fallback criterion, corroborated NDCG 1.0) but fails the
  <=5% latency-overhead bar (11.6%-21.1% across 5 runs), attributable to
  an implementation-specific O(catalog-size) per-query scan in this
  experimental path, not a defect in the corroboration concept itself. A
  fresh adversarial review (its own "second correction round") found
  five real issues — a latency-number transcription error, two doc/test
  miscounts, and, most substantively, that Treatment C's own measured
  NDCG gap vs. D was materially confounded by an unrelated residual-veto
  interaction (R2's own subject) rather than purely reflecting
  corroboration-awareness — all fixed, none reversing the REVISE
  verdict. **No production change was made for R1**; current behavior
  (Treatment A) is retained. Full detail: `docs/experiments/ISSUE42_LOG.md`'s
  "I42-R1" section.
- **R2 (residual lexical semantics): GO for Treatment D.** A compiled
  `Required`/`Preferred` residual-token policy passes every preregistered
  gate (>=90% benign-case recovery, <=1% false recovery on adversarial
  cases, no query-time LLM inference) against a purpose-built 56-product
  fixture with paired benign/adversarial residual-word cases. A fresh
  adversarial review found one real defect — `ResidualPolicy::classify`
  didn't originally account for a compound structural constraint
  narrowing the candidate set — reproduced RED-before-fix, fixed,
  re-verified GO still holds. **Merged into production**: new
  `commerce_core::plan::residual` module; `execute_planned` gained an
  additive `residual_policy: Option<&ResidualPolicy>` parameter, `None`
  (every pre-existing call site) byte-identical to prior behavior. ADR
  0012. Full detail: `docs/experiments/ISSUE42_LOG.md`'s "I42-R2"
  section.
- **R3 (identifier serving primitive): GO for Treatment C.** A
  calibrated uniqueness-ratio-and-variant-scope classifier gating a
  dedicated identifier dictionary clears every preregistered gate: 100%
  Recall@1 / 0% false-match on the genuinely-unique held-out sample,
  correct rejection of adversarial near-miss/prefix queries a
  variant-level text-index alternative (Treatment B) incorrectly
  matches, correct abstention on every non-identifier field (including a
  deliberately ambiguous 2-occurrence "legitimate cross-reference"
  case), and substantially lower build/incremental-update cost than B.
  A fresh adversarial review's own "second correction round" found four
  substantive issues plus one minor one — including a real, disclosed
  negative result (a candidate second classifier signal,
  format-consistency, was tested with real numbers and found to
  empirically *fail*, recorded rather than erased) — all fixed, none
  reversing the GO verdict. **Merged into production**: new
  `commerce_core::index::identifier` module (`IdentifierClassifier`,
  `IdentifierDictionary`), plus a new `MIN_IDENTIFIER_SAMPLE_SIZE=100`
  safeguard disclosed as an addition beyond R3's own experimental scope.
  `plan::LexicalHit` gained an additive `variant: Option<VariantId>`
  field. ADR 0013. Full detail: `docs/experiments/ISSUE42_LOG.md`'s
  "I42-R3" section.
- **Production merge review**: R1/R2/R3's serving-contract decisions
  were reviewed together (per step 6) before any production change —
  R1's REVISE meant no change; R2/R3's GO verdicts were merged. A fresh
  adversarial review of the merge itself found two confirmed defects
  (an ADR call-site-count error; a production test fixture that
  couldn't reproduce R2's own compound-constraint scenario) —
  independently reproduced and fixed, with a new regression test. Full
  detail: `docs/experiments/ISSUE42_LOG.md`'s "I42-Merge" section.
- **E2b (offline LLM-assisted feature discovery and physical
  selection): REVISE.** Statistics-only macro F1=0.5366 (the honest "no
  semantic understanding" floor), LLM proposal (no validator)
  F1=0.7985, LLM + deterministic validator F1=0.7697 (recall 0.8889 at
  precision 0.9756).

  **Correction (E2b serving-contract closure pass)**: the paragraph
  immediately below, as originally written, is preserved per rule 9 but
  is **superseded** — it enumerated only 5 of Issue #42's own 6
  preregistered E2b GO-gate criteria (silently omitting criterion 5,
  serving overhead, which this document's own "What this does NOT
  establish" section, unchanged below, already and correctly disclosed
  as never measured) and, on that incomplete basis, incorrectly called
  repeated-run agreement "the sole remaining gap." A fresh audit found
  and fixed this; `docs/experiments/ISSUE42_LOG.md`'s "I42-E2b
  serving-contract closure" section has the full corrected six-criterion
  table, the newly-implemented serving-overhead measurement (criterion
  5, genuinely **PASS**), a materially larger repeated-run stability
  re-test (criterion 4, **FAIL** at 87.60% on a 10x larger sample, up
  from 85.60%, not close enough to attribute to noise), and a new audit
  of criterion 6 (real structured unseen feed) finding WANDS, as
  actually used in this pipeline, has no real Variant concept and never
  exercises its two oracle-labeled Relationship fields anywhere in the
  pipeline — **downgraded from PASS to NOT ESTABLISHED**. **Corrected
  overall verdict: still REVISE**, now for two established gaps
  (criteria 4 and 6) rather than one, alongside a newly-measured genuine
  PASS on criterion 5. See "What this does NOT establish" and the
  E2b-specific verdict paragraph below, both updated to match.

  > Superseded text (as originally written): "GO gate, final: zero
  > confirmed unsafe accepted classifications (PASS), 86.84% recall on
  > retrieval-significant reference features (PASS), 0.00% end-to-end
  > relevance gap vs oracle (PASS, though the check itself is flagged
  > unreliable at this near-floor magnitude), real structured unseen
  > feed evidence via WANDS (PASS), 85.60% repeated-run agreement on
  > accepted physical primitive vs the 90% bar (**FAIL** — the sole
  > remaining gap, never part of any defect, not a rounding artifact)."

  Two rounds of adversarial review (a self-review before any number was
  trusted, then a fresh, independent subagent review per step 8) found
  and fixed five real implementation defects, all confined to the
  evaluation harness (never `commerce_core`), each with a new regression
  test — the most consequential silently substituted the hardest
  name-perturbation configuration for the intended "real key names
  visible" baseline, understating the LLM's real capability by roughly
  32 relative percentage points of macro F1 until fixed. **No production
  change was made or is warranted for E2b** — REVISE, not GO. Full
  detail: `docs/experiments/ISSUE42_LOG.md`'s "I42-E2b" section
  (original two correction rounds) and "I42-E2b serving-contract
  closure" section (the gate-accounting fix, serving-overhead
  measurement, stability re-run, and WANDS audit), plus four manifests
  (`benchmarks/manifests/i42_e2b_feature_discovery_eval.yaml`,
  `artifacts/manifests/i42_e2b_feature_discovery_eval.json`,
  `benchmarks/manifests/i42_e2b_serving_overhead_eval.yaml`,
  `benchmarks/manifests/i42_e2b_stability_rerun.yaml`).

## The R1/R2/R3/E2b verdicts, stated precisely

**R1**: REVISE. The corroboration concept (Treatment D) is directionally
correct — it wins every correctness/wrong-family/fallback criterion —
but this implementation's latency cost is real and unexplained away;
current production behavior is retained until a cheaper implementation
of the same idea is built and re-measured.

**R2**: GO, merged. A compiled residual-token policy measurably
recovers legitimate zero-result cases without introducing false
recovery on adversarial ones, with no query-time LLM inference — the
central R2 claim holds and is now in production, additive and
opt-in (`None` by default).

**R3**: GO, merged. A statistics-only (never name-based) identifier
classifier gating a dedicated dictionary primitive is strictly better
than expanding the existing lexical delegate to variant scope, on every
measured axis (correctness, cost, adversarial robustness) — now in
production, additive and opt-in.

**E2b**: REVISE. The underlying idea — LLM-proposed feature descriptors,
gated by a deterministic validator that never sees the oracle, can
substantially outperform a "no semantic understanding" statistics-only
floor — is well-supported by the final, twice-corrected numbers
(F1 0.7697 vs 0.5366). Of the six preregistered gates (not five — see
the correction above), four now clear cleanly: zero unsafe accepted,
>=80% recall on retrieval-significant features, end-to-end relevance
within 5% (with a standing reliability caveat, unchanged), and — newly
measured this pass — <=5% serving overhead vs the hand-authored oracle
(P95/P99 both above this measurement's own timer floor, -1.95%/-4.11%).
Two do not: repeated-run agreement, re-measured on a 10x larger sample
at 87.60% (1095/1250), still short of 90% and, with per-configuration
consistency across all three real-WANDS-derived configurations (86-88%
each), not attributable to the original small sample being unlucky; and
real structured unseen feed evidence, previously asserted as PASS
without ever being audited against its own "Product/Variant or
relationship complexity" wording — WANDS, as actually used in this
pipeline, has no real Variant concept and never exercises either of its
two oracle-labeled Relationship fields anywhere in the pipeline, so this
criterion is corrected to NOT ESTABLISHED, not PASS. This document does
not manufacture a GO by treating either shortfall as close enough, and
does not manufacture a STOP by treating the newly-strengthened evidence
on the other four criteria as outweighing them. The evidence is
genuinely encouraging on four of six axes and genuinely short on the
other two, not sufficient as preregistered.

## What this does NOT establish

- That R1's corroboration idea is unworkable — only that this specific
  experimental implementation's per-query scan is too slow; a
  precomputed ingestion-time index (the same pattern R2/R3 both already
  use) was never built or measured for R1's own corroboration lookup.
- That E2b's LLM-assisted mechanism would perform this well on a real
  feed with more than 36 preregistered sample keys, or on a category
  structurally unlike furniture/home-goods (WANDS) or automotive parts —
  both are the only real/synthetic sources tested.
- ~~That E2b's 85.60% repeated-run agreement is a stable population
  parameter — only 2 runs per configuration were measured; a materially
  larger N was out of this pass's own scope.~~ **Resolved (E2b
  serving-contract closure pass)**: re-measured on a materially larger,
  predetermined sample (5 runs/configuration, 1250 pairwise comparisons
  up from 125) at 87.60% (1095/1250), consistently below 90% across all
  three real-WANDS-derived configurations — genuinely established as a
  real shortfall, not a small-N artifact.
- That the statistics-only baseline's F1=0.5366 floor and the
  LLM+validator's F1=0.7697 ceiling generalize to feeds where field
  names are *never* shown to the classifying pass at all (this
  experiment's canonical-config restriction, fixed in the second
  correction round, specifically measures the "real names visible"
  case; `wands_anonymized`'s own 0.4900 macro F1 is the better estimate
  of a name-blind ceiling).
- ~~End-to-end serving overhead vs. the hand-authored oracle (the E2b
  GO gate's own fifth, `<=5%` criterion) — not measured in this
  pass, a real gap in the evidence, not silently glossed over.~~
  **Resolved (E2b serving-contract closure pass)**: measured directly —
  see the E2b correction above. P50 of both measured operations sits
  below this measurement's own pre-declared timer floor (correctly
  reported INCONCLUSIVE, not rounded to PASS); P95/P99 of the heavier,
  more realistic `execute_ranked` operation are both above the floor and
  clear the <=5% bar.
- That WANDS, as actually used in E2b, satisfies Issue #42's own
  "Product/Variant or relationship complexity" dataset requirement —
  audited and found NOT ESTABLISHED (see the E2b correction above): no
  real Variant concept, and the two oracle-labeled Relationship fields
  are never materialized or exercised anywhere in this pipeline.
- That the reconstructed prompt used for the 12 new repeated-run-
  stability passes is identical in wording to the original 8 passes'
  own prompt — it is not: the original prompt's literal text was never
  committed to this repository (only its frozen JSON output was), so the
  new passes' prompt is a faithful reconstruction from the preregistered
  descriptor schema and instructions text, not a byte-identical replay.
  The bounded INPUT DATA itself (statistics, sample values, shown key
  names) is, by contrast, fully deterministic and was reproduced
  exactly.
- Anything about E4 (compiled deterministic reranking), E5 (bitmap
  reuse for faceting), or E6 (integrated system economics) — explicitly
  protocol/design-only under this epic (step 10), not started.
- Whether R2's `ResidualPolicy`/R3's `IdentifierClassifier`, now merged,
  generalize beyond the fixtures/catalogs each was measured against —
  both are additive and opt-in in production (`None` by default), which
  bounds the blast radius of that open question but does not close it.

## What would be built next if continuing this thread

1. **A precomputed, ingestion-time version of R1's corroboration
   lookup** (mirroring R3's own dictionary-primitive pattern), re-measured
   against the same <=5% latency bar — if it clears, R1 becomes a third
   evidence-supported production change; if not, the REVISE verdict is
   confirmed on a implementation-independent basis.
2. ~~A materially larger E2b repeated-run sample~~ — **done** (E2b
   serving-contract closure pass): 87.60% (1095/1250) on a 10x larger
   pairwise sample, still below 90%, consistently across all three
   real-WANDS-derived configurations. Confirmed as a real shortfall, not
   small-N noise.
3. ~~E2b's own missing serving-overhead measurement~~ — **done** (E2b
   serving-contract closure pass): PASS, with a disclosed P50 timer-floor
   caveat (see the E2b correction above).
4. **A second, structurally distinct real feed** for E2b (beyond WANDS
   furniture/home-goods), to test whether the LLM+validator mechanism's
   performance is feed-specific or genuinely general — now also the
   route to resolving the newly-corrected criterion 6 (real structured
   unseen feed with genuine Product/Variant or relationship complexity),
   which WANDS as actually used here does not establish.
5. **A cheap, deterministic real-Variant/relationship check for the E2b
   dataset gate itself** — even a real feed with a formal Variant
   concept could still fail this gate if the pipeline never exercises
   it (WANDS's own failure mode); any next real-feed choice should
   verify the pipeline actually ingests and queries Variant/relationship
   structure, not merely that the feed's schema nominally has one.
6. **E4/E5/E6**, per Issue #42's own explicit sequencing — only after
   R1's own re-test (item 1) and E2b's own remaining gap (item 4/5) are
   addressed, and only with explicit follow-up authorization, never
   silently started under this epic.

## What should explicitly not be built yet

- Any production change adopting R1's corroboration concept before a
  cheaper implementation clears the latency bar on its own merits —
  the concept being directionally correct is not sufficient
  justification per Issue #42's own rule 11 ("benchmark success does
  not authorize rewriting the architecture claim more broadly than the
  evidence").
- Any production change compiling E2b's LLM-proposed descriptors
  directly into the serving path — E2b's own REVISE verdict, and this
  document's own "GO gate — final" table, mean it stays exactly what
  Issue #42's own protocol always called it: an evaluation baseline,
  never a "ship this" pipeline (verbatim from the preregistered
  protocol's own baseline-2 description).
- E4/E5/E6 implementation of any kind — named explicitly as
  protocol/design deliverables under this epic, not implementation
  authorization, per Issue #42's own step 10.
- Any generic query DSL or document-schema abstraction — CLAUDE.md's
  standing prohibition applies with full force to every finding above,
  R2/R3's own new typed modules included.
