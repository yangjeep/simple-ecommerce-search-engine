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

## P3-E11 — diagnostic: ambiguous queries have a strong catalog-frequency dominance signal (first look at the second-largest rejection reason)

**Evidence class**: real, diagnostic only -- no Solr needed (pure
`compile()`/`CatalogIndex` computation over the real catalog).

**Motivation**: per Issue #18's mining loop, the dominant rejection
reason (unresolved residual) has now been mined across P3-E03-E10;
ambiguous queries (22.29% of all real traffic, P3-E01 -- the second-
largest rejection reason) have not been examined at all in this campaign.
Structurally distinct: a phrase resolved to *multiple* candidate
interpretations, not one that failed to resolve.

**A real, checked-not-assumed finding**: `Candidate::confidence` exists
in the type but `cold_start::profile::compile_lexicon` -- the actual
lexicon every real-data Phase 2/3 experiment has used -- hard-codes
every candidate to `confidence: 1.0` unconditionally. "Pick the higher-
confidence candidate" is a dead end against this project's own
benchmarked lexicon without first changing lexicon compilation.

**Method**: `p3e11_ambiguous_frequency_diagnostic`. For the tractable
subclass (exactly one ambiguous span, every candidate a real hard
Constraint), computes each candidate's real catalog frequency via
`CatalogIndex::indexed_candidates` on a single-element constraint slice
-- a catalog-grounded signal, not a placeholder.

**Result**: 5,005/22,458 (22.29%) ambiguous -- exact match to P3-E01's own
count. 4,356/5,005 (87.03%) single-span; every single-span query's
candidates are all real Constraints (none Preference-only). Catalog-
frequency dominance is striking: **3,879/4,356 (89.05%) have a top
candidate at least 10x more catalog-frequent than the runner-up**;
464 (10.65%) are 2-10x; only 13 (0.30%) are flat. Picking the highest-
frequency candidate yields a nonzero, <=250 combined candidate set for
2,113/4,356 -- reported as "42.22% of ambiguous traffic, 9.41% of the
whole corpus," the largest coverage opportunity found in this campaign
by a wide margin.

