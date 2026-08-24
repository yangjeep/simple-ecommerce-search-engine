# Issue #47 Experiment Log — E2d: adaptive semantic consensus and
# proposal-model capability/cost frontier

## I47-E2d Phase A: adaptive-consensus controller

**Hypothesis** (`docs/experiments/ISSUE47_PROTOCOL.md`): a deterministic
adaptive controller wrapped around the already-frozen E2c canonicalizer
(R1-R11, unchanged) can match fixed-5-ensemble (A1) quality/safety while
requesting materially fewer independent proposals per semantic problem,
by certifying a result robust once no possible composition of the
remaining undrawn proposals (up to the pool's own size) could change the
promoted `(role, primitive, scope)` triple.

**Method**: reused `e2c_canonicalizer::canonicalize` (Treatments C and D)
completely unmodified. Implemented a pure deterministic controller
(`crates/issue42-eval/src/e2d_controller.rs`) that, at each depth `n`,
checks whether a hypothetical unanimous block of the remaining
`K_MAX - n` proposals (for any of the 7 possible alternate roles — the
maximum-leverage adversarial perturbation for any vote mix at that
budget) could ever change the current outcome; if not, it stops early
and delivers the current result, never reading any proposal's own
self-reported `confidence` (grep-verified by an automated test). A0
(single proposal), A1 (fixed-5, reusing E2c's own leave-one-out design
unchanged), A2 (adaptive/Treatment C), and A3 (conservative
adaptive/Treatment D) are all computed from the same 5-draw pool per
real key; A2/A3's own repeated-run stability is measured via 5 cyclic
rotations of that pool (`ISSUE47_PROTOCOL.md` §8), matching A1's own
`C(5,2)=10` pairwise-comparison sample size.

Reproduction:
`cargo build --release -p issue42-eval && ./target/release/e2d_adaptive_consensus_eval [calibration|heldout] [out.json]`

### Calibration-lane sanity check (before any held-out draw exists)

Recorded in full in `ISSUE47_PROTOCOL.md`'s own Addendum 1 (not
duplicated here) — run against the already-frozen, already-analyzed
`automotive` E2b artifacts (zero new live calls), confirming the
controller behaves correctly end to end: 100% match with A1 on
oracle-disagreement count (1 of 17), 100% full-descriptor/primitive
stability for both A2 and A3, zero abstention, mean depth 3.18/5
(36.47% reduction vs fixed-5). Named explicitly: the controller's own
"majority lock" mathematics make `n=3` the earliest possible certified
depth for `K_MAX=5` — no key can ever certify at n=1 or n=2 regardless
of real-vote unanimity, since a hypothetical unanimous 3- or 4-vote
remaining block always outvotes 1 or 2 real votes under plain plurality.
No controller code or threshold was changed after this run.

## Real Product/Variant/relationship dataset attempt (external validity)

Per Issue #47's "Datasets / external validity" section: attempted to
acquire at least one license-compatible real structured feed/catalog
with genuine Product/Variant identity or relationship complexity (beyond
WANDS, which R6's own audit already established has **no** real per-row
Variant grouping — every WANDS record is exactly one Product with
exactly one Variant, per `e2b_ingest::build_catalog`'s own doc comment,
carried forward unchanged from E2b/E2c).

This session's network access is restricted to a pre-configured allowlist
(`curl -sS "$HTTPS_PROXY/__agentproxy/status"` shows the policy). A
general web request to `kaggle.com` — host of several well-known real
Product/Variant catalogs (e.g. Home Depot search relevance, Olist
Brazilian e-commerce) — is denied by gateway policy (403,
`connect_rejected`). `github.com` (both HTTPS and `git` protocol) **is**
reachable, confirmed directly (`git ls-remote
https://github.com/magento/magento2-sample-data.git HEAD` succeeded).

A search identified `magento/magento2-sample-data` (official Magento 2
sample catalog, OSL-3.0/AFL-3.0 licensed, GitHub-hosted) as a reachable
candidate with genuine configurable-product (parent) / simple-product
(child SKU, e.g. size/color combinations) structure — real in the sense
of reflecting an actual commercial platform's production configurable-
product schema (used by real Magento merchants), though itself a
vendor-authored demo catalog rather than data scraped from a live store,
a materially weaker "real" claim than WANDS's own genuine Wayfair scrape.

**Decision**: full integration (clone, parse Magento's CSV/product-
attribute-set format, build a new ingestion module analogous to
`e2b_ingest`, author an independent oracle for its configurable/simple
product split, wire it through the E2d pipeline) is a materially sized
new-dataset-ingestion engineering task in its own right, not a short
extension, and Issue #47's own priority ordering places Phase A and
Phase B — not dataset acquisition — as this checkpoint's primary
deliverables. Issue #47 explicitly authorizes this exact fallback: "If
no qualifying dataset is acquired, Phase A/B may still produce useful
results but external validity remains NOT ESTABLISHED." Per that
clause: **external validity beyond WANDS/automotive is NOT ESTABLISHED**
for this checkpoint. `magento/magento2-sample-data` is recorded here as
a confirmed-reachable, license-compatible candidate for a future
checkpoint, not silently dropped.

### Confirmed defect, fixed with a regression test (found while building
### this checkpoint's own held-out safety accounting)

While wiring criterion 1 (zero unsafe accepted) into `e2d_adaptive_consensus_eval`,
found that `e2c_metrics::unsafe_accepted_count`'s implementation
discarded the promoted role entirely (`|(key, _)| ...`) and flagged
*every* promoted key whose oracle role is Identifier/Relationship as
"unsafe," regardless of whether the promoted role actually matched
oracle -- contradicting the function's own doc comment ("an accepted/
promoted descriptor whose oracle-confirmed real role is Identifier or
Relationship" being the risk R5/R7 exist to prevent, i.e. a genuine
identifier/relationship silently accepted as something else) and this
file's own pre-existing test name
(`unsafe_accepted_is_zero_when_promoted_role_matches_oracle_or_oracle_is_not_identity`,
which never actually exercised the Identifier/Relationship branch, so it
passed without verifying its own stated intent).

