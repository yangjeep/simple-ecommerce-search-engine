# Issue #55 Experiment Log — does the planner discard preserved query ambiguity?

Protocol: `docs/experiments/ISSUE55_AMBIGUOUS_ROUTING_PROTOCOL.md`.

## I55-AMBIGROUTE-E00 — initial finding, then corrected by adversarial review: not a novel defect, already studied and already partially solved elsewhere

**Initial measurement**

New diagnostic binary `crates/phase9-eval/src/bin/p9_e07_ambiguous_routing_diagnostic.rs`
(read-only measurement; no `commerce-core` change). Classifies every one
of the 480 real WANDS queries by whether `compile()`'s output has
`ambiguous` non-empty AND `residual_lexical` empty AND `preferences`
empty, then reports native (via `execute_planned`, the same routing
path every recent Issue #55 checkpoint has exercised) vs. Solr NDCG@10.

```
matched queries: n=4
  "driftwood mirror" -- ambiguous: ["driftwood","mirror"] candidates=42994 native_ndcg=0.0000 solr_ndcg=0.9633
  "leather dining chairs" -- ambiguous: ["leather"]        candidates=568   native_ndcg=0.4080 solr_ndcg=0.8365
  "marble" -- ambiguous: ["marble"]                        candidates=42994 native_ndcg=0.0000 solr_ndcg=0.7156
  "wood bar stools" -- ambiguous: ["wood"]                 candidates=775   native_ndcg=0.7196 solr_ndcg=0.7540

matched population: native NDCG@10=0.2819  solr NDCG@10=0.8173  (-65.51% relative)
rest of corpus (same run): native NDCG@10=0.6628  solr NDCG@10=0.6209
```

By the letter of the preregistered gate (n>=5 AND >=10% relative gap):
**not met** (n=4). The initial write-up (superseded by this revision)
argued the effect size and a "mechanistically confirmed" code-level
trace (`compile()` preserves ambiguity in `query.ambiguous`;
`plan()`/`crates/commerce-core/src/plan/mod.rs:172-177` never reads it;
`execute_ranked` scores every candidate `0.0` for the resulting
empty-residual FastPath query) justified treating this as a real,
worth-fixing architecture gap despite the failed population-size gate.

**This was wrong, and caught by adversarial review before being
recorded as a decision** — three independent reviewers (code-correctness,
methodology, architectural-significance lenses) were run against the
claim. All three confirmed the code-level mechanism is accurate (verified
independently against the live source, not the claim's paraphrase), but
all three also found the "novel architecture gap" framing overstated, for
convergent reasons:

1. **This project already ran a much larger, dedicated study of exactly
   this question and reached a firm verdict.** `docs/decisions/PHASE3_DECISION.md`
   (Issue #14, P3-E11 through P3-E15, ~5,000 real ESCI queries, 22.29% of
   that traffic): *"The ambiguous-query rejection reason... remains
   fundamentally unaddressed by any mechanism this phase tried... not
   because the toolkit was under-explored, but because ambiguity
   resolution alone supplies no ranking/precision signal, confirmed
   twice... Closing this gap requires a materially different signal
   source (Issue #16)."* Neither this checkpoint's protocol nor its
   initial log cited that decision record — a direct miss of this
   project's own research-discipline step 1 ("read the issue, relevant
   decision record... before starting broad work").
2. **A sibling module already implements the fix this checkpoint was
   about to propose.** `crates/commerce-core/src/admission.rs:106-108`
   (verified directly):
   ```rust
   if !query.ambiguous.is_empty() {
       return AdmissionDecision::Reject(RejectReason::Ambiguous);
   }
   ```
   `admission::admit` — a Phase 3 (Issue #14) mechanism, RED-tested
   (`rejects_an_ambiguous_query_regardless_of_selectivity`) — already
   treats non-empty `ambiguous` as disqualifying and falls back to the
   lexical delegate. `plan()`/`execute_planned` (the Phase 2/9 lineage
   this and every recent Issue #34/#43/#55 checkpoint exercises) is a
   second, parallel, never-composed routing implementation in the same
   crate that simply never inherited that safety property. The accurate
   characterization is "two uncomposed routing modules coexist, one
   safer than the other for this case" — not "the planner has no answer."
3. **Whole-workload impact is negligible.** Weighting the claimed
   fix's benefit across all 480 queries: `4 x (0.8173 - 0.2819) / 480
   ~= 0.0045` — under half a percentage point, on a workload where
   native (0.6628) already leads Solr (0.6209) on the other 476 queries.

**Follow-up measurement, prompted by the methodology lens's specific
critique** (the n=4 population conflates different severities and was
never compared against a proper control): extended the diagnostic with
per-query NDCG and a control-group classification (queries with empty
`residual_lexical`/`preferences` and empty `ambiguous`, i.e. genuinely
nothing extracted at all — the "dinosaur"/"chair and a half recliner"
shape). Result:

```
control queries (ambiguous empty, constraints empty, zero-signal-for-FastPath): n=0
```

**No such query exists in this 480-query set.** Every real WANDS query
that reaches `residual_lexical.is_empty()` either has `ambiguous`
non-empty (these 4) or has real, non-ambiguous resolved `constraints`
(the normal FastPath population) — there is no query in this dataset
with literally nothing extracted at all. This means the intended
control group cannot be built from this corpus, but the per-query
breakdown above does the same diagnostic job directly: the 4 matched
queries are **not homogeneous**. "driftwood mirror" and "marble" have
`candidates=42994` (the entire catalog — no constraint narrowed
anything) and score exactly `0.0000` — byte-identical to the
already-documented, already-named "FastPath + huge unnarrowed candidate
set = arbitrary order" pathology from
`docs/decisions/ISSUE55_EMPTY_RESIDUAL_FIX_DECISION.md`'s own open
follow-up ("should a 100%-of-catalog candidate set route to `FastPath`
at all"). "leather dining chairs" (`candidates=568`) and "wood bar
stools" (`candidates=775`) have a real, narrower candidate set from a
different, non-ambiguous constraint elsewhere in the query, and score
`0.4080`/`0.7196` — the latter *above* the corpus's own native average
(0.6628). The `-65.51%` aggregate is driven entirely by the two
full-catalog cases; ambiguity absorbing a query's tokens is not itself
catastrophic, huge unnarrowed candidate sets are, exactly as checkpoint
7 already found and named as an open question.

**Corrected conclusion**: this is not a distinct "planner ignores
ambiguity" defect. It is one more concrete instance of the single,
already-named, already-tracked question from checkpoint 7: whether
`FastPath` should have a selectivity/candidate-count gate independent of
*why* a query ended up with empty `residual_lexical` (ambiguity
absorption is one path there; the project's Phase 2/9 lineage never
required narrowing to route to `FastPath` at all). The genuinely new,
disclosed contribution of this checkpoint is narrow but real:
`PHASE3_DECISION.md` named, as unexplored future work, *"a
random/stratified re-run of the ambiguous-query mining loop on a
materially different real catalog... to test whether the 'no ranking
signal' boundary found here is catalog-specific or general"* — this
checkpoint is a small, real (if tiny, n=4) data point confirming that
boundary generalizes to WANDS, a materially different, more structurally
rich catalog than ESCI.

## Adversarial review

Performed via a 3-lens independent-agent workflow (code-correctness,
methodology, architectural-significance) before any decision was
recorded — see `docs/decisions/ISSUE55_AMBIGUOUS_ROUTING_DECISION.md`
for the verdict this drove. Every citation above (`PHASE3_DECISION.md`'s
exact wording, `admission.rs`'s exact code, `plan/mod.rs`'s exact
behavior) was independently re-verified by hand after the workflow
returned, not taken on the sub-agents' word alone.

## Decision

See `docs/decisions/ISSUE55_AMBIGUOUS_ROUTING_DECISION.md`.