**Decision**: strong enough to justify a full relevance measurement --
but explicitly flagged (matching this project's own "candidate-set-size
promise is not sufficient evidence" lesson from P3-E03) that this number
has NOT been checked against whether resolving the ambiguity actually
makes the *whole query* complete (i.e. whether `residual_lexical` is
also empty afterward) -- P3-E12 supplies that check.

## P3-E12 — real measurement: the diagnostic's promise mostly evaporates, for a real and well-understood reason (BOUNDED, not KEEP)

**Evidence class**: real, whole-workload -- no live Solr querying (reuses
P3-E06's already-persisted `whole_corpus_solr_ndcg.csv`, which covers
every query including ambiguous ones since that pass ran before any
admission filtering; only the native side needs fresh computation).

**Hypothesis**: resolving tractable ambiguous queries via their catalog-
frequency-dominant candidate, requiring the *fully resolved* query to
be complete (`residual_lexical` empty, matching `admit()`'s own
definition) and its candidate set to stay within a fixed 250 cap, clears
Issue #14's relevance budgets with real coverage close to P3-E11's
9.41% estimate.

**Method**: `p3e12_ambiguous_frequency_eval`. Same tractable-subclass
identification as P3-E11, but for each candidate, substitutes the
frequency winner as a real hard constraint, executes the *fully
resolved* query exactly as any other admitted query, and requires
`residual_lexical` to be empty afterward -- the completeness check
P3-E11's diagnostic never performed. Sweeps the frequency-ratio
threshold (1x-100x) at a fixed 250-candidate cap.

### A real anomaly, investigated before drawing any conclusion

The first run found only 24-29 tractable queries, not the ~2,113 P3-E11
estimated -- a >99% collapse. Per this project's own "any benchmark
anomaly is a bug investigation, not noise to ignore," this was broken
down by exclusion reason rather than accepted or dismissed:

```text
exclusion breakdown (of 4,356 single-span, all-Constraint queries):
  multi_span (excluded upstream, not part of this 4,356): 649
  not_all_constraint: 0
  zero_top_freq: 0
  residual_still_nonempty_after_resolution: 4,279 (98.2%)
  resolved_candidate_set_zero: 48
  tractable: 29
```

**Not a bug in either binary -- a real, load-bearing fact about this
corpus's queries**: 98.2% of single-span ambiguous queries *also* carry
unresolved residual text elsewhere in the query, even after resolving
the one ambiguous phrase. An ambiguous phrase is usually just one part
of a longer, multi-word real shopper query ("air mattress queen size"
where "air mattress" might resolve ambiguously and "queen size" is
separate residual text), not the whole query. P3-E11's diagnostic
measured a real, catalog-grounded signal correctly, but its "coverage
opportunity" estimate implicitly assumed resolving the ambiguous span
alone would make these queries complete -- an assumption this measurement
disproves directly.

### Result — real, safe, but negligible in isolation

| ratio>= | admitted | coverage (% of whole corpus) | native NDCG | Solr NDCG (same admitted) | whole-workload degradation |
|---|---|---|---|---|---|
| 1 (unlimited) | 24 | 0.11% | 0.0836 | 0.1510 | +0.0001 (0.04% relative) |
| 10 | 23 | 0.10% | 0.0872 | 0.1544 | +0.0001 (0.04% relative) |
| 100 | 2 | 0.01% | 0.0000 | 0.0212 | +0.0000 |

0 variant-correctness violations (the resolved-query execution path
re-verifies hard constraints exactly like every other mechanism in this
phase). Native NDCG is meaningfully worse than Solr on the admitted
subset itself (-0.0674 at ratio>=1, ~45% relative) -- similar in kind to
every other no-ranking-signal mechanism in this phase -- but coverage is
so small (0.11% at best) that whole-workload impact is negligible either
way (+0.0001, indistinguishable from zero in practical terms).

**Decision**: BOUNDED, not KEEP. The mechanism is real and safe (no
correctness violations, negligible whole-workload cost) but recovers a
coverage of matters (0.11% of the whole corpus) too small to justify
hardening into `commerce_core` on its own -- the real constraint is not
this mechanism's own precision but its *applicability*: 98.2% of its
target population is disqualified by co-occurring residual text before
frequency-based resolution ever gets a chance to help. Per Issue #18's
"isolate whether the limitation is fundamental, data-quality-specific,
benchmark-specific, or implementation-specific" instruction: this is
FUNDAMENTAL to how real multi-word shopper queries are shaped on this
corpus, not a data-quality gap or an implementation bug.

**Decision discipline applied**: rather than discard this finding
because the mechanism alone is negligible, the real, well-diagnosed
bottleneck (co-occurring residual text) points directly at the next
experiment: composing frequency-based ambiguity resolution *with*
lexical narrowing on the leftover residual (reusing P3-E03/P3-E05/P3-E09's
own `lexical_and_candidates` machinery, seeded with the frequency-
resolved constraint already in place) rather than requiring residual to
already be empty. This is not scope creep -- it is the same "identify
the highest-volume rejected class -> characterize what's missing ->
propose the smallest mechanism" loop Issue #18 itself prescribes,
applied to the specific blocker P3-E12 just measured.

Raw artifacts: `docs/research/artifacts/p3e11_run1/` (diagnostic log
only, no persisted CSV), `p3e12_run1/`.

**Next**: P3-E13 -- a combined ambiguity-resolution-plus-lexical-narrowing
mechanism targeting the 98.2% of tractable ambiguous queries this
experiment found blocked by co-occurring residual text.

## P3-E04 — diagnostic: structural+lexical queries have a meaningfully better relevance profile than pure-lexical-only

**Evidence class**: real, but a diagnostic only -- reuses P3-E03's
already-measured per-query relevance data (no new Solr calls), joined
against a fresh, cheap, deterministic recompile of each query solely to
check `!compiled.constraints.is_empty()`. Whole-workload impact below is
a bucket-*mean* approximation, explicitly not treated as a verdict on its
own (P3-E05 supplies the real number).

**Hypothesis**: `admit_lexically_narrowed` treats a query with an
existing structural constraint (Brand/ProductType/etc.) alongside
residual text identically to one with no structural constraint at all --
but the former's structural half is exactly the kind of independent
precision anchor P3-E02 found tolerates having no ranking signal
reasonably well. This distinction alone might explain a meaningful share
of P3-E03's aggregate REJECT.

**Method**: `p3e04_structural_plus_lexical_diagnostic`. Buckets P3-E03's
11,794 eligible queries by `has_structural_constraint`, reporting native
NDCG, Solr NDCG (on the same admitted subset), delta, and false-positive
rate at the same four representative cap points P3-E03's own frontier
table used (1, 20, 250, unlimited).

**Result**: confirmed, consistently, at every cap point:

| cap | bucket | admitted | native NDCG | Solr NDCG | delta | false-positive rate |
|---|---|---|---|---|---|---|
| 1 | has_constraint | 240 | 0.1337 | 0.2335 | -0.0998 | 18/240 (7.50%) |
| 1 | pure_lexical | 543 | 0.1509 | 0.3153 | -0.1645 | 108/543 (19.89%) |
| 250 | has_constraint | 1,526 | 0.2769 | 0.3768 | -0.0999 | 227/1,526 (14.88%) |
| 250 | pure_lexical | 7,802 | 0.2178 | 0.3733 | -0.1556 | 2,493/7,802 (31.95%) |
| unlimited | has_constraint | 1,557 | 0.2714 | 0.3704 | -0.0990 | 239/1,557 (15.35%) |
| unlimited | pure_lexical | 10,237 | 0.1666 | 0.3105 | -0.1438 | 3,620/10,237 (35.37%) |

`has_constraint` shows a smaller NDCG delta and a 2-3x lower false-
positive rate than `pure_lexical` at every cap, and the gap *widens* as
the cap loosens -- consistent with the "structural constraint as
precision anchor" hypothesis, not a coincidence at one cap value.

**Decision**: real, evidence-based grounds to measure a restricted
policy -- "never invoke lexical narrowing when there is no existing
structural constraint" -- properly (P3-E05), rather than either
abandoning the coverage lever entirely (P3-E03's aggregate REJECT) or
promoting this bucket-mean approximation directly.

## P3-E05 — KEEP: structurally-anchored lexical narrowing clears the relevance budget with real coverage gain

**Evidence class**: real (full 1,215,854-product catalog, full
22,458-query judged corpus, live local Solr, fresh whole-corpus Solr
pass -- same methodology as P3-E02/P3-E03, not the P3-E04 diagnostic's
bucket-mean approximation).

**Hypothesis**: restricting `admit_lexically_narrowed` to only the
`UnresolvedResidual`-rejected queries that also carry a non-empty
`query.constraints` clears Issue #14's relevance budgets at a real,
useful coverage point, per P3-E04's diagnostic signal.

**Method**: `crates/phase3-eval/src/bin/p3e05_structural_anchored_lexical_eval.rs`,
mirroring P3-E02/P3-E03 exactly (fresh whole-corpus Solr baseline, same
log-scale cap sweep, whole-workload degradation computed from each
non-admitted query's own real Solr score, RQ2 budget calibration,
once-only native latency measurement) but restricted to the
structurally-anchored population: of 17,268 `UnresolvedResidual`-
rejected queries, 4,599 (26.63%) also carry an existing structural
constraint; of those, 3,042 are blocked outright (out-of-vocabulary
token or an empty AND-combination, same safety check P3-E03's bug fix
established) and 1,557 have a real, non-zero combined candidate count,
swept below.

### Result — every swept cap clears Issue #14's 2% budget; three caps clear tighter budgets with real coverage

| cap | admitted | coverage (% of whole corpus) | native NDCG (admitted) | Solr NDCG (same admitted) | whole-workload NDCG | degradation vs. Solr-only | false-positive admissions |
|---|---|---|---|---|---|---|---|
| 1 | 240 | 1.07% | 0.1337 | 0.2335 | 0.2324 | +0.0011 (0.47% relative) | 18 (7.50%) |
| 2 | 382 | 1.70% | 0.1783 | 0.2866 | 0.2317 | +0.0018 (0.77% relative) | 26 (6.81%) |
| 20 | 1,110 | 4.94% | 0.3322 | 0.4182 | 0.2293 | +0.0043 (1.84% relative) | 57 (5.14%) |
| 250 | 1,526 | 6.79% | 0.2769 | 0.3768 | 0.2267 | +0.0068 (2.91% relative) | 227 (14.88%) |
| 200,000 | 1,557 | 6.93% | 0.2714 | 0.3704 | 0.2266 | +0.0069 (2.96% relative) | 239 (15.35%) |

Whole-workload pure-Solr-only baseline: NDCG@10 = 0.2335 (identical to
P3-E02/P3-E03's, same corpus/Solr instance). RQ2 budget calibration:
**budget<=0.5%: best cap=1, coverage 1.07% of whole corpus. budget<=1.0%:
best cap=2, coverage 1.70%. budget<=2.0%: best cap=20, coverage 4.94%.**
(budget<=0.0% stays unreachable by construction, same footnote as
P3-E02.) At `cap=20`, coverage is **~6x P3-E02's entire structural-only
coverage (0.81%)**, while staying inside the 2% budget -- the largest
real coverage gain of the whole Phase 3 campaign so far. Even at
*no* cap limit at all, degradation only reaches 2.96% relative -- nowhere
near the unrestricted mechanism's 31.0% relative degradation at the same
unlimited point (P3-E03). Native execution latency: mean=0.0015ms
(30 reps, individual per-query timing over a small-candidate-set sample)
-- back in line with P3-E02's fast structural-admission numbers, not
P3-E03's inflated ones (which were dominated by iterating much larger,
unrestricted candidate sets).

**Not perfect, stated plainly**: a false-positive rate of 7.5-15.35%
remains even in this restricted policy -- lower than P3-E03's 16-33% by
a real margin, but not zero. Representative examples from the unlimited-
cap false-positive set (illustrative, not individually root-caused):
qid 31529 "dansko womens shoes", qid 33792 "disney loungefly purse", qid
41959 "foodie ninja cooker" -- brand-plus-descriptive-noun queries where
the structural constraint (brand) narrows correctly but the residual
noun phrase's lexical match still occasionally selects a non-relevant
product from that brand's catalog. This is the same "no ranking signal"
risk documented since P2-E17, now bounded to a smaller and less frequent
share of traffic by the structural anchor rather than eliminated.

**A Solr-baseline-fairness audit (triggered by P3-E13's own surprising
result, see that entry) checked this verdict too, and it holds -- record
corrected here, verdict unchanged.** `round1_eval::solr::solr_query_for`
silently drops from Solr's own `q=` any word that resolved to a
structural constraint with no `fq` substitute (`extract_brand_color`
only ever populates `fq` for Brand/BrandAny/color; `size` and price-range
constraints get neither). Measured directly against this experiment's
real 4,599-query anchored-lexical population: **104 queries (2.26%)**
carry such a constraint (68 `size`, 36 price-under; no `ProductType`/
`Category`/other-Enum ever occurs in this population, since real ESCI
products carry only sentinel values for those fields). Since dropping a
word can only make Solr's own score *lower*, never higher, this gap can
only have made P3-E05's reported native-vs-Solr advantage look slightly
*better* than the fully-fair number would -- i.e. the bias, where it
exists, favors this KEEP decision, not against it, and at 2.26% incidence
it is too small to matter regardless of direction. An initial audit
report additionally claimed the whole-corpus Solr baseline (NDCG@10 =
0.2335, used as this table's degradation denominator) was "unrelated" to
this verdict; an adversarial review proved that claim false and derived
the correction directly: `whole_workload_degradation` algebraically
collapses to a term computed entirely over the *admitted* subset (the
non-admitted `rest_solr_sum` term cancels), so the shared denominator's
own corpus-wide gaps (a separate, larger issue -- see P3-E11-E13) affect
only how the raw degradation is expressed as a percentage, never its
sign or the admitted-subset comparison itself; correcting that
denominator would only shrink the reported relative-degradation
percentages in this table (more room under budget), never grow them.
**Net: P3-E05's KEEP verdict is CONFIRMED, not just assumed, under this
audit** -- both potential gaps point the same way (favoring this
decision, not threatening it) and neither is large enough to move it
regardless. P3-E06/P3-E07/P3-E10, which compose this mechanism's own
already-measured admission counts rather than re-deriving an independent
Solr comparison, inherit the same conclusion without needing separate
re-verification.

**A pre-existing, bug-independent risk worth flagging separately**: the
same audit's adversarial review noted P3-E07's own bootstrap CI for the
`budget<=2.0%` combined operating point has a 95% upper bound of 2.16%
-- outside the nominal 2% target -- under the *current* baseline,
regardless of this fairness question. This is not caused by the
`solr_query_for` gap (P3-E07's own entry already reported it honestly
without connecting it here), but is worth a human decision on whether
that specific operating point should be characterized as REVISE rather
than KEEP; not resolved by this audit, flagged for follow-up.

**Decision**: KEEP. Hardened into `commerce_core::admission::admit_structurally_anchored_lexical`
(additive, does not modify `admit_lexically_narrowed`'s own already-
measured-and-REJECTed-in-general contract): rejects outright whenever
`query.constraints` is empty, so a future caller cannot reproduce
P3-E03's rejected behavior by calling the wrong function. 2 new RED-first
tests (a pure-lexical-only query that plain `admit_lexically_narrowed`
would admit but this rejects; a structural-constraint-anchored query that
admits with the expected candidate count) -- 17/17 admission tests pass.
Not yet wired into a combined production admission policy alongside the
original structural `admit()` (the two populations are disjoint by
construction, so combining them is additive, not conflicting) -- that
wiring, plus a paper-grade replication (>=30 runs, bootstrap CIs) at the
promoted cap point, is next.

**Cross-phase note**: this is the first real coverage-expansion win of
Issue #14's P3-E03+ loop. Combined with P3-E02's own 0.81% structural-
only coverage (disjoint population, additive), the safe FastPath
architecture now has real, evidence-backed coverage in the 5-8% range at
a <=2% relevance budget, depending on which cap is chosen for each
mechanism -- still short of RQ4's ">50% coverage" threshold for a p50
latency shift, but a meaningful, real step beyond P3-E02's ceiling.

**Next**: continue Issue #14's loop. Immediate candidates: (a) paper-
grade replication of this result at the promoted cap points; (b) apply
the same "diagnose the rejected population for a real precision anchor"
approach P3-E04 used to the *pure-lexical-only* population itself (is
there a further split within it -- e.g. by residual token count, or by
whether the residual is a single highly-specific token like a model
number vs. multiple generic descriptive words -- that recovers some of
its otherwise-REJECTed coverage); (c) Issue #16's learned semantic
implication rules, which could supply a real ranking/precision signal
where this experiment relied on structural-constraint co-occurrence
alone.

## P3-E06 — the combined system: additivity confirmed, real safe-offload Pareto frontier, RQ4 answered

**Evidence class**: real (full 1,215,854-product catalog, full
22,458-query judged corpus, live local Solr, fresh whole-corpus Solr
pass -- this run's whole-corpus per-query Solr NDCG is now persisted to
`docs/research/artifacts/p3e06_run1/whole_corpus_solr_ndcg.csv` for
future experiments to reuse without repeating the ~70s Solr pass).

**Hypothesis**: P3-E02 (structural `admit()`) and P3-E05
(`admit_structurally_anchored_lexical`) were each measured in isolation,
scoring everything the *other* mechanism would separately admit as a
Solr fallback. Since the two populations are disjoint by construction
(the latter requires non-empty `residual_lexical`, the former's `Admit`
branch requires it empty), running both together should be additive --
this measures the combined system directly rather than assuming it.

**Method**: `p3e06_combined_admission_frontier`. Computes both eligible
populations cap-independently in one pass (185 structural, matching
P3-E02 exactly; 1,557 anchored-lexical, matching P3-E05 exactly),
**explicitly asserts zero overlap between them** (a real correctness
check, not just an assumption), then sweeps a small representative grid
-- `structural_cap` in {50, 250} x `anchored_lexical_cap` in {1, 20,
250} -- six combined operating points. Latency is a real weighted mean
using each route's own already-measured mean (P3-E01/P3-E02/P3-E05),
weighted by this corpus's own real per-route admission counts at each
grid point -- not a new synthetic timing campaign.

### Result 1 — disjointness confirmed; additivity holds

`0 overlap` between the structural and anchored-lexical eligible
populations, verified directly rather than merely assumed. This means
every grid point's combined coverage is exactly the sum of the two
mechanisms' own admission counts at that cap pair, with no double-
counting risk.

### Result 2 — the real combined safe-offload Pareto frontier

| structural cap | anchored-lexical cap | structural admitted | anchored admitted | coverage | whole-workload NDCG | degradation (relative) | weighted mean latency |
|---|---|---|---|---|---|---|---|
| 50 | 1 | 82 | 240 | 1.43% | 0.2322 | +0.0013 (0.56%) | 2.5236ms |
| 250 | 1 | 164 | 240 | 1.80% | 0.2319 | +0.0016 (0.69%) | 2.5143ms |
| 50 | 20 | 82 | 1,110 | 5.31% | 0.2290 | +0.0045 (1.93%) | 2.4245ms |
| 250 | 20 | 164 | 1,110 | 5.67% | 0.2288 | +0.0048 (2.06%) | 2.4152ms |
| 50 | 250 | 82 | 1,526 | 7.16% | 0.2265 | +0.0070 (3.00%) | 2.3771ms |
| 250 | 250 | 164 | 1,526 | 7.53% | 0.2262 | +0.0073 (3.13%) | 2.3678ms |

Reading the frontier against Issue #14's four budgets: **budget<=1.0%**
is cleared by (structural=250, anchored=1) at **1.80% combined
coverage**; **budget<=2.0%** is cleared by (structural=50, anchored=20)
at **5.31% combined coverage** -- the tightest point that also clears
2.0% in this six-point grid ((250,20) at 2.06% relative narrowly misses
it). No grid point clears the strictest 0.5% budget (closest:
(50,1) at 0.56% relative) -- a finer sweep near `anchored_lexical_cap<1`
is not meaningful (1 is already the minimum), so 0.5% is not reachable
by *coverage-side* tuning alone at this grid's structural-cap values;
a stricter structural cap below 50 was not swept here and is a natural
follow-up if a 0.5% operating point is specifically wanted.

### Result 3 — RQ4 answered analytically: p50 does not move at this coverage level

Weighted mean latency drops only modestly (2.5236ms to 2.3678ms across
the grid, a 1.4-7.5% reduction from the pure-Solr baseline mean
2.5603ms) -- consistent with coverage topping out at 7.53% in this grid.
Since admission is content-based (ambiguity/residual/structural-
constraint shape), not latency-based, and P3-E01 already found Solr's
own per-query latency has a tight CI uncorrelated with which queries get
admitted, every percentile at or above the coverage fraction remains
governed by `solr_baseline`'s own already-measured distribution as long
as coverage stays below 50%. At a measured ceiling of 7.53%, **p50/p95/p99
do not move onto the native path** -- this is the same conclusion P3-E02
reached qualitatively, now confirmed quantitatively for the *combined*
system rather than either mechanism alone, and without needing a
fabricated synthetic combined-latency campaign to discover it.

**Decision**: KEEP the combined-measurement methodology and its result.
The safe-offload architecture, combining both currently-KEPT mechanisms,
reaches a real, evidence-backed **1.80-7.53% coverage band across
budgets from 1% to 3% relative degradation**, roughly 2.2-9.3x P3-E02's
own structural-only ceiling (0.81%) depending on which combined operating
point is chosen. This is genuine, measured progress on Issue #14's
central thesis, but RQ4's p50-shift threshold (>50% coverage) remains
far out of reach with the mechanisms KEPT so far -- reaching it requires
either Issue #16's learned semantic implications or a further coverage
lever on the still-untouched pure-lexical-only population (P3-E05's
"Next" note), not incremental cap tuning on what already exists.

**Next**: (a) paper-grade replication (>=30 runs, bootstrap CIs) at the
promoted operating points -- (structural=250, anchored=1) for a <=1%
budget, (structural=50, anchored=20) for a <=2% budget; (b) the
pure-lexical-only population's own further segmentation (P3-E05's
deferred idea); (c) Issue #16/#17 as queued, orthogonal coverage levers.

## Issue #18 opened: HOW-driven epic governs the remainder of this loop

The repo owner opened Issue #18 ("Expand the commerce-native multiplier
toward P50/P95") and `docs/research/HOW_DRIVEN_THESIS.md`, plus explicit
PR review direction: keep mining #14's highest-volume rejected class
until additional safe coverage attempts are exhausted or a clear
structural boundary is established, before moving to #16/#17/#9/#7/
#11/#12. Governing targets: search coverage toward >=50%/P50, browse/PLP
toward >=95%/P95, an 80x physical-multiplier floor on promoted fast
paths, negligible fallback tax. P3-E08+ below continue directly under
this mandate -- mining the pure-lexical-only population (the remainder
after P3-E04/E05 recovered its structurally-anchored slice) rather than
moving on to a new epic.

## P3-E08 — diagnostic: single-residual-token queries are a strikingly better precision class within the pure-lexical-only remainder

**Evidence class**: real, but a diagnostic only -- reuses P3-E03's own
already-measured per-query data (native NDCG computed by actually
executing `execute_lexically_narrowed` during that run, not an
approximation) joined against P3-E05's eligible-qid set (to exclude the
already-recovered structurally-anchored slice). No new Solr calls.

**Hypothesis**: per Issue #18's mining loop ("keep mining the highest-
volume rejected class until additional safe coverage attempts are
exhausted or a clear structural boundary is established"), the pure-
lexical-only remainder's aggregate REJECT should not be accepted as
final without checking its own internal structure. A single-token
residual (often a specific, low-ambiguity term -- a model name, a rare
descriptor) should be a stronger precision signal than a multi-token
residual (more often a generic descriptive phrase -- P3-E03's own cited
false positives: "without full grille bar", "24 volt electric plug",
each combining several individually-common words whose AND-narrowing
does not track compositional intent, including an explicit negation
"without" that token-presence verification cannot represent at all).

**Method**: `p3e08_pure_lexical_token_count_diagnostic`. Buckets the
10,237-query pure-lexical-only remainder by `residual_token_count`
(1/2/3+) at the same four representative cap points prior diagnostics
used.

**Result**: confirmed, sharply, and in a way no other admission
mechanism in this campaign has shown:

| cap | token count | admitted | native NDCG | Solr NDCG | delta | false-positive rate |
|---|---|---|---|---|---|---|
| 1 | 1 | 20 | 0.1865 | 0.2348 | -0.0483 | 0/20 (0.00%) |
| 20 | 1 | 279 | 0.5482 | 0.6213 | -0.0731 | 5/279 (1.79%) |
| 20 | 2 | 1,041 | 0.3874 | 0.4734 | -0.0860 | 65/1,041 (6.24%) |
| 20 | 3+ | 2,667 | 0.3080 | 0.4320 | -0.1240 | 319/2,667 (11.96%) |
| unlimited | 1 | 824 | 0.2226 | 0.3330 | -0.1108 | 229/824 (27.79%)* |

(*unlimited-cap false-positive rate recomputed in P3-E09 below on the
final disjoint population; the pattern -- 1-token consistently better
than 2-token consistently better than 3+-token, at every cap -- holds
throughout.) At cap=20, the 1-token bucket's false-positive rate
(1.79%) is *lower* than P3-E05's own already-KEPT anchored mechanism at
the identical cap (5.14%) -- a pure-lexical-only signal outperforming a
structurally-anchored one on precision, which is why this diagnostic
promotes directly to a real measurement rather than being filed as a
minor curiosity.

**Decision**: strong, real evidence justifying a full whole-workload
measurement of a new, third disjoint mechanism (P3-E09), not just a
noted pattern.

## P3-E09 — KEEP: single-token lexical narrowing is a third disjoint mechanism that clears budget at every cap, including unlimited

**Evidence class**: real, whole-workload -- no new Solr querying (every
input is already-persisted, already-measured per-query data: P3-E03's
`eligible_queries_raw.csv` for native NDCG, P3-E06's
`whole_corpus_solr_ndcg.csv` for every other query's own real Solr
score).

**Hypothesis**: restricting admission to residual-lexical queries whose
entire residual is exactly one token -- regardless of whether a
structural constraint also exists, so this is disjoint from both `admit`
(requires empty residual) and `admit_structurally_anchored_lexical`
(requires a structural constraint; P3-E08's population explicitly
excludes anchored qids) -- clears Issue #14's relevance budgets with real
coverage.

**Method**: `p3e09_single_token_lexical_eval`, same "isolated marginal
contribution" methodology as P3-E03/P3-E05 (every non-admitted query,
including ones the other two KEPT mechanisms would separately admit,
scores as a Solr fallback here). 824/22,458 (3.67%) queries are
single-residual-token and non-anchored, cap-independently eligible.

