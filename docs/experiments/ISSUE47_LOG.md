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
