# Issue #47 Decision — E2d: adaptive semantic consensus and
# proposal-model capability/cost frontier

**Overall decision: REVISE.** Not GO, not STOP. Phase A's adaptive
consensus controller is safe, mathematically sound, and adversarially
reviewed, but does not clear its own preregistered efficiency bar.
Phase B's centerpiece mechanism — the cheap→strong cascade — is a clear,
well-evidenced negative result (it costs *more* than always using the
strong model). Phase B's secondary finding — single-tier substitution,
never cascading — shows real promise (the mid tier matched the strong
tier exactly) but was measured on only 53 keys and one held-out draw,
and external validity against a genuine Product/Variant/relationship
dataset remains NOT ESTABLISHED throughout, which alone precludes an
overall GO regardless of every other result.

## What this covers

Executed under Issue #47's own governance: Phase A preregistered before
any held-out draw existed (`docs/experiments/ISSUE47_PROTOCOL.md`),
implemented behind an experimental boundary
(`crates/issue42-eval/src/e2d_controller.rs`, `e2d_metrics.rs`), measured
on held-out data, adversarially reviewed by three independent reviewer
agents with a confirmed defect found and fixed, then frozen and decided
before Phase B was preregistered as an addendum to the same protocol
document, implemented (`bin/e2d_phase_b_eval.rs`), measured, and
adversarially reviewed by two further independent reviewers with two
more confirmed corrections. Full detail in `docs/experiments/ISSUE47_PROTOCOL.md`
(preregistration, including the Phase B addendum) and
`docs/experiments/ISSUE47_LOG.md` (the experiment log, including both
adversarial reviews). Baseline: clean `main` head
`20db66a0016176b3c16c1566c4e0796584f5e243`, exactly as Issue #47
requires; branch `claude/issue-47-e2d-adaptive-consensus`, fresh off
that commit, not stacked on any historical research branch.

- **Reused, not reinvented**: the Phase A/B controller is a pure
  algorithmic wrapper around the already-frozen, unmodified E2c
  canonicalizer (R1-R11, `e2c_canonicalizer.rs`) — no canonicalizer or
  validator rule was changed for either phase, per Issue #47's own
  instruction, except one independently confirmed metrics-layer defect
  (below) that E2c's own established usage was never exposed to.
