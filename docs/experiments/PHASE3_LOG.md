# Phase 3 Experiment Log — Issue #14, safe fast-path offload frontier over Solr

Follows the same discipline as `docs/experiments/PHASE2_LOG.md`: each entry
states a falsifiable hypothesis before implementation, records raw real-data
results (not just a summary verdict), and ends in KEEP/REJECT (this phase's
per-change decision) feeding toward Issue #14's own final STRONG SUPPORT /
NARROW SUPPORT / NEGATIVE verdict.

## Recap: why Phase 3 exists

Phase 2 (`PHASE2_DECISION.md`, `docs/experiments/PHASE2_LOG.md` P2-E17)
rejected the whole-engine 5-10x QPS/$ replacement thesis: traffic-weighted,
commerce-native was ~2.3-3.0x *slower* than a mature Solr baseline, because
its `Hybrid`/`Punt` paths (99%+ of real traffic) always paid for both
structural planning *and* an embedded Tantivy delegate call, on top of a
single mature Solr call's cost, never instead of it. The one clean physical
win -- `structural_exact_entity`/`variant_scoped_structural`, `FastPath`,
87-105x faster than Solr -- covered under 1% of real traffic on the real
Amazon ESCI catalog/query corpus, and even that win carried a real,
uncorrected relevance cost (`execute_ranked` has no ranking signal when
`query.preferences` is empty, which it always is for this benchmark's
baseline lexicon).

Issue #14 reframes the thesis around that one clean win rather than trying
to salvage `Hybrid`/`Punt`: treat Solr as the permanent, mature safety-net
fallback, and ask how much real traffic a *cheap* admission check can
safely intercept for native `FastPath` execution -- never both commerce-
native *and* a delegate call on the same query. This removes the exact
mechanism P2-E17 found responsible for most of the "slower" result.

## Architecture

```text
query
  -> compile() (cheap, in-process, already excluded from every prior
     phase's own timing convention)
  -> admission::admit() (cheap: ambiguity/residual/no-constraint checks,
     one indexed_candidates bitmap build against a tunable selectivity cap
     -- no delegate call, no Tantivy, no Hybrid narrow-then-rank)
      -> Admit: execute natively (CatalogIndex::execute_ranked, unchanged
         from Phase 2 -- Issue #14's own rule: do not rebuild ranking)
      -> Reject: forward the ORIGINAL, unmodified Solr request (the same
         solr_query_for()-shaped query Phase 2's P1-D already validated as
         a fair baseline construction) -- exactly as if commerce-native did
         not exist for this query
```

`commerce_core::admission` (new this phase) owns the mechanism:
`AdmissionPolicy { max_candidates }`, `AdmissionDecision::{Admit,Reject}`,
`RejectReason` (Ambiguous / UnresolvedResidual / NoStructuralConstraint /
NotSelectiveEnough). `max_candidates` is the one tunable knob Issue #14's
RQ2 coverage-frontier sweep varies; additional knobs are added only when a
specific rejected-query-class experiment (P3-E03+) finds real evidence one
is needed, not speculatively ahead of that evidence.

`crates/phase3-eval` (new this phase) owns measurement, mirroring
`phase2-eval`'s structure but with no Tantivy dependency at all -- Phase 3's
target architecture never calls an embedded delegate.

## P3-E00 — scaffold: `commerce_core::admission` + `phase3-eval`

**Evidence class**: unit-level (hand-built fixtures), no real data yet --
this entry documents the mechanism build itself, before any real-corpus
measurement.

7 tests in `crates/commerce-core/tests/admission.rs`, one rejection reason
isolated per test: candidate count within/at/over the policy cap
(admit/admit/reject, pinning the inclusive boundary explicitly rather than
leaving it to whichever comparison operator got typed); ambiguity rejects
regardless of an otherwise-generous cap; unresolved residual rejects; no
structural constraint at all rejects; a `Preference`-only compiled query
(no hard constraints, per `ir::query::compile`'s own contract that a
`Preference`-resolved phrase always stays in `residual_lexical` --
ADR 0010) rejects even though `preferences` is non-empty, guarding against
a future change mistaking a soft ranking signal for admission-worthy
structure.