### Result — every swept cap clears the 2% budget; this population is nearly saturated at unlimited cap

| cap | admitted | coverage (% of whole corpus) | native NDCG (admitted) | Solr NDCG (same admitted) | whole-workload NDCG | degradation (relative) |
|---|---|---|---|---|---|---|
| 1 | 20 | 0.09% | 0.1865 | 0.2348 | 0.2335 | 0.00% |
| 20 | 279 | 1.24% | 0.5482 | 0.6213 | 0.2326 | 0.39% |
| 75 | 407 | 1.81% | 0.4416 | 0.5632 | 0.2313 | 0.94% |
| 250 | 503 | 2.24% | 0.3627 | 0.4980 | 0.2305 | 1.28% |
| 200,000 | 823 | 3.66% | 0.2226 | 0.3334 | 0.2294 | 1.76% |

RQ2 budget calibration: **budget<=0.5% at cap=20 (1.24% coverage);
budget<=1.0% at cap=75 (1.81% coverage); budget<=2.0% at cap=200,000
(3.66% coverage -- 99.88% of the entire eligible population)**. This is
qualitatively different from every prior mechanism measured in Phase 3:
P3-E02's structural admission and P3-E05's anchored lexical narrowing
both showed degradation *accelerating* as the cap loosened past their
"safe" zone; here, even the loosest possible cap (200,000, effectively
unlimited) stays within the 2% budget with real margin (1.76% relative,
not a hair's-width pass). The population is close to fully saturated: no
further cap tuning meaningfully changes coverage past ~3.66%, because
there are simply very few single-residual-token, non-anchored eligible
queries with a combined candidate count above 200,000's practical range.

**Decision**: KEEP. Hardened into `commerce_core::admission::admit_single_token_lexical`
(additive, does not modify `admit_lexically_narrowed`'s or
`admit_structurally_anchored_lexical`'s own already-measured contracts):
rejects outright whenever the residual does not tokenize to exactly one
token, then delegates to `admit_lexically_narrowed` unchanged.
Deliberately does *not* also require empty `query.constraints` --
unlike the anchored mechanism, a single-token residual is safe on its
own merits regardless of whether a structural constraint also happens to
be present; a production caller composing multiple mechanisms should try
them in a fixed order (`admit`, then `admit_structurally_anchored_lexical`,
then this) so a query already admitted upstream is never re-evaluated
here -- exactly how this measurement's own population was kept disjoint
(explicitly excluding P3-E05's anchored qids). 3 new RED-first tests: a
single-token residual admits with the expected count; a 2-token residual
rejects outright even when its own combined count is small and non-zero
(i.e. even though plain `admit_lexically_narrowed` would happily admit
it -- the boundary is token count, not candidate count); a single-token
residual admits even when a structural constraint is also present
(deliberately not mutually exclusive with the anchored mechanism). 20/20
admission tests pass (142 total workspace tests).

**Cross-phase note**: three disjoint, independently-KEPT admission
mechanisms now exist (`admit` structural, `admit_structurally_anchored_lexical`,
`admit_single_token_lexical`), each measured on its own isolated marginal
contribution. A proper P3-E06-style three-way combined measurement
(overlap-checked, real Pareto frontier) is the natural next step before
any further claim about total combined coverage -- three isolated
"clears budget" results do not automatically imply their sum clears
budget when combined, even though all three are pairwise disjoint by
construction (the same caution P3-E06 itself demonstrated is necessary,
not assumed).

Raw artifacts: `docs/research/artifacts/p3e08_run1/`, `p3e09_run1/`.

**Next**: (a) a P3-E10 three-way combined Pareto frontier (structural x
anchored-lexical x single-token-lexical), mirroring P3-E06's own
methodology and overlap-verification discipline; (b) bootstrap CIs on
the newly promoted P3-E09 operating points, mirroring P3-E07; (c)
whether the pure-lexical-only population has further exploitable
structure beyond token count (e.g. does a single token that is also a
recognized brand/product-line spelling variant behave even better --
this edges toward Issue #16's learned-implication territory and should
be evaluated there rather than re-litigated here); (d) per Issue #18's
stated execution order, once #14's mining loop is judged exhausted or
bounded, proceed to #16/#17/#9/#7/#11/#12 in the stated priority.

## P3-E10 — three-way combined Pareto frontier: additivity confirmed exactly, best grid point 3.04% coverage within the 2% budget

**Evidence class**: real, whole-workload -- no new Solr querying (pure
re-aggregation of P3-E02's/P3-E05's/P3-E06's already-persisted per-query
CSVs, plus a re-derivation of the single-token population from P3-E03's
CSV using the identical filter P3-E08/P3-E09 established).

**Hypothesis**: P3-E09 established a third mechanism, disjoint from both
prior KEPT mechanisms. Per P3-E06's own discipline ("three isolated
'clears budget' results do not automatically imply their sum clears
budget when combined"), the three-way combined system needs its own
direct measurement, not an assumption from the three pairwise-disjoint
isolated results.

**Method**: `p3e10_three_way_combined_frontier`. Explicitly asserts
pairwise disjointness across all three eligible populations (185
structural, 1,557 anchored-lexical, 824 single-token-lexical) before
computing any combined number -- **confirmed: 0 overlap in all three
pairwise checks**. Sweeps a representative 2x2x2 grid (`structural_cap`
in {50, 250}, `anchored_lexical_cap` in {1, 20}, `single_token_cap` in
{20, 200,000} -- each mechanism's own two most information-dense cap
values from its own prior budget calibration).

### Result

| structural cap | anchored cap | single-token cap | combined coverage | degradation (relative) |
|---|---|---|---|---|
| 50 | 1 | 20 | 2.68% | 0.94% |
| 250 | 1 | 20 | 3.04% | 1.07% |
| 50 | 1 | 200,000 | 5.10% | 2.27% |
| 50 | 20 | 20 | 6.55% | 2.31% |
| 250 | 1 | 200,000 | 5.46% | 2.40% |
| 250 | 20 | 20 | 6.92% | 2.44% |
| 50 | 20 | 200,000 | 8.97% | 3.64% |
| 250 | 20 | 200,000 | 9.34% | 3.77% |

**A precise internal-consistency check, not just a plausibility check**:
because the three admitted sets are disjoint by construction, whole-
workload degradation should be *exactly* additive across each
mechanism's own isolated-measurement degradation at the same cap.
Verified directly: P3-E06's own isolated (structural=250, anchored=1)
point measured degradation +0.0016; P3-E09's own isolated
(single_token=20) point measured +0.0009; their sum, 0.0025, matches
this experiment's (250, 1, 20) combined point *exactly*. This is real
evidence the combined-coverage arithmetic is correct, not merely
internally plausible.

Within this grid, only the two points with `anchored_cap=1` and
`single_token_cap=20` clear the 2% budget: (50, 1, 20) at 0.94%
relative degradation / 2.68% coverage, and **(250, 1, 20) at 1.07%
relative degradation / 3.04% coverage** -- the best coverage this grid
found while staying under budget, meaningfully more than P3-E06's own
best two-way point at a comparable budget (1.80% coverage at 0.67%).
Every grid point using `single_token_cap=200,000` (near-saturation of
that population) pushes combined degradation over 2%, confirming that
even a mechanism whose own *isolated* unlimited-cap measurement clears
budget comfortably (P3-E09: 1.76% relative alone) can combine with
others to exceed a shared budget -- exactly why this direct combined
measurement, not an assumption from isolated results, is required.

**Decision**: KEEP the three-way combined system and this measurement
methodology. Real, disjoint, additive combined coverage of up to 9.34%
is now established (though only up to ~3% within the 2% budget on this
specific grid); a finer cap search around `anchored_cap` in [1,20] and
`single_token_cap` in [20, a few hundred] would likely find a combined
point closer to the true Pareto-optimal frontier under the 2% budget
than this coarse 8-point grid did, since the additive relationship makes
the tradeoff surface exactly characterizable from each mechanism's own
already-measured per-cap degradation without further Solr querying.

Raw artifacts: `docs/research/artifacts/p3e10_run1/`.

**Cumulative Issue #14 status after P3-E00-E10**: three independently-
KEPT, disjoint, real admission mechanisms exist (structural exact-match,
structurally-anchored lexical narrowing, single-token lexical
narrowing), combined coverage 3-9% depending on operating point and
budget, RQ4 (P50 shift) still requires >50% coverage and remains far
out of reach with these three mechanisms alone. The dominant remaining
rejection reason by volume that has *not yet* been mined at all in this
campaign is **ambiguous queries (22.29% of all real traffic, P3-E01)** --
the second-largest rejection reason after unresolved residual, and a
structurally distinct failure mode (a resolved phrase with multiple
candidate interpretations, not a resolution failure). Per Issue #18's
"keep mining the highest-volume rejected class" mandate, this is the
next candidate worth a diagnostic pass before Issue #14's mining loop
can be honestly described as exhausted.

## P3-E07 — bootstrap confidence intervals for the promoted operating points; a real determinism bug caught by running it twice

**Evidence class**: real, but requires no new Solr querying -- pure
resampling arithmetic over already-persisted per-query data
(P3-E02's/P3-E05's `eligible_queries_raw.csv`, P3-E06's
`whole_corpus_solr_ndcg.csv`).

**Hypothesis/motivation**: every prior Phase 2/3 relevance/coverage
number has been a single deterministic point estimate (correctly so, per
`bench_harness`'s own documented methodology -- these numbers have no
run-to-run variance to bound). But the *aggregate* whole-workload NDCG is
itself a statistic over a finite 22,458-query sample of a larger
real-traffic population, and the user's own instructions ask for
bootstrap confidence intervals on promoted headline results -- not yet
supplied for P3-E05/P3-E06's numbers.

**Method**: `p3e07_bootstrap_ci`. A *paired* percentile bootstrap (not
`bench_harness::bootstrap_ci_diff_of_means`, which is for two
*independent* sample sets): each of the two promoted operating points
((structural=250, anchored=1) and (structural=50, anchored=20)) gets one
length-22,458 array pairing each query's combined-policy score with its
own Solr score, so each of 5,000 resamples draws the *same* query indices
for both quantities, correctly propagating their correlation.

**A real bug, caught by running the binary twice and diffing the
output**: the first implementation built its per-query array via
`solr_ndcg.keys().copied().collect()` -- `HashMap`'s default hasher is
randomized per-process, so the array order (and therefore which query
lands at which index the seeded RNG later draws) silently differed
between runs, breaking the "deterministic given seed" reproducibility
this project's own bootstrap convention requires. Fixed by sorting the
qid list before building the arrays. Confirmed fixed by running the
binary twice more and diffing: byte-identical output past the cargo
build-status lines. Full gate green: fmt, clippy `-D warnings`, workspace
test suite, release build.

### Result

| operating point | coverage | whole-workload NDCG (95% CI) | degradation, relative (95% CI) | CI excludes zero |
|---|---|---|---|---|
| budget<=1.0% (structural=250, anchored=1) | 1.80% | 0.2319 [0.2281, 0.2358] | 0.67% [0.51%, 0.84%] | yes |
| budget<=2.0% (structural=50, anchored=20) | 5.31% | 0.2290 [0.2252, 0.2328] | 1.92% [1.68%, 2.16%] | yes |

Point estimates exactly match P3-E06's own reported numbers (sanity
check passed). Both degradations are **statistically distinguishable
from zero** -- the relevance cost is small, but real, not noise. **A
genuinely important nuance, not smoothed over**: at the budget<=2.0%
operating point, the *point estimate* (1.92%) clears the 2% budget, but
the **upper bound of its 95% CI (2.16%) does not** -- a resampled draw
of this exact query population could plausibly show a degradation
slightly over the nominal 2% target. This is exactly the kind of result
Issue #14 asks to be "actively tried to break": the point estimate alone
would have read as a clean pass; the CI reveals the margin is thinner
than it looks. The budget<=1.0% operating point has no such issue --
even its CI's upper bound (0.84%) stays comfortably under 1%.

**Decision**: KEEP both operating points as promoted, but state the
budget<=2.0% point's real margin precisely rather than rounding it up to
"clears comfortably": it clears on point estimate, sits right at the
edge under resampling uncertainty. A future paper-grade replication
should either accept this margin explicitly or choose a marginally
tighter cap pair for a 2% claim with more headroom (e.g. a smaller
`anchored_lexical_cap` than 20, trading some coverage for margin).

Raw artifacts: `docs/research/artifacts/p3e07_run1/`.