- **Real, independent proposal-model calls**, not fixture replay: every
  held-out draw in both phases is a genuine, fresh completion from the
  named model (`claude-sonnet-5` for Phase A and Phase B's mid tier,
  `claude-opus-5` for Phase B's strong tier, `claude-haiku-4-5-20251001`
  for Phase B's small tier), generated via the same fresh-subagent,
  no-session-context mechanism `ISSUE42_PROTOCOL.md`'s own E2b Amendment
  1 established as this repository's precedent, frozen unmodified to
  `dataset_cache/export/e2d_llm_proposals_*.json` before any measurement
  touched them.
- **Two confirmed defects, found by fresh adversarial review, fixed with
  regression tests**: (1) `e2d_controller.rs`'s own worst-case-robustness
  flag was vacuously `true` whenever a draw pool was merely exhausted,
  not only when genuinely proven safe early — verified to affect zero
  controller decisions or quality/safety numbers (the flag was purely a
  reporting field at that code path), but it materially inflated the
  `certified_robust_rate_pct` diagnostic (corrected: combined
  88.68%→50.94%, WANDS alone 83.33%→30.56%). (2)
  `e2c_metrics::unsafe_accepted_count` discarded the promoted role
  entirely, flagging every promoted oracle-Identifier/Relationship key
  as "unsafe" regardless of whether it was actually mismatched —
  verified this never affected any of E2c's own already-published
  numbers (its own established usage pre-filters to structural roles
  only, a call site where the bug was provably inert), fixed, and
  covered by new regression tests including the specific
  Identifier↔Relationship cross-conflation direction a second review
  found untested even after the first fix.
- **Two confirmed reporting corrections, found by fresh adversarial
  review of Phase B**: a dropped cost-accounting disclosure (the
  protocol's own preregistered "raw batched-call count" unit, which
  shows 0% deployment-relevant savings for Phase A's adaptive controller
  even where the per-key unit shows 20.38%) was restored; a
  "color is just the messiest field" characterization of Phase B's one
  cross-tier disagreement was corrected after independently verifying it
  is actually the oracle's own highest-confidence retrieval-significant
  structural/attribute field, with a real, quantifiable capability-loss
  risk (losing exact-match faceting on a field ~8.75% of real WANDS
  queries reference) the recall metric used for the GO gate cannot
  detect on its own.
- **A self-caught cost-accounting bug**, found and fixed by this
  checkpoint's own review of its own first output before ever reporting
  a number: the cascade's per-key cost initially counted only the final
  (escalated-to) tier's own consumed draws, silently dropping the cheap
  tier's own already-spent draws that triggered escalation in the first
  place — exactly the "fake cascade win" Issue #47's own text warns
  against.

## Phase A verdict, stated precisely

**Quality/safety (criteria 1, 2, 3, 4, 5, 7, 8) passes cleanly**: zero
confirmed unsafe accepted structural classifications across every
treatment (A0-A3) on 53 held-out semantic problems; 100% compiled-
primitive and full-descriptor stability for the adaptive treatments;
retrieval-significant recall identical to the fixed-5 reference (0pp
gap); relevance inherited by construction from a byte-identical
promoted set; genuine (non-abstention-driven) savings where any exist;
a structurally-guaranteed, test-verified "never forces a vote at max
depth" discipline. The controller mechanism itself — a complete,
provably sufficient worst-case search over unanimous single-role
synthetic vote blocks — was independently re-derived and confirmed
mathematically sound by adversarial review, not merely asserted.

**Economics (criterion 6) fails**, under both preregistered cost units:
20.38% average per-key depth reduction combined, well short of the 40%
target (automotive alone: 37.65%, near this controller design's own
mechanical ceiling of 40% for `K_MAX=5`; WANDS alone: 12.22%, far
below); **0% reduction under the deployment-relevant raw-batched-call
unit** in every scope measured, since every configuration has at least
one straggler key that never certifies before exhausting the pool. The
corrected `certified_robust_rate_pct` sharpens why: on real WANDS data,
only 30.56% of promoted keys are ever genuinely proven safe to stop
early — the rest reach their answer only because the draw pool ran out,
identical to what the fixed-5 ensemble would have produced anyway.

**Decision: REVISE.** The controller is defensible (safe, mathematically
sound, honestly instrumented after adversarial review) but does not
deliver the preregistered adaptive-consensus efficiency claim on this
held-out data. Frozen as-is, unmodified, to serve as Phase B's own
shared adaptive-controller mechanism, carrying the economics caveat
forward explicitly rather than re-tuning the trigger to manufacture a
better number.

## Phase B verdict, stated precisely

**The cascade (B5) is the headline negative result.** Its true per-key
cost (6.49 draws, both tiers whenever escalation fires) exceeds simply
always using the strong model (5.0 draws, B1's own fixed-5 ensemble),
confirmed in real measured tokens (262,331 vs. 164,528 — 159% of B1's
own cost under this repository's own established whole-batch draw
convention; ~109% even under a hypothetical, currently-unsupported
per-key-selective draw mechanism — the direction is unchanged either
way). The mechanism: the escalation trigger fires on 49.81% of all
semantic problems and **51.58% of retrieval-significant ones
specifically** — not a rare exception cheap-first was designed to
catch, essentially a coin flip — and every escalated key pays for both
tiers' draws, never a discount on either. The cascade fails three
independent, preregistered criteria: economics (6), escalation-rate
(7), and — found only by adversarial review, not the first draft — its
own within-tier stability floor (2/3's dropped conjunctive clause,
98.87% vs. a 99% bar).

**Single-tier substitution (never cascading) is a distinct, more
promising finding, but not a clean pass.** B3 (`claude-sonnet-5` alone)
matches B2 (`claude-opus-5` alone) exactly — identical promoted role for
all 53 keys, identical stability/recall/abstention — at materially
lower cost: on this held-out set, the strong tier was not needed at
all. B4 (`claude-haiku-4-5-20251001` alone) reaches 98.11% agreement
with the strong reference (52 of 53 keys identical) at 59.5% of B1's own
cost — but the one disagreement is not a random unimportant miss: it is
`color`, the oracle's own highest-confidence retrieval-significant
structural/attribute field, with a real, plausible loss of exact-match
faceting a presence-only recall metric cannot detect. B4 is a
REVISE-not-GO result on real, quantified evidence, not merely a missed
threshold.

**Decision: REVISE.** Overall Phase B GO is precluded by criterion 8
(external validity NOT ESTABLISHED) regardless of B1-B7. Among B1-B7:
quality/safety is clean on the criteria that measure it directly; the
cascade fails on three independent grounds and should not be
re-attempted with a loosened escalation trigger merely to manufacture a
lower rate (forbidden by Issue #47's own governance); single-tier
substitution is the direction with real promise but needs a materially
larger and more diverse held-out sample before the quality question
(currently turning on one field out of 53) can be trusted.

## Answering Issue #47's own central question

> How much model do we actually need once semantic compilation is
> deterministic, and exactly which semantic problems still force us to
> pay for a stronger model?

On this held-out data: the strong tier (`claude-opus-5`) was not needed
at all when the mid tier (`claude-sonnet-5`) was used directly — it
matched the strong tier's compiled output exactly on every one of 53
keys. The small tier (`claude-haiku-4-5-20251001`) came close but not
close enough on the sample's single most retrieval-important field, a
genuine and quantified (not assumed) gap. **Exactly which problems force
a stronger model, measured rather than guessed**: roughly half of all
semantic problems in this sample, and a very slightly higher share of
the retrieval-significant ones — the escalation rate a strict,
adversarially-hardened safety trigger actually produces when applied to
the small tier specifically. The *cascade* strategy for capturing "cheap
most of the time, strong only when needed" does not deliver that
combination here: because escalation is common, not rare, cascading
costs more than committing to the strong tier outright. **Model
capability was not shown to be unimportant** — the opposite: half of
this sample's own semantic problems, including a disproportionate share
of the load-bearing ones, still required strong-tier-level certainty to
resolve safely under the frozen controller's own deliberately
conservative certification rule.

## What this does NOT establish

- That the mid-tier-matches-strong-tier finding (B3 = B2 exactly)
  generalizes beyond this 53-key sample — WANDS/automotive, the same two
  sources this entire E2* thread has used throughout.
- That the small tier (haiku) is an acceptable substitute for the strong
  tier in production — its one measured disagreement is on the sample's
  single most retrieval-important field, a real, disclosed, unresolved
  risk, not a cleared bar.
- That the cascade strategy is unsalvageable in every possible design —
  only that this specific, frozen, safety-preserving certification
  trigger produces an escalation rate too high for cheap-first to pay
  off; a materially different trigger design was explicitly not
  attempted here, per Issue #47's own prohibition on re-tuning
  thresholds after seeing an unfavorable result.
- Anything about real Product/Variant/relationship generalization — no
  qualifying dataset was acquired this checkpoint (external validity NOT
  ESTABLISHED, disclosed explicitly in both phases, with a confirmed-
  reachable, license-compatible candidate for future work named in
  `docs/experiments/ISSUE47_LOG.md`).
- Anything about E4/E5/E6, R1b/#51, or production compilation of E2d's
  outcomes into `commerce_core`'s real serving path — none authorized or
  attempted here; the controller and every compiled bundle in this
  checkpoint remain experimental/evaluation-only, matching E2b/E2c's own
  precedent exactly.
- That per-key-scoped independent model calls (as opposed to this
  repository's own established whole-configuration batching) would
  change the cascade's own qualitative conclusion — the direction (costs
  more, not less) held under both cost-accounting readings measured, but
  only the batched reading was actually exercised end to end.

## What would be built next if continuing this thread

1. **A materially larger and more diverse held-out sample for the
   single-tier-substitution question specifically** — B3's exact match
   and B4's one-field miss are both drawn from the same 53 keys Phase A
   already used; a genuinely fresh, larger sample would tell us whether
   `color`-style messy-but-important fields are common enough to worry
   about, or a one-off.
2. **A dataset or fixture that stresses proposal-model capability
   directly** — this checkpoint's own 53-key sample turned out to have
   near-unanimous raw agreement even from a single mid-tier draw,
   leaving little room to observe capability differences; a workload
   specifically constructed with harder, more genuinely ambiguous
   semantic problems would give the capability/cost frontier question a
   fairer test.
3. **A genuine test of whether a less conservative, still-safe
   escalation trigger exists** for cascading specifically — this
   checkpoint used the same worst-case-robustness certificate Phase A's
   own adversarial review hardened, deliberately not loosened to chase a
   better cascade number; whether a different, still-provably-safe
   trigger could lower the escalation rate without reintroducing a false-
   certification risk is a real, unanswered, and non-trivial design
   question.
4. **The real Product/Variant/relationship dataset** — `magento/magento2-sample-data`
   (confirmed reachable, OSL-licensed) remains the best-identified
   candidate; full ingestion (ingestion pipeline, independent oracle,
   wiring through the E2d pipeline) is a materially sized task of its
   own, not attempted here.

## What should explicitly not be built yet

- Any production compilation of E2d's controller or its cascade design
  into `commerce_core`'s actual serving path.
- Any query-time LLM call of any kind — the controller runs entirely
  offline over already-frozen artifacts throughout both phases,
  unchanged from E2b/E2c's own precedent.
- Any claim that the cascade strategy "works" based on B3/B4's own more
  favorable single-tier numbers — the cascade's own negative result is
  real and specific to cascading, not resolved by a different treatment
  succeeding.
- A re-attempt of the cascade with a loosened escalation trigger tuned
  to produce a lower escalation rate — Issue #47's own governance
  forbids exactly this ("do not weaken thresholds... to manufacture a
  GO"), and this checkpoint's own adversarial review confirmed the
  trigger's strictness is load-bearing for safety, not merely
  conservative by accident.
- E4/E5/E6, R1b/#51, any generic query DSL or document-schema
  abstraction — CLAUDE.md's standing prohibition applies with full
  force; zero new enum variants were added by this checkpoint.
