# Issue #55 — does the planner discard preserved query ambiguity?

Log: `docs/experiments/ISSUE55_AMBIGUOUS_ROUTING_LOG.md`. Protocol:
`docs/experiments/ISSUE55_AMBIGUOUS_ROUTING_PROTOCOL.md`.

## Verdict: REJECT (as a new architecture-gap finding) — SUPERSEDED by prior work; folded into an existing follow-up

The code-level mechanism this checkpoint found is real and was
independently verified twice (once by this checkpoint, once by an
adversarial-review workflow reading the live source directly):
`ir::query::compile` preserves genuine multi-candidate ambiguity in
`query.ambiguous` and never adds it to `residual_lexical`;
`plan()`/`execute_planned` (the routing path every Issue #34/#43/#55
checkpoint this session has exercised) never reads `query.ambiguous`.
For 4 of 480 real WANDS queries this produces a large local NDCG gap
(native 0.2819 vs. Solr 0.8173).

**This is not, however, a new finding requiring a new fix**, for three
independent, verified reasons:

1. **Already studied, at far larger scale, with a firm conclusion.**
   `docs/decisions/PHASE3_DECISION.md` ran a dedicated ~5,000-real-query
   (22.29% of ESCI traffic) investigation of ambiguous-query handling
   and concluded ambiguity resolution alone supplies no ranking signal,
   naming a materially different signal source (Issue #16) as the only
   remaining lever — not a planner-routing tweak.
2. **Already partially solved, in a sibling module never composed with
   the path this session exercised.** `commerce_core::admission::admit`
   already rejects any query with non-empty `ambiguous`, forwarding it
   to the lexical delegate — exactly the behavior this checkpoint's
   initial write-up was about to propose as a "new fix." `plan()` is a
   parallel, uncomposed implementation that never inherited it.
3. **Whole-workload impact is negligible** (~0.45 percentage points on
   the full 480-query average) — disproportionate to a dedicated fix
   experiment given this project's own "translate microbenchmarks into
   system-level impact" discipline.

**What the per-query/control-group follow-up (prompted by adversarial
review) established**: the 4-query population is not homogeneous. The
2 queries with a full-catalog candidate set (`candidates=42994`,
"driftwood mirror"/"marble") score exactly `0.0` — identical to the
already-documented, already-named "FastPath + unnarrowed huge candidate
set" pathology from `docs/decisions/ISSUE55_EMPTY_RESIDUAL_FIX_DECISION.md`.
The 2 queries with a real narrowing constraint elsewhere
(`candidates=568`/`775`) score `0.4080`/`0.7196` — the latter *above*
the corpus's own native average. No query in this 480-query set has
literally nothing extracted at all (`ambiguous`, `constraints`, and
`residual_lexical` all empty) to serve as a clean control, but the
per-query split does the same job: **candidate-set size, not the reason
`residual_lexical` ended up empty, is what determines whether this
pathology bites.** Ambiguity-absorption is one path to an empty
`residual_lexical`; it is not itself the defect.

## Why REJECT/SUPERSEDED, not REFINE or KEEP

REFINE or KEEP would imply this checkpoint discovered something
actionable beyond what the project already knows and has already
partially built. It did not. Recording a REJECT here, with full credit
to the prior work that already answered this question, is the honest
outcome — matching this project's own "reuse it, complete it, or
explicitly explain why a different experiment is higher-information"
rule for anything already covered by existing research (Issue #55's own
governing text, referencing exactly `PHASE3_DECISION.md`'s lineage).

## What this does change

- **`docs/decisions/PHASE3_DECISION.md`** gains a dated addendum (not a
  rewrite): its own named future-work item 4 ("a random/stratified
  re-run of the ambiguous-query mining loop on a materially different
  real catalog... to test whether the 'no ranking signal' boundary
  found here is catalog-specific or general") is partially answered —
  a small (n=4), real data point on WANDS (genuine multi-entity
  structural data, unlike ESCI) is consistent with the same boundary:
  ambiguity alone still supplies no ranking signal there either.
- **`docs/decisions/ISSUE55_EMPTY_RESIDUAL_FIX_DECISION.md`'s** open
  follow-up ("should a 100%-of-catalog candidate set route to `FastPath`
  at all") is not resolved here, but is now understood more precisely:
  the trigger is candidate-set size at `FastPath` admission, and
  ambiguity-absorption is one, but not the only, way a real query
  reaches that state. This checkpoint does not re-open or duplicate that
  follow-up; it adds one more piece of evidence to it.
- **Names, but does not schedule**, a distinct, larger architectural
  question this investigation surfaced as a genuine side-finding: two
  parallel, uncomposed routing/admission implementations
  (`plan`/`execute_planned` from Phase 2/9, `admission::admit` from
  Phase 3) coexist in `commerce_core`, and every recent Issue #34/#43/#55
  checkpoint has been exercising only the one without `admission`'s
  ambiguity-rejection safety property. Whether this is intentional
  (a deliberate supersession, `admission` treated as an abandoned
  prototype) or accidental drift is not established here and is a fair
  candidate for a future, separately-scoped question — not pursued in
  this checkpoint given the demonstrated negligible whole-workload
  impact of the specific case that surfaced it.
- **No `commerce-core` production code changed.** This checkpoint is
  measurement and literature/decision-record reconciliation only.

## Adversarial review

Performed as a dedicated 3-lens independent-agent workflow
(code-correctness, methodology, architectural-significance) run against
this checkpoint's own initial (now-superseded) conclusion *before* this
decision was recorded — the review is what produced the correction
recorded above, not a post-hoc confirmation of an already-decided
verdict. Every citation the reviewers relied on was independently
re-verified afterward by direct inspection of `plan/mod.rs`,
`admission.rs`, `ir/query.rs`, and `PHASE3_DECISION.md`'s literal text.
This is preserved as a positive example of the falsification loop's own
required discipline working as intended: a plausible, code-verified,
seemingly-novel finding was caught and correctly downgraded before being
recorded as a KEEP/REFINE decision, rather than after.

## Traceability

Source: `crates/phase9-eval/src/bin/p9_e07_ambiguous_routing_diagnostic.rs`
(diagnostic only, no production code). Raw evidence:
`docs/research/artifacts/i55_ambiguous_routing/run1.txt`.