Quality gate green: fmt, clippy `-D warnings`, full workspace test suite
(122 tests total across the workspace, up from 115), release build.

**Decision**: KEEP (mechanism only, no real-data verdict yet). Next:
P3-E01, the transparent fallback baseline -- measure the admission check's
own overhead on the reject path before trusting any coverage/relevance
number that depends on it staying cheap.

## P3-E01 — transparent fallback baseline: fallback tax

**Evidence class**: real (full 1,215,854-product Amazon ESCI catalog,
full 22,458-query judged corpus, fresh local Apache Solr 9.10.1 re-indexed
with the identical catalog -- same evidence base Phase 2 used).

**Hypothesis**: `admission::admit()`'s own overhead on the reject path
(the one cost every rejected query pays) is close to zero relative to a
single Solr round trip -- Issue #14's invariant 1, initial target <=5%
fallback tax.

**Method**: `crates/phase3-eval/src/bin/p3e01_fallback_baseline.rs`. Whole
corpus admission pass (cheap, no Solr calls) with `AdmissionPolicy {
max_candidates: 50 }` as an initial conservative starting point (not yet
calibrated -- that is P3-E02's job), reporting coverage and a reject-
reason breakdown. Then a seeded-random (`ChaCha8Rng`, not "smallest-N-by-
id" -- addressing the P2-E17 adversarial review's own finding about Phase
2's sampling) sample of 20 rejected queries, 30 reps/arm, interleaved via
`bench_harness::round_robin_schedule`, comparing `solr_baseline` (a direct
`solr_search` call, using the exact `round1_eval::solr::solr_query_for`
construction Phase 2's P1-D already validated as fair) against
`admission_then_solr` (`admit()` -- which returns `Reject` for every
query in this sample by construction -- immediately followed by the
identical `solr_search` call). `compile()` itself is excluded from both
arms' timed block, symmetric with every prior phase's own convention.

**Result** (raw: `docs/research/artifacts/p3e01_run1/`):

Whole-corpus admission pass (`max_candidates=50`): **admitted 82/22,458
(0.37%)**; rejected ambiguous 5,005 (22.29%); rejected unresolved-residual
17,268 (76.89%); rejected not-selective-enough 103 (0.46%); rejected
no-structural-constraint 0 (0.00%, absent from the corpus entirely at
this lexicon).

**A strong, unplanned cross-validation against Phase 2's own established
9-class counts**, all exact: rejected-ambiguous (5,005) equals Phase 2's
`ambiguous_punt` class size exactly; rejected-unresolved-residual (17,268)
equals the sum of Phase 2's four residual-bearing classes exactly
(`structural_plus_lexical_residual` 3,253 + `structural_plus_semantic_residual`
57 + `lexical_first` 8,274 + `long_tail_noisy` 5,684 = 17,268); admitted +
not-selective-enough (82 + 103 = 185) equals Phase 2's total FastPath-
eligible population exactly (`structural_exact_entity` 153 +
`selective_multi_attribute_structural` 0 + `variant_scoped_structural` 30
+ `range_plus_structural` 2 = 185). Same lexicon, same `compile()`, same
underlying query population -- `admission::admit`'s ambiguity/residual
checks land on precisely the same population boundaries `classify9`
already established, which is exactly what should happen since both read
the same compiled-query shape, and is a real (if incidental) regression
check that the new module isn't silently drawing its lines somewhere
`classify9` doesn't.

A genuinely new number this run reveals: **of the 185 queries that would
have been FastPath-eligible under Phase 2's plain "no residual, no
ambiguity" test, more than half (103/185, 55.7%) resolve to a candidate
set larger than 50** -- i.e. a strict, safety-first selectivity cap
rejects a majority of Phase 2's own FastPath population outright. This is
concrete evidence for why P3-E02's coverage-frontier sweep matters: the
raw "FastPath-eligible" population Phase 2 measured is not automatically
"safe" by this phase's own selectivity-conscious admission bar.

Fallback-tax latency (interleaved, 30 reps/arm): `solr_baseline`
mean=2.5603ms (sd=0.4102ms); `admission_then_solr` mean=2.4983ms
(sd=0.2702ms). Bootstrap CI (diff = `admission_then_solr` - `solr_baseline`):
**diff=-0.0620ms, CI=[-0.2377, 0.1050], excludes_zero=false** -- the
admission check's own cost is not statistically distinguishable from
zero at this sample size, and the point estimate is negative (i.e.
`admission_then_solr` measured *faster*, which is measurement noise, not
a real negative cost -- `admit()` cannot make a Solr call faster). As a
percentage of the `solr_baseline` mean: **-2.42%**, comfortably within
Issue #14's <=5% target with headroom to spare.

**Threats to validity, stated rather than assumed away**: n=20 unique
queries (not a random draw from the full 17,376-query rejected
population at this policy) -- adequate for this exploratory measurement
per Issue #14's own distinction between exploratory and paper-grade final
replication, but the *exact* tax percentage should not be over-read from
one run at this sample size; the CI already including zero is the
important, robust part of this result, not the -2.42% point estimate.
The `not_selective_enough` rejection path (which does build a real
`indexed_candidates` bitmap, unlike the other three cheap short-circuit
rejections) was not isolated from the cheaper rejection paths in this
run's 20-query sample by construction -- a targeted follow-up sampling
only `NotSelectiveEnough`-rejected queries would more precisely bound the
selectivity-check's own marginal cost specifically, if a future
experiment needs that finer breakdown.

**Decision**: KEEP. The admission check's overhead is close to zero and
statistically indistinguishable from a pure Solr call, meeting Issue
#14's invariant 1 with margin. Proceed to P3-E02: sweep `max_candidates`
on the full real corpus to trace the safe-coverage/relevance frontier.

## P3-E02 — the safe coverage/relevance frontier: coverage is capped by semantic resolution, not selectivity

**Evidence class**: real (full 1,215,854-product catalog, full
22,458-query judged corpus, live local Solr, single deterministic
whole-corpus pass -- relevance/coverage/candidate-count numbers require
no repeated measurement per `bench_harness`'s own documented methodology,
since `commerce_core`'s compile/plan/execute path is deterministic).

**Hypothesis**: sweeping `AdmissionPolicy::max_candidates` traces a real
coverage-vs-relevance frontier -- higher caps admit more real traffic at
some relevance cost, per Issue #14 RQ2's four target budgets (~0%, 0.5%,
1%, 2% degradation).

**Method**: `crates/phase3-eval/src/bin/p3e02_coverage_frontier.rs`. One
pass: compile every real query; query Solr for every real query (148.8s
for 22,458 queries -- the whole-corpus pure-Solr baseline, cap-
independent); find every "structurally eligible" query (ambiguous empty,
residual empty, >=1 constraint, at an effectively unlimited cap) and rank
it natively; sweep `max_candidates` in
`{1,2,3,5,10,20,30,50,75,100,150,250,500,1000,2500,5000,10000,50000,200000}`,
filtering the eligible set by candidate count at each point. Native
execution latency measured once (30 reps, individual per-query timing)
over a small-candidate-set sample, since a query's own native cost never
depends on which cap is being evaluated.

### Result 1 -- only 185/22,458 (0.82%) real queries are ever structurally eligible, at *any* cap

The full sweep, from `max_candidates=1` to `200,000` (nearly the entire
1.2M-product catalog), tops out at **183/22,458 admitted (0.81%)** --
essentially the entire eligible population, reached already by
`max_candidates=2,500`. Two of the 185 eligible queries are *never*
admitted even at `max_candidates=200,000` (their candidate sets exceed
even that). **Selectivity is not the binding constraint on coverage --
semantic resolution is.** The dominant rejection reason (P3-E01's own
whole-corpus admission pass, `max_candidates=50`): unresolved residual
lexical text, 17,268/22,458 queries (76.89%) -- more than three times the
ambiguous-rejection rate (5,005, 22.29%) and roughly 170x the
not-selective-enough rate (103, 0.46%). Tightening or loosening the
selectivity cap cannot move coverage past this ceiling; only reducing the
*residual* rejection rate can.

### Result 2 -- whole-workload relevance degradation is tiny across the entire sweep

| cap | admitted | coverage | native NDCG (admitted) | Solr NDCG (same admitted) | whole-workload NDCG | degradation vs. Solr-only |
|---|---|---|---|---|---|---|
| 1 | 20 | 0.09% | 0.0000 | 0.0271 | 0.2335 | +0.0000 |
| 50 | 82 | 0.37% | 0.2428 | 0.3034 | 0.2333 | +0.0002 |
| 500 | 174 | 0.77% | 0.1438 | 0.2105 | 0.2330 | +0.0005 |
| 200,000 | 183 | 0.81% | 0.1367 | 0.2019 | 0.2330 | +0.0005 |

Whole-workload pure-Solr-only baseline: NDCG@10 = 0.2335. Worst observed
whole-workload degradation across the *entire* sweep: 0.0005 absolute
(~0.21% relative) -- comfortably inside every one of Issue #14's four
target budgets (0%, 0.5%, 1%, 2%) in absolute terms, though see the
calibration note below for a real, minor subtlety in the 0% case.
Native-vs-Solr NDCG *on the admitted subset itself* is consistently
negative (native worse, by 0.06-0.09 NDCG at most cap values) -- this is
the same missing-ranking-signal cost P2-E17 already found and flagged as
unresolved (`execute_ranked` has no ranking signal when
`query.preferences` is empty), inherited unchanged into Phase 3. It does
not threaten the *whole-workload* budget at this coverage level only
because the admitted subset is such a small share of total traffic
(<=0.81%) that its own relevance cost barely moves the aggregate.

**A real, minor methodological note, not smoothed over**: the RQ2
calibration loop reports "no swept cap value stays within 0.0% budget,"
even though the sweep table's 4-decimal-rounded degradation column shows
`+0.0000` for `max_candidates` 1 through 20. The exact (unrounded)
degradation at those cap values is a small positive number that rounds
to `0.0000` for display but is not exactly zero, so it fails a strict
`<= 0.0` comparison. A true, non-rounded exact-zero budget is not
achievable by construction (any admitted-and-imperfect query moves the
degradation off exactly zero), so this is not a bug, but worth stating
precisely: the reported table already shows the *actual* achievable
floor, and only the "0.0%" *label* is unreachable in the strict sense.

### Result 3 -- false-positive admissions exist even at the strictest cap, and are the *same* real-catalog defect P2-E15 already found

At `max_candidates=1` (the most conservative point swept), 2 of the 20
admitted queries are false-positive admissions (native NDCG=0 while Solr
found at least one relevant result): qid 51954 ("huggies size 1") and
qid 64314 ("luvs size 3"). Both are diaper-brand-plus-numeric-size
queries. Checked directly against `docs/experiments/PHASE2_LOG.md`
P2-E15's own diagnostic output from this session: **qid 64314 is the
exact same query P2-E15 already root-caused** -- every real judged-
relevant product for this query carries *no* `size` attribute in
`effective_attributes()` at all, so any numeric size constraint is
unsatisfiable regardless of value, a genuine real-catalog data-quality
gap (this ingestion's diaper products carry no structured size field),
not a Phase 3-specific defect. "huggies size 1" is the identical shape
(same brand category, same missing attribute). This is expected, not
alarming: Phase 3's "structurally eligible" population *is* Phase 2's
`FastPath`-eligible population (P3-E01 already found this an exact-count
match), so the same already-diagnosed data-quality cases necessarily
recur here. **Important, and genuinely new**: a small, near-unique
candidate set (`max_candidates=1`, as conservative as an admission policy
can be) does *not* protect against this failure mode -- a highly
selective but semantically-*absent*-data structural match is just as
capable of a false-positive admission as a broad one. Selectivity and
correctness are different axes; no selectivity cap alone closes this gap.
False-positive admissions grow from 2 (cap=1) to 54 (cap>=1,000) as more
of the eligible population's own real defects (the same brand-exact-match
and color-vocabulary-noise issues P2-E15 catalogued) get admitted at
looser caps.

### Result 4 -- native execution latency is extremely fast, confirming the core mechanism

30 reps x 20 small-candidate-set (<=10) queries, individual per-query
timing: mean=0.0011ms, p50=0.0011ms, p99=0.0032ms, max=0.0194ms. Even
faster than Phase 2's own ~0.02ms `structural_exact_entity` FastPath
number (expected: this sample is drawn from queries with even smaller
candidate sets). Combined with P3-E01's near-zero fallback tax, every
query's cost in this architecture is now characterized: admitted ~
0.001ms, rejected ~ solr_baseline (already measured, ~2.5ms mean). Given
coverage tops out at 0.81% (Result 1), the *aggregate* whole-workload
latency distribution will not materially shift versus a pure-Solr
deployment on this real corpus -- RQ4's "does p50 move onto the native
path" hypothesis requires materially higher coverage than this dataset's
current semantic-resolution rate supports, not a looser selectivity cap.

### Decision: KEEP the mechanism; the frontier is now well-characterized; proceed to coverage expansion (P3-E03+)

Per Issue #14's own framing ("if coverage stays small, narrow the claim
instead of overstating it"): on *this* real dataset, with the *current*
baseline lexicon (no P1-B/P1-C enhancements), the safe-admission
architecture is real, cheap, and relevance-safe, but capped at ~0.81%
coverage -- not enough to move the whole-workload latency distribution.
The highest-information next experiment is not further selectivity-cap
tuning (exhausted by this sweep) but attacking the dominant rejection
reason directly: unresolved residual lexical text (76.89% of all real
traffic, by far the largest of the three rejection reasons). Per Issue
#14's own P3-E03+ loop ("identify opportunity -> add RED/adversarial
tests -> implement smallest semantic/compiler improvement -> replay full
corpus -> rerun latency campaign -> update frontier -> KEEP/REJECT").

## P3-E03 — native token-verified lexical narrowing: REJECT (real evidence, after fixing a real bug)

**Evidence class**: real (full 1,215,854-product catalog, full
22,458-query judged corpus, live local Solr) -- one deterministic pass,
same methodology as P3-E02.

**Hypothesis**: the dominant rejection reason (unresolved residual
lexical text, 76.89% of traffic) can be safely narrowed if every residual
token is verified against Round 1's native `lexical_and_candidates`
token-postings index (no delegate call) and the combined structural+
lexical candidate set stays small -- the same "small candidate set is
safe without a ranking signal" principle `admit`'s own `max_candidates`
cap already relies on, applied to a second signal.

**Method**: `commerce_core::admission::admit_lexically_narrowed`/
`execute_lexically_narrowed` (additive, does not touch the existing
`admit`/`AdmissionPolicy` contract) plus `CatalogIndex::execute_ranked_narrowed_by`.
A pre-implementation diagnostic (`p3e03_residual_lexical_diagnostic`, no
Solr needed) found this *could* newly make 54.02% of residual-rejected
queries (41.54% of the whole corpus) safe-admissible under a combined
cap<=250 -- candidate-set-size promise only, explicitly flagged as
insufficient on its own (no ranking signal exists on this path either,
the same open risk P2-E17/P3-E02 already found for the original
mechanism). `crates/phase3-eval/src/bin/p3e03_lexical_narrowing_eval.rs`
supplies the missing real NDCG@10/Recall@10/MRR evidence: for every query
`admit()` rejects `UnresolvedResidual`, sweep `max_lexical_narrowed_candidates`
over the same log-scale points P3-E02 used, comparing native relevance
against both the real ESCI judgments and what Solr actually returns for
the same queries. The whole-workload metric isolates this mechanism's own
marginal contribution (every query it does not admit, including ones the
*original* structural `admit()` would separately admit, scores as a Solr
fallback) since the two admission paths are disjoint by construction.

### A real bug, found and fixed before trusting the first run's numbers

The first run (raw output preserved; superseded by the fixed re-run below)
reported whole-workload NDCG degradation of **+0.0220 absolute even at the
most conservative swept cap** (`max_lexical_narrowed_candidates=1`) -- far
beyond every one of Issue #14's four target budgets, worse than expected
even accounting for the known missing-ranking-signal risk. Per this
project's own "any benchmark anomaly is a bug investigation, not noise to
ignore," this was checked against the raw per-query artifact before being
treated as a verdict on the mechanism itself.

Root cause, confirmed directly against `eligible_queries_raw.csv`: **24.9%
of "eligible" queries (3,908/15,702) had a combined structural+lexical
candidate count of exactly zero** -- every residual token individually
known somewhere in the catalog, but their AND-combination (or the AND-
combination with an existing structural constraint) empty. `admit_lexically_narrowed`'s
cap check (`count as usize > max_lexical_narrowed_candidates`) never
special-cased `count == 0`, so these queries were silently "admitted" to
a guaranteed-empty native result at *every* swept cap (0 is never greater
than any cap). 994 of those 3,908 had Solr find a real relevant result the
native path would have returned nothing for. This is the exact same unsafe
claim the function's own doc comment already rejected for a single
out-of-vocabulary token ("makes the whole query ineligible... rather than
safely narrows to zero candidates") -- just never applied to the combined
count itself.

Fixed RED-first: added
`rejects_lexical_narrowing_when_every_token_is_known_but_the_combined_set_is_empty`
(two catalog-fixture tokens, "runner" and "boot", each individually known
but whose AND is empty), confirmed it failed against the old code, then
added `count == 0` to the rejection check. 15/15 admission tests pass.
Full gate green (fmt, clippy `-D warnings`, 137 workspace tests, release
build). This does not by itself establish the mechanism is safe -- it
only removes one confirmed source of inflated coverage/degradation so the
real-data measurement could be rerun honestly.

### The corrected result: still REJECT, at every swept cap

Post-fix (`docs/research/artifacts/p3e03_run1/`, superseding the buggy
first run): of the 17,268 residual-rejected queries, 5,474 (31.7%) are now
correctly blocked outright (an out-of-vocabulary token, or a known-but-
empty AND-combination -- internally consistent with the pre-fix run's own
1,566 genuinely-OOV-token count plus the 3,908 newly-caught zero-combined
count: 1,566 + 3,908 = 5,474 exactly). 11,794 queries have a real,
non-zero combined candidate count, swept below.

| cap | admitted | coverage (% of whole corpus) | native NDCG (admitted) | Solr NDCG (same admitted) | whole-workload NDCG | degradation vs. Solr-only | false-positive admissions |
|---|---|---|---|---|---|---|---|
| 1 | 783 | 3.49% | 0.1456 | 0.2902 | 0.2285 | +0.0050 (2.14% relative) | 126 (16.09%) |
| 20 | 5,097 | 22.70% | 0.3426 | 0.4478 | 0.2096 | +0.0239 (10.24% relative) | 446 (8.75%) |
| 250 | 9,328 | 41.54% | 0.2274 | 0.3739 | 0.1727 | +0.0608 (26.04% relative) | 2,720 (29.16%) |
| 200,000 | 11,791 | 52.50% | 0.1805 | 0.3184 | 0.1611 | +0.0724 (31.0% relative) | 3,859 (32.73%) |

Whole-workload pure-Solr-only baseline: NDCG@10 = 0.2335 (identical to
P3-E02's, same corpus/Solr instance). **Every single swept cap value fails
every one of Issue #14's four target budgets (0%, 0.5%, 1%, 2%)** -- the
RQ2 calibration loop reports "no swept cap value stays within this
budget" at all four thresholds. Even the single best point in the entire
sweep, `max_lexical_narrowed_candidates=1` (as conservative as this
mechanism can be), overshoots the loosest 2% budget (2.14% relative
degradation). Coverage is real and large (up to 52.50% of the whole
corpus, confirming the diagnostic's candidate-set-size promise was not
wrong on its own terms), but relevance cost grows with it, not against
it: native NDCG on the admitted subset actually peaks around cap=20
(0.3426) and *declines* as the cap loosens further, while degradation and
false-positive rate climb monotonically and substantially at every point.

**Why, in terms this project can act on**: `execute_ranked_narrowed_by`
has no ranking signal (same as every other Phase 2/3 FastPath execution
path) -- ties break on ascending `(product_id, variant_id)`. `admit`'s own
structural constraints (`Brand=X`, `ProductType=Y`) are a strong precision
signal for relevance almost by construction, which is why a small
structurally-admitted candidate set tolerates having no ranking signal
reasonably well (P3-E02). Independent per-token presence-in-title
verification is a much weaker signal: it establishes that a word
*appears somewhere* in a product's indexed text, not that the product
matches the query's actual compositional intent. Representative real
examples from the false-positive set (not individually root-caused the
way P2-E15's diaper-attribute gap was, but illustrative of the pattern):
qid 553 "06 trailblazer headlight without full grille bar" contains an
explicit negation ("without full grille bar") that independent token
verification cannot represent -- it can only confirm "grille" and "bar"
*appear*, which selects toward the opposite of what the shopper excluded;
qid 3018 "24 volt electric plug trolling motor" and qid 4459 "4x4 inch
gauze pads" combine several individually-common technical/numeric tokens
whose co-occurrence in one product's title does not reliably imply that
product is the one being searched for. Coverage growing while relevance
degrades faster (rather than the flat-cost shape P3-E02's structural
mechanism showed) is consistent with this: looser caps admit queries
whose "safe" candidate set is large specifically *because* the tokens
verified are generic, which is exactly when a missing ranking signal
matters most.

### Decision: REJECT this mechanism as evaluated; preserve the evidence, do not fold it into the admission path

Naive per-token presence verification with AND-narrowing and no ranking
signal does not clear Issue #14's relevance-budget bar at any coverage
point on this real dataset -- not a narrow miss, but a miss by 4-15x at
every budget threshold once every swept cap is examined. This is a
genuine negative result, preserved rather than iterated away: the
mechanism code, its 15 tests, and this measurement stay in the tree
(`admit_lexically_narrowed`/`execute_lexically_narrowed` are additive and
harmless when unused -- nothing calls them from the production admission
path), but they are **not** wired into `admit`/`AdmissionPolicy`, and
coverage expansion should not pursue this specific lever further without
new evidence that changes the diagnosis (e.g. a real ranking signal on
the narrowed candidate set, or restricting to phrase-level rather than
independent-token verification).

The one confirmed, durable finding from P3-E03 worth carrying forward:
**candidate-set-size promise is not sufficient evidence for a coverage
lever's safety, and this project's own discipline of measuring real
relevance before promoting a candidate-set-size diagnostic is exactly
why this REJECT was caught before being shipped as a KEEP.** The
already-queued P1 follow-ups (#16 -- learned semantic implication rules;
#17 -- browse/PLP structural benchmark) attack coverage expansion along
different axes than naive token verification and remain independently
worth pursuing; #16 in particular targets exactly the gap this experiment
exposes (a real semantic/ranking signal, rather than raw token
co-occurrence, behind any new admission).

**Next**: continue Issue #14's loop by identifying the next-highest-
information experiment. The dominant rejection reason (unresolved
residual) still accounts for 76.89% of traffic and remains the largest
lever by volume, but naive token verification is now a closed, documented
dead end for it -- the next attempt on this reason needs a real
relevance/ranking signal, not just a narrower candidate set.