Reproduced independently with a new failing test
(`unsafe_accepted_does_not_flag_an_identifier_correctly_promoted_as_identifier`,
`e2c_metrics.rs`): a genuine identifier (`part_number`-shaped) correctly
promoted *as* Identifier (its oracle-matching, R5-cleared, safe outcome)
was wrongly flagged unsafe by the pre-fix code (count=1, expected 0).
Fixed by adding the missing role-mismatch comparison; a paired
regression test (`unsafe_accepted_still_flags_an_identifier_promoted_as_something_else`)
preserves the true-positive case the function exists to catch.

**Verified this does not change any of E2c's own already-published
numbers**: `e2c_canonicalization_eval.rs`'s own established usage
pre-filters promoted keys to `is_structural` (Enum/Numeric/Boolean)
*before* ever calling `unsafe_accepted_count`, so Identifier/
Relationship-oracle keys never reached this function in E2c's own
pipeline -- the defect was latent, never triggered, in every one of
E2c's own headline runs. Confirmed directly: re-ran both
`e2c_canonicalization_eval` and `e2c_serving_overhead_eval` before and
after the fix (`docs/research/artifacts/i47_e2d_e2c_regression_check_run1/`)
-- the canonicalization JSON is byte-identical; the serving-overhead
JSON differs only in measurement-inherent latency/build-time jitter
(same disclosed precedent as the R3/R9 fix in `ISSUE45_PROTOCOL.md`),
confirmed by diffing every non-latency field programmatically (identical).
The defect surfaced only in E2d's own broader safety scope, which
deliberately does not pre-filter to structural-only, so as to exercise
Identifier/Relationship promotions too (`ISSUE47_PROTOCOL.md`'s own
"Product/variant correctness is non-negotiable" mandate applies with
full force to the controller's own safety accounting, not only to the
canonicalizer rules themselves).

### Held-out measurement

10 fresh, independent `claude-sonnet-5` proposal draws (5x
`wands_baseline`, 5x `automotive`) were generated via a Workflow run
(one transient connection failure on `automotive-run2`, retried
successfully via Workflow resume -- see run IDs in this checkpoint's own
tool history) and frozen unmodified to
`dataset_cache/export/e2d_llm_proposals_{wands_baseline,automotive}_run{1..5}.json`.
Reproduction: `./target/release/e2d_adaptive_consensus_eval heldout docs/research/artifacts/i47_e2d_heldout_run1/summary.json`.

**A striking first-order finding, verified before trusting it**: raw
role-level agreement across all 5 genuinely independent draws is
**unanimous on all 53 of 53 held-out keys**, both configurations (zero
raw disagreement at all -- verified directly against the raw JSON
files, not inferred). Before treating this as a real result rather than
a caching artifact, checked: (1) all 10 raw draw files have distinct
MD5 hashes (genuinely different completions, not a Workflow cache
collision on identical (prompt, opts) tuples across "different" run
indices -- a real risk given the prompt is byte-identical across runs
within a config, and Workflow's own resume mechanism is explicitly
content-keyed); (2) this unanimity is claude-sonnet-5-specific evidence
about *this* model's raw consistency on *this* bounded-input contract,
not a property of the canonicalizer -- E2b/E2c's own original frozen
artifacts (a different, undocumented model per `ISSUE42_PROTOCOL.md`'s
own disclosed gap) showed real raw disagreement (87.60%/74.96%) on the
same underlying WANDS/automotive fields. This is recorded as a genuine,
disclosed, first-order Phase A finding, not assumed away: **a more
capable/more recent proposal model can itself substantially reduce raw
disagreement**, which is directly relevant to Phase B's own question.

**Consequence for A0-A3**: because raw role agreement is unanimous
everywhere, `promoted_role_by_key` (the full per-key promoted-role map,
included in the summary JSON for exactly this kind of verification) is
**byte-identical across A0, A1, A2, and A3** on every held-out key,
verified programmatically, not inferred from aggregate counts alone.
For A2/A3 specifically this is not a coincidence but a **provable
consequence of the worst-case-robustness certificate's own definition**
(`e2d_controller.rs`): an early-certified outcome is, by construction,
proven identical to whatever the full 5-draw pool would have produced,
so A2's primary-rotation trace equaling A1 on every key is a designed
invariant, not a surprise. A0 matching A1 on every key is the genuinely
notable part -- it means, for this specific held-out draw, even a
single proposal already carried enough signal (combined with the
stats-driven R1/R4-R7 rules) to reach the same final answer 5 draws
would have -- but this is a property of *this checkpoint's own draws*
being unusually clean, disclosed as such, not claimed to generalize.

#### Results (combined, 53 keys; safety fields use the fixed `unsafe_accepted_count`;
#### certified-robust fields use the post-adversarial-review-fixed semantics --
#### see the "Adversarial review" section below for both fixes. Numbers here
#### are the corrected, final reading, not the checkpoint's own first draft.)

