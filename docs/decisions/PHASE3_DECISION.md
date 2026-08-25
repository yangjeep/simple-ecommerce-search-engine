# Phase 3 Decision

**Decision: NARROW SUPPORT** for the safe-offload thesis Issue #14 set out
to test, on the real catalog/query evidence gathered this phase. A real,
evidence-backed, relevance-safe commerce-native fast path exists and is
additive across three independently-verified admission mechanisms, but
real coverage (up to 5.80% of traffic within a disclosed-margin 2%
relevance budget) remains far short of the >=50% threshold Issue #14's own
RQ4 named as the point where the whole-workload latency distribution
would materially shift. This is not a STOP: unlike Phase 2's whole-engine
replacement thesis, nothing measured this phase contradicts the narrower
safe-offload architecture — it is confirmed, repeatedly, adversarially,
and its own mining loop is now judged exhausted within the admission
family tested. Scaling coverage further requires a materially different
mechanism (Issue #16), not more of what this phase tried.

This document is Phase 3's terminal decision artifact, in the same spirit
`PHASE2_DECISION.md` closed Phase 2 — not overwriting it (Phase 2 remains
historically accurate for its own scope and evidence base, and its STOP
verdict on the *whole-engine replacement* thesis is not revisited here),
but synthesizing what this phase (Issue #14, `docs/experiments/PHASE3_LOG.md`
P3-E00 through P3-E17) found, now that its mining loop has reached a
defensible, adversarially-reviewed boundary.

## Recap: what Phase 3 was asked to answer

Phase 2 rejected whole-engine replacement but found one clean physical
win: true structural `FastPath` is 87-105x faster than Solr, confined to
under 1% of real traffic. Issue #14 reframed the architecture around that
win and asked one question: at a fixed relevance/correctness budget, what
fraction of real ecommerce traffic can a commerce-native forwarding plane
safely intercept, with near-zero cost when it declines and falls back?
Five research questions (RQ1-RQ5) structured the investigation, all of
which are now answered:

- **RQ1** (fallback tax) — **near zero** (P3-E01): fallback overhead is
  statistically indistinguishable from the pure-Solr baseline.
- **RQ2** (safe coverage frontier at 0%/0.5%/1%/2% budgets) — **answered
  with a full, exhaustively-searched frontier** (P3-E02/E05/E09/E10/E16),
  not just a first pass: three disjoint mechanisms combine to a verified,
  exactly-additive 5.80% coverage at the <=2% budget (P3-E16), with
  bootstrap CIs on every promoted point (P3-E07/E17).
- **RQ3** (coverage expansion by query class) — **both major rejection
  reasons mined to a terminal verdict**: unresolved residual lexical text
  (76.89% of traffic) split into KEPT (structural anchoring, single-token
  narrowing) and REJECTED (naive unrestricted narrowing) sub-mechanisms;
  ambiguous queries (22.29% of traffic) mined to BOUNDED/REJECTED
  (frequency resolution alone, and combined with lexical narrowing,
  P3-E11-E15) — a fundamental limitation (no ranking signal), not an
  under-explored one, confirmed under a corrected, fair Solr baseline.
- **RQ4** (does p50 move onto the native path) — **NO, not at this
  coverage level**: >=50% coverage would be required, and this phase's
  full, exhaustively-searched combined frontier tops out at 9.34% even
  outside any relevance budget (P3-E10), 5.80% within the tightest
  practically-relevant one (P3-E16). This is the single fact that caps
  this phase's verdict at NARROW rather than STRONG.
- **RQ5** (serving economics) — **favorable, but proportionally small**:
  every admitted query removes a Solr round-trip at ~0.001-0.0015ms
  native cost (P3-E02/E05), a real per-hit saving, but at <=5.80% coverage
  the aggregate Solr-request-avoided rate is too small to reshape
  whole-workload serving cost materially — consistent with RQ4's finding.

## Architecture tested

`commerce_core::admission` (new this phase): three independently-KEPT,
mutually-disjoint admission mechanisms, each hardened with its own
RED-first tests and each measured on its own isolated marginal
contribution before any combined claim was made:

1. **`admit()`** (original, P3-E02) — fully structurally resolved query,
   empty lexical residual, selective enough. 0.81% coverage alone.
2. **`admit_structurally_anchored_lexical`** (P3-E05) — >=1 structural
   constraint alongside a lexically-narrowed residual. The largest single
   coverage contributor (up to 6.93% alone at unlimited cap).
3. **`admit_single_token_lexical`** (P3-E09) — exactly one residual
   token, structural constraint optional. Near-saturated at ~3.66% of
   whole-corpus traffic even at unlimited cap.

A fourth candidate mechanism (`admit_lexically_narrowed`, unrestricted
token-presence verification, P3-E03) was built, measured, and REJECTED —
preserved in `commerce_core::admission` only as the internal delegate the
two restricted, KEPT mechanisms above call, never exposed as a
general-purpose policy itself. A fifth candidate mechanism (frequency-
resolved ambiguity, alone and combined with lexical narrowing, P3-E11-E15)
was built, measured, found favorable, adversarially investigated after
its own favorable result looked anomalous, traced to a real bug in shared
Solr-baseline infrastructure, corrected, and REJECTED once the baseline
was fair.

All three KEPT mechanisms are additive by verified construction, not
assumption: pairwise disjointness was checked directly on real per-query
data at every combination stage (P3-E06 for two mechanisms, P3-E10 for
three), and the combined system's whole-workload degradation was proven
*exactly* additive by algebraic identity in P3-E16 (each mechanism's own
isolated degradation equals `(sum_solr_on_admitted - sum_native_on_admitted)
/ total`, independent of what any other mechanism admits when populations
are disjoint) — cross-checked against every point P3-E10 had separately
measured, exact match in all eight cases.

## Datasets / workloads

The same real evidence base Phase 2 established, reused throughout:
the real 1,215,854-product Amazon ESCI catalog and the real 22,458-query,
human-judged corpus, plus a fresh, same-environment, locally-running
Apache Solr 9.10.1 instance. `crates/bench-harness`'s statistical-rigor
tooling (repeated measurement, bootstrap CIs) is reused unchanged.

**The same named, real dataset limitation Phase 2 already flagged remains
true and load-bearing this phase too**: the ESCI export has no structured
`product_type`/`category`/price data (`round1_eval::catalog`'s
`UNKNOWN_PRODUCT_TYPE`/`UNKNOWN_CATEGORY` sentinels). Brand is the only
real, per-product-diverse structural entity. Every admission mechanism
this phase built and measured is therefore implicitly scoped to
brand-anchored and lexical-residual signal only — a genuinely different
real catalog with structured multi-entity data remains untested and could
shift this phase's specific coverage numbers, though not the underlying
methodology.

## Measured results

Full per-experiment tables and raw artifacts: `docs/experiments/PHASE3_LOG.md`
P3-E00-E17, `docs/research/artifacts/p3e{00..17}_run1/`.

| Question | Evidence | Result |
|---|---|---|
| Fallback tax (RQ1) | P3-E01 | Statistically indistinguishable from zero |
| Structural-only frontier | P3-E02 | Hard-capped at 0.81% by semantic resolution, not selectivity |
| Naive lexical narrowing | P3-E03 | REJECT — fails every budget; real `count==0` bug found and fixed |
| Structurally-anchored lexical | P3-E05 | KEEP — clears every budget, up to 6.93% coverage alone |
| Combined 2-way frontier | P3-E06 | Verified disjoint/additive; RQ4: no p50 shift at 1.80-7.53% coverage |
| 2-way bootstrap CIs | P3-E07 | Real, nonzero degradation; <=2% point's CI upper (2.16%) exceeds nominal budget |
| Single-token lexical | P3-E08/E09 | KEEP — clears budget even at unlimited cap, near-saturated ~3.66% alone |
| Combined 3-way frontier (coarse) | P3-E10 | Verified disjoint/additive; best sub-2%-budget point 3.04% coverage |
| Ambiguous-query mining | P3-E11/E12 | Diagnostic + BOUNDED — frequency resolution alone recovers negligible coverage |
| Ambiguity + lexical (original) | P3-E13 | Originally reported KEEP — anomalously favorable, investigated |
| Solr-baseline fairness audit | P3-E14 | Real bug found in shared baseline helper; P3-E05/06/09/10 confirmed unaffected |
| Ambiguity + lexical (corrected) | P3-E15 | REJECT confirmed — original result was entirely a baseline artifact |
| Combined 3-way frontier (fine) | P3-E16 | Exact algebraic re-derivation; best <=2%-budget point nearly doubles to 5.80% |
| 3-way bootstrap CIs | P3-E17 | All three promoted points' CI upper bounds exceed their own nominal budget |

## Failed experiments (preserved, not erased)

Two real bugs were found and fixed this phase, plus one mechanism that
looked KEPT and was corrected to REJECT once its own supporting
measurement was fixed — all recorded in full in `docs/experiments/PHASE3_LOG.md`,
not smoothed over:

1. **`admit_lexically_narrowed`'s `count==0` bug** (P3-E03): silently
   admitted queries whose combined structural+lexical candidate set was
   *empty*, guaranteeing a zero-relevance false positive. Found via the
   "any benchmark anomaly is a bug investigation" discipline when an
   early sweep showed impossibly bad degradation, fixed RED-first.
2. **Bootstrap non-determinism from `HashMap` iteration order** (P3-E07):
   a seeded RNG fed a randomized-per-process array order, silently
   breaking run-to-run reproducibility. Found by running the binary
   twice and diffing byte-for-byte, fixed by sorting query IDs before
   array construction — a lesson reused explicitly in every subsequent
   bootstrap binary this phase (P3-E17).
3. **`round1_eval::solr::solr_query_for`'s ambiguous-span/residual-
   constraint drop bug** (P3-E14, this phase's most significant finding):
   Solr's own `q=` construction silently omitted any word that resolved
   to a non-brand/color structural constraint, and every word inside an
   ambiguous span, whenever `residual_lexical` was non-empty. This made
   P3-E13's own reported "native beats Solr" result — the first of its
   kind in fifteen prior entries — a measurement artifact, not a real
   finding. A 4-agent adversarial-audit workflow quantified the bug's
   prevalence across four populations, confirmed P3-E05/E06/E09/E10's
   own KEEP verdicts were unaffected (gap tiny, 2.26%, and favoring the
   KEEP direction), and found P3-E11-E13's own population was severely
   affected (98.23% still non-empty residual after ambiguity resolution).
   P3-E15's surgical fix and fresh re-measurement reversed P3-E13
   completely (`ndcg_delta` +0.0102 -> -0.1488). This is the clearest
   demonstration in this project's history of why "actively try to kill
   every favorable result" is not a formality: a single surprising,
   never-before-seen result was investigated rather than celebrated.

**A structural, not per-mechanism, statistical finding, disclosed rather
than smoothed over** (P3-E07, generalized in P3-E17): every operating
point this phase's own grid searches promoted as "the tightest point
that clears budget X" has a 95% bootstrap CI whose upper bound *exceeds*
budget X — at all three budgets (<=0.5%/<=1.0%/<=2.0%), not just one.
This is a property of optimizing a point estimate to sit exactly at a
threshold, not evidence any individual mechanism is unsound (each cleared
its own budget with real margin at moderate, non-grid-search-optimal
caps). A deployment wanting a hard statistical guarantee should back off
from the searched optimum; this phase discloses the fact rather than
resolving how far to back off, since that is a deployment-policy choice.

## Unresolved risks

1. **The searched-optimal operating points carry thinner statistical
   margin than their point estimates alone suggest** (P3-E07/E17, above)
   — not resolved to a specific safer recommendation, only disclosed.
2. **False-positive admissions exist in every lexical-narrowing mechanism,
   never eliminated, only bounded**: 5.14-15.35% at various caps for the
   anchored mechanism (P3-E05), higher for the unrestricted, REJECTED one
   (P3-E03). The underlying cause — `execute_ranked` has no ranking
   signal when `query.preferences` is empty, inherited unchanged from
   Phase 2's own P2-E17 finding — remains unfixed; every KEEP verdict
   this phase reached is a statement about whole-workload aggregate
   safety at a given cap, not a claim that any individual admitted query
   is guaranteed correct.
3. **The single-token mechanism is near-saturated** (P3-E09: ~3.66% of
   whole-corpus traffic even at unlimited cap) — its own ceiling is a
   property of how few real queries have exactly one residual token, not
   a cap-tuning question; no further coverage is available from this
   specific mechanism regardless of relevance budget.
4. **The ambiguous-query rejection reason (22.29% of traffic) remains
   fundamentally unaddressed** by any mechanism this phase tried
   (frequency resolution alone or combined with lexical narrowing) — not
   because the toolkit was under-explored, but because ambiguity
   resolution alone supplies no ranking/precision signal, confirmed twice
   (P3-E12's own real measurement, then P3-E15's corrected
   re-confirmation after the baseline bug was fixed). Closing this gap
   requires a materially different signal source (Issue #16).
5. **P3-E14's audit did not directly re-examine P3-E02/E03's own Solr
   comparisons** — reasoned, not measured, to be safe a fortiori (a
   Solr-weakening bug can only make P3-E03's REJECT verdict more
   supported, never less; P3-E02's admitted population is structurally
   immune by construction, empty residual by definition, matching
   P3-E09's own confirmed reasoning) — but this reasoning was not
   independently verified by a separate audit pass the way P3-E05's was.
6. **The same real-catalog limitation Phase 2 named remains unresolved**:
   no genuine structured multi-entity data exists in this catalog to test
   whether a `ProductType`/`Category`-anchored version of any mechanism
   here would behave differently — untested, not assumed either way.
7. Every unresolved risk `PHASE2_DECISION.md` already named and this
   phase didn't touch remains open (no incremental index update path; no
   concurrent-load benchmarking; single shared/virtualized environment,
   no cross-hardware validation; `FastPath`'s zero-ranking-signal
   relevance cost, still unfixed).

## What would be built next if scaling up (conditional -- see decision)

This phase's own evidence justifies continuing the safe-offload
architecture as a narrow, additive optimization layer, not scaling it as
a whole-engine strategy. The concrete next steps, in the priority order
Issue #18 itself set:

1. **Issue #16 — learned semantic implication rules**: the only
   remaining lever this phase's own diagnostics identified for the
   ambiguous-query and residual-lexical-without-ranking-signal
   populations, since both are bounded by a missing ranking/precision
   signal that further token/frequency-based narrowing cannot supply.
2. **A default ranking signal for the admitted subset** (still unfixed
   from Phase 2): would directly reduce the 5-15% false-positive rate
   this phase's KEPT mechanisms still carry, independent of Issue #16.
3. **A deployment-margin decision, informed by P3-E17's CIs**, on how
   far to back off from the grid-search-optimal operating points before
   any of this phase's mechanisms are used in a production-facing
   context with a hard relevance guarantee.
4. **A random/stratified re-run of the ambiguous-query mining loop on a
   materially different real catalog** (one with genuine structured
   multi-entity data), to test whether the "no ranking signal" boundary
   found here is catalog-specific or general.

## What should explicitly not be built yet

- **Further token/frequency-based narrowing within the same admission
  family** (naive lexical narrowing generalizations, additional
  ambiguity-resolution heuristics) — this phase's own mining loop
  exhausted this toolkit for both major rejection reasons; continuing
  within it is not expected to yield new safe coverage per P3-E16's own
  boundary analysis.
- **Adopting the raw grid-search-optimal operating points in a
  production context without first deciding a deployment margin** —
  P3-E17 found every one of them has a CI upper bound exceeding its own
  nominal budget.
- **Distributed/sharded serving, cluster coordination, multi-tenancy,
  HA** — unaffected by this phase, `PHASE2_DECISION.md`'s conclusion
  stands.
- **A production LLM-backed `ModelProvider` or any model call in the
  default query hot path** — Issue #16 explicitly requires the offline/
  online separation this phase's own `control_plane` precedent already
  established; nothing here changes CLAUDE.md's hard rule.

## What this decision does and does not claim

**Claims**: on the real 1,215,854-product Amazon ESCI catalog and real
22,458-query human-judged corpus, measured against a fresh, same-
environment, real Apache Solr 9.10.1 instance, with a real, previously-
invisible Solr-baseline-construction bug found via self-directed
adversarial scrutiny, quantified via a 4-agent audit, and corrected: three
independently-verified, disjoint, hardened admission mechanisms exist that
safely intercept up to 5.80% of real traffic within a 2% relevance
degradation budget, with near-zero fallback tax and no p95/p99
regression, an exactly-additive combined frontier proven by algebraic
identity and cross-checked against every prior measured point, and
honest, disclosed statistical margins (including the finding that every
grid-search-optimal operating point's CI crosses its own nominal budget).
Both rejection-reason classes making up 99.18% of real rejected traffic
have been mined to a terminal KEEP/REJECT/BOUNDED verdict.

**Does not claim**: that 5.80% coverage is enough to move the
whole-workload p50 latency distribution (RQ4 explicitly requires >=50%,
not reached, and not expected to be reached without a materially
different mechanism); that the searched-optimal operating points are
safe to deploy without a human margin decision (P3-E17); that every
individual admitted query is guaranteed relevant (false-positive rates
of 5-15% persist, bounded not eliminated); that the ambiguous-query
population is permanently unsolvable (only that this phase's specific
token/frequency-based toolkit cannot solve it); that a different real
catalog with genuine multi-entity structure would show the same coverage
numbers (untested); or that Issue #14 as a whole is closed — only that
its *mining loop*, as scoped to the admission family this phase built, has
reached a defensible terminal boundary, and that further coverage growth
is Issue #16's question to answer, not this phase's to keep re-asking.

### Addendum (2026-08-25) — the "no ranking signal" ambiguous-query boundary partially cross-validated on WANDS

Item 4 of this phase's own "what would be built next" list named exactly
this open question: does the ambiguous-query "no ranking signal" finding
(P3-E12/P3-E15) generalize beyond ESCI to a materially different, more
structurally rich real catalog? An Issue #55 checkpoint
(`docs/decisions/ISSUE55_AMBIGUOUS_ROUTING_DECISION.md`) found, on the
real WANDS catalog (genuine `Category`/`ProductType` structure, not
ESCI's single-entity Brand data), a small (n=4 of 480 real queries)
but real, mechanistically-confirmed instance of the same boundary:
queries whose entire content resolves to ambiguous attribute readings
get no useful native ranking signal, and score materially worse than a
real lexical delegate (0.2819 vs. 0.8173 NDCG@10) specifically when no
other constraint narrows the candidate set. This is a small, single-
dataset, non-exhaustive data point, not a repeat of this phase's own
5,000-query study — but it is consistent with, not contrary to, this
phase's own conclusion, on the second, materially different catalog
this phase's own "what's next" list asked for. It does not by itself
close item 4 or change this phase's terminal-boundary verdict; it is
recorded here per this project's evidence-preservation discipline
rather than left as an isolated, uncross-referenced Issue #55 artifact.
The Issue #55 checkpoint also found that `commerce_core::admission::admit`
(this phase's own KEPT mechanism) already rejects any query matching
this pattern to the lexical delegate — the defect it measured exists
only in the separate, later `plan`/`execute_planned` routing path
(Phase 2/9's lineage), which never composed with this phase's own
admission contract.