| Metric | A0 (n=1) | A1 (fixed-5) | A2 (adaptive C) | A3 (conservative D) |
|---|---|---|---|---|
| Mean depth (per-key unit) | 1.0 | 5.0 | 3.98 | 3.98 |
| Median / P95 depth | 1 / 1 | 5 / 5 | 3 / 5 | 3 / 5 |
| Reduction vs fixed-5 (per-key unit) | 80.0% | 0.0% | 20.38% | 20.38% |
| **Raw batched-call count (deployment unit)** | 1 | 5 | **5** | **5** |
| **Reduction vs fixed-5 (deployment unit)** | 80.0% | 0.0% | **0.0%** | **0.0%** |
| Certified-robust rate (corrected) | n/a | n/a | 50.94% | 50.94% |
| Full-descriptor stability | n/a | 100.00% | 100.00% | 100.00% |
| Primitive stability | n/a | 100.00% | 100.00% | 100.00% |
| Unsafe accepted | 0 | 0 | 0 | 0 |
| Oracle disagreements (of 47 promoted) | 3 | 3 | 3 | 3 |
| Retrieval-significant recall | 92.11% | 92.11% | 92.11% | 92.11% |
| Abstention rate | 11.32% | 11.32% | 11.32% | 11.32% |

**The two bolded rows are the single most important, easy-to-miss
finding in this checkpoint, per the adversarial review below**:
`ISSUE47_PROTOCOL.md` §10 preregistered two distinct cost units *because*
one live call returns proposals for every key in its configuration at
once -- a straggler key still unresolved forces another full-config
draw covering every key again, "so a real deployment using this exact
batching shape" saves nothing until every key in the batch certifies.
Both configurations have at least one such straggler (P95 depth = 5 in
both), so **`raw_batched_call_count` for A2/A3 equals A1's own (5) in
every scope measured here -- zero deployment-relevant savings**, even
though the per-key unit shows a real 20.38% reduction. This is not a
new finding contradicting the per-key numbers -- both were always true
by the protocol's own design -- but the first draft of this section
reported only the flattering per-key unit and omitted this row
entirely, exactly the disclosure gap the adversarial review's
methodology reviewer found and is corrected here.

Disagreeing keys (all four treatments, identical): `productwarranty`,
`heat_range` -- both already named in `ISSUE45_DECISION.md` as genuine
reasonable-disagreement cases (raw consensus itself disagrees with the
oracle author's own single judgment call, not a canonicalizer defect) --
plus `title`, a new disagreement not seen in E2c's own precedent
(`ISSUE42_PROTOCOL.md`'s own workload notes flag `title` as a real
"does this duplicate `product_name`'s role" trap by design). No unsafe
promotions in any of the three.

**Per-configuration breakdown** (the aggregate above hides a real
split, and the corrected certified-robust numbers sharpen it
considerably): `automotive` (17 keys) reduces 37.65% per-key (mean
depth 3.12/5), with a **corrected** certified-robust rate of 94.12% (16
of 17 keys genuinely proven early, not merely exhausted) -- close to
the theoretical ceiling this checkpoint's own controller design allows
(§8's "majority lock" argument makes `n=3` of 5 the earliest possible
certified depth). `wands_baseline` (36 keys) reduces only 12.22%
per-key (mean depth 4.39/5), with a **corrected** certified-robust rate
of just **30.56%** (11 of 36 keys) -- down sharply from this
checkpoint's own first-draft figure of 83.33%, which the adversarial
review found was inflated by a vacuous-truth bug (below) counting every
key that merely exhausted the pool as "certified." The real,
messier WANDS data is not just harder to reduce cost on than the
cleaner synthetic automotive set -- for roughly seven in ten promoted
WANDS keys, the controller never actually proves the early stop would
have been safe; it just runs out of draws and reports whatever
`canonicalize()` says at that point, identical to what A1 would have
said anyway. Abstention (16.67%, all four treatments identically) is
real WANDS data-quality noise, not an early-stopping artifact, per the
identical abstention rate across every depth.

#### Phase A GO-gate evaluation (criteria per `ISSUE47_PROTOCOL.md` §11)

1. **Zero confirmed unsafe accepted** -- 0 for A2 and A3. **PASS.**
2. **>=99% compiled primitive agreement** -- 100.00% (A2/A3). **PASS.**
3. **>=98% full canonical descriptor agreement** -- 100.00% (A2/A3). **PASS.**
4. **Retrieval-significant recall within 3pp of fixed-5** -- identical
   (92.11% vs 92.11%, 0pp gap), because A2/A3's promoted sets are
   byte-identical to A1's on this held-out data. **PASS.**
5. **No material relevance regression** -- not independently
   recomputed: since A2/A3's `promoted_role_by_key` is byte-identical to
   A1's on every held-out key (verified programmatically), the compiled
   catalog and end-to-end relevance are necessarily identical to
   whatever A1/E2c's own established result already is, inheriting the
   same disclosed `check_reliable=false` near-floor caveat E2b/E2c
   already carry -- not independently confirmed here, explicitly by
   inheritance. **PASS-by-inheritance, disclosed as such.**
6. **Average per-key depth reduced by >=40% vs fixed-5** -- 20.38%
   combined under the per-key unit. **FAIL.** (automotive alone: 37.65%,
   close to this controller design's own mechanical ceiling;
   wands_baseline alone: 12.22%, far below.) Under the protocol's own
   *other* preregistered cost unit (raw batched-call count, the
   deployment-relevant one given this repo's real batching mechanism),
   the reduction is **0%** in every scope measured -- both
   configurations have at least one straggler key that never certifies
   before exhausting the pool, so a real deployment issuing whole-config
   batched draws would need the same 5 calls A1 already needs. **FAIL
   under both cost units, more starkly under the deployment-relevant
   one.**
7. **Savings not achieved primarily through excessive abstention** --
   A2/A3's own abstention rate (11.32%) is *identical* to A1's own
   (11.32%) -- the modest reduction achieved is genuine early
   certification, not increased abstention. **PASS.**
8. **Max-depth unresolved cases abstain rather than force a vote** --
   structurally guaranteed by the controller (verified by
   `max_depth_unresolved_abstains_never_forces_a_vote` and related
   `e2d_controller.rs` tests). **PASS.**

**Quality/safety (1,2,3,4,5,7,8) passes cleanly.** **Economics
(criterion 6) fails** at the combined level, under both preregistered
cost units. Per Issue #47's own text: "If quality passes but economics
do not, record REVISE." A fresh adversarial review (per Issue #47's own
governance, before freezing the Phase A controller) follows below.

### Fresh adversarial review

Three independent reviewer agents, no implementation mandate, no access
to each other's output or this session's own conclusions, each given a
different angle (controller math/algorithm; methodology and peeking
risk; safety-critical correctness). All findings below were
independently confirmed by this session before acting on them, not
applied on the reviewers' word alone.

**1. CONFIRMED, safety/interpretation-relevant: vacuous "certified"
flag at max depth (controller-math reviewer).** `e2d_controller.rs`'s
`worst_case_robust` returned `true` unconditionally whenever
`remaining_budget == 0` -- true only in the vacuous sense that no
composition of *zero* remaining draws can change anything, not because
any adversarial composition was tested and survived. Concrete failing
case: `contested_role_escalates_to_full_depth`'s own genuine 3-Enum-vs-
2-FreeText split, which only resolves at `n=5`, was reported
`certified_robust_at_stop = true` -- contradicting `ISSUE47_PROTOCOL.md`
§8's own text ("Promoted but not certified-robust... disclosed
explicitly per key... not hidden inside an aggregate pass rate").
**Verified this affects zero decisions**: `remaining_budget == 0` occurs
only at `n == k_max`, where `run_controller`'s own decision is *already*
unconditionally `Stop` regardless of this function's return value (the
`if n == k_max` branch takes priority over the `robust` check) -- so
`n_used`, `final_outcome`, and every quality/safety GO-gate number
(criteria 1-5, 7, 8) are byte-for-byte unaffected; only the
`certified_robust`/`certified_robust_at_stop` *reporting* fields, and
metrics derived from them (`certified_robust_rate_pct`), were wrong.
Fixed (`return false` instead of `return true` on that branch); two
existing tests (`contested_role_escalates_to_full_depth`,
`plurality_margin_alone_does_not_certify_when_r9_conflict_risk_remains`)
strengthened with explicit `certified_robust_at_stop` assertions that
would have caught this. **Rerun affected measurements**: corrected
`certified_robust_rate_pct` combined 88.68% -> **50.94%**; automotive
100% -> 94.12%; wands_baseline 83.33% -> **30.56%** (the results table
above is the corrected reading; the pre-fix numbers are superseded, not
silently discarded, and are preserved in this log's own git history at
commit `cf72f61`).

**2. CONFIRMED, disclosure gap (methodology reviewer).** This
checkpoint's own protocol (`ISSUE47_PROTOCOL.md` §10) commits to
reporting the per-key-depth cost unit *and* the raw-batched-call-count
unit "side by side, so neither reading is presented as the other," and
the calibration-lane addendum honestly did report both. The held-out
results section's own first draft dropped the raw-batched-call-count
row entirely, reporting only the flattering per-key numbers (20.38%/
37.65%/12.22% reduction) with no mention that the deployment-relevant
unit shows 0% reduction in every scope. Fixed: the results table above
now carries both rows, with an explicit callout paragraph. This is a
reporting/disclosure fix only -- no code or measurement changed.

**3. CONFIRMED, methodology reviewer's independent verification (not a
defect, a strengthening of an existing claim).** Re-checked the "not a
caching artifact" claim (draws genuinely independent, not duplicated)
by diffing `evidence`/`aliases`/`confidence` text across raw draws for
several keys directly, beyond the MD5-distinctness check this log
already cited -- confirmed genuinely varying phrasing and confidence
values run to run, not template reuse. Also independently re-verified,
from git history, that `e2d_controller.rs` and `ISSUE47_PROTOCOL.md`
were byte-identical before and after the held-out draws existed (the
controller's own freeze commitment held); noted as a real but narrower
gap that `e2d_adaptive_consensus_eval.rs`/`e2c_metrics.rs` (the code
that *scores* the frozen controller's output) were not under the same
freeze -- disclosed, not a violation, since the protocol's own §7 freeze
language covers only "the controller's code (§8, §12)."

**4. CONFIRMED, coverage gap (safety reviewer).** The
`unsafe_accepted_count` fix's own regression tests covered only
Identifier<->Enum mismatches, never the Identifier<->Relationship
cross-conflation the fix's own doc comment claims to catch. Added
`unsafe_accepted_flags_relationship_identifier_cross_conflation_both_directions`,
verifying both directions explicitly. Also confirmed: the real held-out
data never exercises this mismatch branch at all (both oracle-
Relationship keys, `compatiblediningchairpartnumber`/
`compatibledrainassemblypartnumber`, abstain rather than promote in
every treatment) -- so the held-out "0 unsafe" result, while true, is a
weak empirical test of the specific defect that was fixed; the new
unit test is the real coverage for that case.

**5. CONFIRMED, drift-risk cleanup (safety reviewer).**
`e2d_adaptive_consensus_eval.rs`'s own `safety_breakdown` function
independently re-implemented `unsafe_accepted_count`'s filter predicate
a second time (to build the reported `unsafe_keys` list), never
verified consistent with the count by any assertion -- exactly the
"independently-reimplemented safety logic that could silently drift"
pattern this repo's own conventions elsewhere avoid. Fixed: added
`e2c_metrics::unsafe_accepted_keys`, the single shared implementation
both `unsafe_accepted_count` (via `.len()`) and the eval binary now
call; a new test (`unsafe_accepted_keys_returns_exactly_the_keys_the_count_counts`)
asserts the two can never diverge.

**Checked and found sound** (not defects): the unanimous-single-role-
block worst-case search is mathematically complete for both R2's
plurality-flip risk and R3/R9's cross-run-conflict risk (independently
re-derived by the controller-math reviewer from the actual
`plurality()`/tie-break code, not merely the prose claim); the
controller never reads any proposal's own `confidence` field (traced
through the real code paths, not just the grep test); the fixed
`unsafe_accepted_count` is correct for the cases exercised; the "byte-
identical before/after" regression claim for E2c's own
`e2c_canonicalization_eval` is not just empirically true but *provably*
guaranteed (E2c's own `is_structural` pre-filter makes the old and new
predicates mathematically identical on that call site, since a
structural role can never equal Identifier/Relationship); no oracle
leakage into the controller, metrics module, or draw-generation prompt.

**Two plausible, unverified concerns recorded for future work, not
acted on now**: (a) `synthetic_role_vote` always sets `scope:
Scope::Product` regardless of the role under test, currently harmless
because `has_real_variant_grouping=false` for both configurations makes
R6 ignore scope votes entirely, but a live false-certification path if
a future dataset sets that flag `true`; (b) the "earliest certifiable
depth is `n=3` for `K_MAX=5`" claim implicitly assumes `K_MAX` is odd
and doesn't fully generalize to an even `K_MAX`, though this doesn't
affect any `K_MAX=5` result reported here.

Full workspace `cargo fmt`/`clippy`/`test`/`build` re-run clean after
every fix above.

### Phase A decision: REVISE

Per Issue #47's own governance ("record REVISE... rather than forcing
Phase B" when quality passes but economics does not; "if there is no
defensible adaptive controller, STOP... rather than forcing Phase B"):

**Quality/safety is defensible and cleanly established**: zero unsafe
accepted structural classifications (criterion 1, and the specific fix
that could have masked this was independently found and corrected by
adversarial review, then closed with regression tests covering the
exact case that mattered); 100% primitive/full-descriptor stability
(criteria 2-3); identical retrieval-significant recall to the fixed-5
reference (criterion 4); relevance inherited by construction from a
byte-identical promoted set (criterion 5); genuine, non-abstention-
driven savings where any exist (criterion 7); a structurally-guaranteed
never-forces-a-vote abstention discipline (criterion 8, verified by
tests). The controller mechanism itself -- the worst-case-robustness
certificate -- is mathematically sound (independently re-derived by
adversarial review) and, after the vacuous-certification fix, its own
diagnostic reporting is now honest about when a stop is genuinely proven
versus merely pool-exhausted.

**Economics is not established**: 20.38% combined reduction under the
per-key cost unit, well short of the preregistered 40% target, and
**0% reduction under the protocol's own deployment-relevant
raw-batched-call-count unit** in every scope measured, because every
configuration has at least one straggler key. The corrected
certified-robust numbers sharpen why: on real WANDS data, only 30.56%
of promoted keys are ever genuinely proven safe to stop early; the rest
reach their answer only because the pool ran out, identical to what the
fixed-5 ensemble would have produced anyway. The controller design's own
"majority lock" mathematics (§8) mean the theoretical best case for
`K_MAX=5` tops out at 40% reduction even under perfect unanimous
agreement -- so this specific `K_MAX` and stop-rule combination cannot
reach its own preregistered target even in the best case measured here
(automotive, at 37.65%/94.12% certified), and falls far short on the
harder, more realistic WANDS data.

**Decision: REVISE, not GO, not STOP.** The controller is defensible
(safe, mathematically sound, honestly instrumented after this review's
fixes) but does not deliver the preregistered adaptive-consensus
efficiency claim on this held-out data, particularly under the
deployment-relevant cost unit. Per Issue #47's own ordering ("only with
a frozen defensible Phase-A controller" may Phase B proceed), this
checkpoint treats the controller as frozen and defensible enough to
serve as Phase B's own shared adaptive-controller mechanism (its safety
and correctness are not in question), while carrying the economics
caveat forward explicitly: Phase B must not claim adaptive escalation
itself delivers the preregistered cost savings, only report what it
actually measures. Freezing the controller's own code
(`e2d_controller.rs`) as-is for Phase B, unmodified from this
checkpoint's own adversarial-review-fixed state.

## I47-E2d Phase B: proposal-model capability/cost frontier

**Method**: per the Phase B addendum in `ISSUE47_PROTOCOL.md`, 20 fresh
independent proposal draws were generated (10 `claude-opus-5`, 10
`claude-haiku-4-5-20251001`, 5 each × `wands_baseline`/`automotive`)
using the identical mechanism, prompt, bounded-input contract, and
K_MAX=5 as Phase A -- only the `model` parameter varied. All 20
succeeded on the first attempt (no retries needed), frozen unmodified to
`dataset_cache/export/e2d_llm_proposals_{opus,haiku}_{wands_baseline,automotive}_run{1..5}.json`.
B3 (mid-tier) reuses Phase A's own 10 `claude-sonnet-5` held-out draws
verbatim -- zero new calls. Reproduction:
`./target/release/e2d_phase_b_eval docs/research/artifacts/i47_e2d_phase_b_run1/summary.json`.

**Self-caught cost-accounting bug, fixed before reporting any Phase B
number**: the cascade's own depth metric initially counted only the
*final* tier's own consumed draws for an escalated key (e.g. opus's own
`n_used`), silently dropping the haiku tier's own already-spent draws
that triggered the escalation decision in the first place -- exactly
the "fake cascade win" Issue #47 warns against, caught by this
checkpoint's own review of its first output before it was ever
committed or logged. Fixed: an escalated key's true cost is haiku's own
`n_used` **plus** opus's own `n_used`, both genuinely spent in any real
deployment of this cascade design.

### Results

| Metric | B1 (opus fixed-5) | B2 (opus adaptive) | B3 (sonnet adaptive, = Phase A's A2) | B4 (haiku adaptive) | B5 (cascade) |
|---|---|---|---|---|---|
| n promoted (of 53) | 47 | 47 | 47 | 47 | 47 |
| Mean depth (true total draws/key) | 5.0 | 3.98 | 3.98 | 4.04 | **6.49** |
| Unsafe accepted | 0 | 0 | 0 | 0 | 0 |
| Full/primitive/role stability (own repeated draws) | 100% | 100% | 100% | 100% | 98.87% |
| Retrieval-significant recall | 92.11% | 92.11% | 92.11% | 92.11% | 92.11% |
| Abstention rate | 11.32% | 11.32% | 11.32% | 11.32% | 11.32% |
| **Cross-tier agreement vs. B2 (opus reference)** | -- | -- | **100.00%** | **98.11%** | **100.00%** |
| Total measured tokens (10 batched calls) | 164,528 | (same pool) | (Phase A's own, partly uncaptured -- see Phase A log) | 97,803 | **262,331** (both tiers' full cost) |

**Zero unsafe promotions anywhere, in every treatment.** The only
promoted-role disagreement anywhere in the entire Phase B run is a
single key: `color` (opus/sonnet/cascade: `enum`; haiku alone: `free_text`)
-- a field ISSUE45's own disagreement taxonomy already named as
genuinely messy (real values include a literal placeholder artifact,
`"[ tied to : color ]"`), not a new failure mode. B3 (sonnet) and B5
(the cascade, by construction) both reach 100% agreement with B2 (opus);
B4 (haiku alone, no escalation) is the only treatment that ever differs
from the strong reference, on this one field, out of 53.

**The headline finding: the cascade is not cheap.** B5's own true
per-key cost (6.49 draws, counting both tiers whenever escalation
fires) is **higher than B1's own fixed-5 opus ensemble (5.0)** --
confirmed in real measured tokens too: the cascade's total cost
(262,331 tokens, both tiers' full 10-call cost) is **159% of B1's own
cost (164,528 tokens)**, not a saving. The mechanism is simple and, in
retrospect, unsurprising given Phase A's own findings: the escalation
trigger (`certified_robust_at_stop`) is the same flag Phase A's
adversarial review found only genuinely fires on a modest majority of
keys even for a single tier alone -- so a cheap tier's own controller
frequently does not certify, escalation fires often, and every escalated
key pays for *both* tiers' draws, not a discount on either.

**Escalation rate is the direct explanation, and it is not low**: the
cascade escalates to opus on **49.81%** of all rotation-level decisions
(132 of 265) and, critically, on **51.58%** of retrieval-significant
ones (98 of 190) -- essentially a coin flip, and if anything *slightly
higher* for the load-bearing fields than for the catalog as a whole.
"Cheap-first" is not filtering out the easy cases and reserving the
strong model for the hard ones in any meaningfully lopsided way; it is
escalating roughly half of everything, retrieval-significant or not.

**A genuinely more promising, distinct finding, not to be confused with
the cascade's own failure**: committing to a *single* cheaper tier
(never cascading) looks considerably better than cascading between
tiers. B3 (sonnet alone) matches B2 (opus alone) **exactly** -- same
promoted role for all 53 keys, same stability, same recall, same
abstention -- at meaningfully lower per-call token cost (a same-family,
smaller/cheaper model than opus). B4 (haiku alone) reaches 98.11%
agreement with the opus reference (52 of 53 keys identical) at only
59.5% of B1's own total token cost (97,803 vs. 164,528) -- cheaper, and
only one field's worth of disagreement out of 53. **That one field is
not a random unimportant miss, though**: it is `color`, the oracle's
own highest-confidence retrieval-significant structural/attribute field
(§ below has the full evidence) -- so B4's own result reads as a
real-but-imperfect direction worth the caveat below, not a clean
Pareto win on its own. Neither B3 nor B4 ever pays for a second tier;
the cascade's own failure is specific to *cascading*, not to using a
cheaper tier at all.

#### Phase B GO-gate evaluation (criteria per `ISSUE47_PROTOCOL.md` §B6)

1. **Zero confirmed unsafe accepted** -- 0 in every treatment (B1-B5).
   **PASS.**
2. **Compiled primitive agreement within 1pp of B2** -- by cross-tier
   reading (does tier X's own compiled primitive match B2's, per key):
   B3 100.00% (0pp gap, **PASS**); B5 100.00% (0pp gap, **PASS** by
   escalation construction on this half); **B4 98.11% (1.89pp gap,
   FAIL** -- just outside the 1pp bar, driven entirely by the single
   `color` mismatch). Criterion 2's own text is conjunctive ("within 1pp
   of B2 **and above the absolute stability floor**" -- Phase A's own
   99% primitive-agreement bar). Checked separately (own within-tier
   5-rotation stability, the same reading Phase A used): B3 100.00%,
   B4 100.00% -- both clear the floor even where they fail the cross-tier
   half -- but **B5 (the cascade) is 98.87%, 0.13pp under the 99% floor**
   -- a second, independent FAIL for the cascade this checkpoint's own
   first draft never checked (found by this section's own adversarial
   review, below).
3. **Full canonical descriptor agreement within 1pp of B2** -- same
   single-key mismatch, same two readings: B3/B5 **PASS** cross-tier
   (0pp gap), **B4 FAIL** (1.89pp gap); own-stability floor (98%): B3/B4
   clear it (100.00%), **B5 fails it too** (98.87% < 99%, using the same
   primitive-agreement number since the one B5 instability is a
   primitive-level flip -- see the adversarial review below for the
   exact mechanism).
4. **Retrieval-significant recall within 3pp of B2** -- identical
   92.11% in every treatment (0pp gap in every case, since the one
   `color` disagreement is a role/primitive mismatch, not an
   abstention -- `color` is still promoted, still counted, in all four
   treatments). **PASS** for all of B3/B4/B5 -- but see the adversarial
   review below on why this presence-only metric cannot see the actual
   capability loss the `color` mismatch causes.
5. **No material relevance regression** -- B3 inherits B2's own result
   by construction (byte-identical promoted set): **PASS-by-inheritance**.
   B4/B5 differ from B2 on exactly one field, `color` -- **not a random
   unimportant field**: independently checked against every confidence
   value in `e2b_oracle.rs` (not just the reviewer's own characterization) --
   it is the oracle's own highest-confidence (`0.9`) `RetrievalSignificant`
   **structural/attribute** entry in the 53-key sample, second overall
   only to `part_number`'s `1.0` (a categorically different kind of
   field -- an exact-lookup identifier, not a descriptive attribute).
   `e2b_oracle.rs`'s own annotation calls `color` "the single most
   obvious real shopper query term in this sample." The reviewer's own
   draft
   cited "43 of 480 (~9%)" real queries containing "an explicit color
   term"; independently reproduced directly against
   `dataset_cache/wands/query.csv` before trusting it (per this repo's
   own "do not trust the experiment author" discipline, applied to a
   reviewer's own claim as much as to this session's own) -- the literal
   substring `"color"`/`"colour"` appears in only 2 of 480 queries, not
   43; matching real color **names** as whole words instead (`black`,
   `navy`, `turquoise`, `rose gold`, etc. -- a 23-word list) gives **42
   of 480 (8.75%)**, close to but not exactly the reviewer's own figure,
   confirmed here as the number this log actually relies on: 42 real
   shopper queries, not 43, and via color-name matching, not the literal
   word "color." `retrieval_significant_recall` only checks whether a
   key is
   *promoted*, not whether its role/primitive is correct, so it cannot
   see this: haiku's `free_text`/`LexicalPostings` compiles to
   substring/contains matching, not the oracle's `enum`/`BitmapEnum`
   exact-match faceting -- a real, likely material capability loss on a
   field roughly one in eleven real queries touches, not a negligible
   footnote. **FAIL-with-evidence for B4/B5**, corrected from this
   checkpoint's own first-draft "PASS-with-a-named-caveat," which
   assumed negligibility instead of bounding it.
6. **Total LLM cost falls by >=50% vs. B1, or a clear Pareto advantage**
   -- B4 alone: 40.5% reduction (97,803 vs. 164,528 tokens), short of
   the 50% bar; combined with criterion 5's corrected FAIL above, this is
   **not** the clean Pareto candidate this checkpoint's own first draft
   described (cheaper, yes, but the one quality cost is on the sample's
   single most retrieval-important field). **B5 (the cascade): FAILS
   outright** under this repo's own established whole-batch cost
   convention (`ISSUE47_PROTOCOL.md` section 10's own "raw batched-call
   count" reading, unchanged since Phase A) -- 262,331 vs. 164,528
   tokens, 159% of B1's own cost, because both tiers' full 10-call pools
   are drawn regardless of which specific keys escalate. Disclosed
   nuance (found by this section's own adversarial review): if a
   hypothetical deployment could draw the strong tier *only* for the
   49.81% of keys that actually escalate rather than the whole
   configuration batch, a rough proportional estimate gives ~109% of
   B1's cost instead of 159% -- the *direction* (cascade costs more, not
   less) is unchanged either way, but 159% is the number this repo's own
   batching reality actually supports, not a pessimistic upper bound.
7. **Strong-model escalation explicit and low enough that cheap-first is
   not cosmetic** -- 49.81% overall, 51.58% among retrieval-significant
   problems. **FAIL** -- this is not low; cheap-first resolves barely
   more than half of anything and is not measurably better at protecting
   retrieval-significant fields specifically.
8. **Overall GO requires qualifying real Product/Variant/relationship
   evidence** -- **NOT ESTABLISHED** (same disclosed limitation as Phase
   A; no qualifying dataset was acquired this checkpoint). This alone
   already precludes an overall GO regardless of B1-B7.

### Fresh adversarial review of Phase B

Two independent reviewer agents, no implementation mandate, no access to
each other's output or this session's own conclusions, one focused on
the cascade's own implementation correctness, one on the statistics and
GO-gate interpretation. All findings below independently confirmed by
this session before acting on them.

**1. CONFIRMED, corrects an omitted GO-gate check.** Criterion 2's own
text is conjunctive (cross-tier agreement *and* an absolute stability
floor); this checkpoint's own first draft evaluated only the cross-tier
half. Checked: B5's own within-tier (5-rotation) stability is 98.87%,
under the 99% floor Phase A's own criteria used -- a second, independent
FAIL for the cascade beyond the already-clear economic ones. The
mechanism: B5's escalation decision is itself computed per-rotation (a
key can certify from the cheap tier on one draw ordering and escalate on
another), so the cascade's *own* final answer is very slightly less
stable across draw orderings than any single tier's own answer -- a
real, small, previously unmeasured cost of cascading specifically, not
a defect in any tier's own controller.

**2. CONFIRMED, corrects an unsupported "not material" inference.** The
first draft's characterization of the sole B4/B5 disagreement (`color`)
as merely "the single messiest field" understated a more important
fact: it is also the oracle's own highest-confidence retrieval-significant
*structural/attribute* field (second overall only to an identifier
field, a categorically different kind of entry -- verified directly
against every confidence value in `e2b_oracle.rs`, not merely the
reviewer's own claim), and the recall metric used for criterion 4
structurally cannot
detect the capability loss a role/primitive misclassification causes
(it only checks presence in the promoted set). A cheap, concrete bound
was available and not computed in the first draft; independently
reproduced (not merely copied from the reviewer's own draft figure) at
42 of 480 real WANDS queries (8.75%) matching a real color name as a
whole word. Criteria 5 and 6 are corrected above to reflect this -- B4
is a cheaper-but-real-quality-cost result, not a clean near-Pareto
point, on the specific evidence
available.

**3. CONFIRMED, no implementation defect, one disclosed cost-accounting
nuance.** Independently traced every rotation-index use in the cascade
path (`b5_rotations`, `b5_depths`) by hand -- `b4_rotations[i]` (cheap)
and `b2_rotations[i]` (escalation target) are paired at the same index
throughout, no copy-paste or off-by-index mixing found. Independently
re-verified the escalation-rate arithmetic (132/265 = 49.8113%, 98/190 =
51.5789%, matching the reported figures exactly) and the token-manifest
sums (164,528 / 97,803 / 262,331, all matching
`e2d_phase_b_draw_cost_manifest.json` digit-for-digit). B3's own numbers
were confirmed to come from `run_controller` called fresh on the raw
sonnet draw files with the current, Phase-A-fixed controller code, not
a stale cached Phase A result. The one disclosed nuance: the 159%-of-B1
cost figure assumes this repo's own established whole-batch draw
mechanism (every key in a configuration is drawn together, so the whole
tier's own cost is paid once any key in it escalates); a hypothetical
per-key-selective draw mechanism (which this repo's tooling does not
actually support) would give a smaller, still-negative ~109% figure --
folded into criterion 6 above.

**4. CONFIRMED, a fair characterization check.** The alternative
"within-tier stability" reading of criteria 2/3 would have made B4 look
*better* (100%/100%, a clean pass) than the cross-tier reading this
checkpoint chose (98.11%, a fail) -- direct evidence the chosen reading
was not selected to flatter the treatment this checkpoint's own prose
went on to call "promising." The cross-tier reading is also the
textually correct one (criteria 2/3 explicitly name B2 as the reference
point). This checkpoint's own earlier "an interpretive choice, not
certainty" hedge overstated the real ambiguity, which is confined to
the dropped absolute-floor half of criterion 2 (now added above), not
to whether cross-tier comparison is the right frame at all.

**5. Noted, addressed by reordering below.** The first draft's own
decision section led with the cascade's failure but closed by
emphasizing B3/B4 as "the more promising direction" -- a fair
description of the *relative* comparison, but one that could read as
softening what is, against Phase B's own preregistered criteria, an
unambiguous, multi-criterion FAIL (6, 7, and now 2/3's floor clause) for
the treatment that was actually the centerpiece Issue #47 asked to test
("the cheap→strong cascade... report exactly how often the strong model
is needed"). The decision below leads with that finding first.

### Phase B decision: REVISE

**The centerpiece finding, stated first and plainly**: the cascade (B5)
-- cheap-tier-first, escalate-to-strong-only-when-needed -- does not
work. It fails criteria 2 (own-stability floor), 5 (corrected), 6, and 7
against this checkpoint's own preregistered bars, and on real measured
tokens costs 159% of simply always using the strong model, because the
escalation trigger fires on very close to half of all semantic problems
(49.81% overall, 51.58% of retrieval-significant ones specifically) --
not a rare exception cheap-first was designed to catch, but close to a
coin flip. This is exactly what Issue #47's own governance warned a
cascade might do ("do not claim small models are enough if the cascade
secretly sends the hard cases to the strong model") -- measured
directly here, not assumed: **the strong model is needed for roughly
half of this held-out catalog, slightly more of its retrieval-significant
half, and cascading does not recover cheap-tier-level cost once that is
true.**

**A secondary, more promising but still not clean, finding**: committing
to a *single* cheaper tier (never cascading between tiers within one
catalog) fares better than cascading. B3 (sonnet-5 alone) matches B2
(opus-5 alone) exactly -- every one of 53 keys, identical role/primitive/
scope/stability/recall -- at materially lower cost; on this held-out
set, the strong tier added nothing measurable over the mid tier. B4
(haiku-4.5 alone) is close but, on the corrected reading above, not a
clean Pareto point: 98.11% agreement at 59.5% of B1's cost sounds
favorable until the one disagreement is examined -- it is the sample's
highest-confidence retrieval-significant structural/attribute field,
with a real,
plausible capability loss (losing exact-match color faceting) the
recall metric cannot see. This is a REVISE-not-GO result for B4
specifically, with a real, quantified reason for caution, not merely a
missed threshold.

**Decision: REVISE.** Overall GO is precluded by criterion 8 (external
validity NOT ESTABLISHED) regardless of B1-B7. Among B1-B7: quality/
safety is clean on the criteria that measure it most directly (1, and
4's own narrow presence-only reading); the cascade (B5) fails on
economics, escalation rate, and now its own stability floor -- three
independent, preregistered criteria, not merely a single missed bar;
single-tier substitution (B3 cleanly, B4 with a real, evidenced quality
caveat) is the more informative direction, but B4 does not itself clear
this checkpoint's own bars either. Recommended next step if this thread
continues: do not re-attempt the cascade with a loosened escalation
trigger merely to manufacture a lower escalation rate (forbidden by
Issue #47's own governance); if single-tier substitution is pursued
further, measure it against a materially larger or more diverse held-out
set before trusting a 52/53-key sample where the entire quality
question turns on a single field, and prioritize measuring exactly
which semantic-problem classes (not just which fields happen to appear
in one 53-key sample) systematically need the strong tier.
