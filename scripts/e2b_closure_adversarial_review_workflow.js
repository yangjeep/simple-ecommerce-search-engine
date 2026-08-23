export const meta = {
  name: 'e2b-closure-adversarial-review',
  description: 'Issue #42 E2b serving-contract closure: fresh adversarial review, no implementation mandate',
  phases: [
    { title: 'Independent review', detail: 'two independent reviewers inspect the corrected E2b gate accounting, serving-overhead measurement, stability re-run, and WANDS audit' },
  ],
}

const REVIEW_BRIEF = `You are a fresh, independent adversarial reviewer with NO implementation task. You have never seen this conversation and have no memory of writing any of the code or docs you are about to review. Your job is to try to FALSIFY the findings below, not confirm them. Per this repository's own governance (Issue #42, CLAUDE.md): "Claude/LLM output, fixture generators, benchmark code, documentation, and conclusions are all untrusted until independently checked." Apply that standard ruthlessly to everything described below, including this brief itself.

## Context

Repository: /home/user/simple-ecommerce-search-engine (already checked out at the current HEAD -- use Bash/Read/Grep freely to inspect the actual committed state, do not trust any summary, including this one, without checking the real files).

A prior pass on PR #44 was asked to treat Issue #42's E2b (offline LLM-assisted feature discovery) REVISE conclusion as untrusted and try to falsify it. That pass:

1. Found and fixed a gate-accounting error: Issue #42's own E2b GO gate has SIX preregistered criteria (see the "GO gate and preregistered thresholds" text near the end of docs/experiments/ISSUE42_PROTOCOL.md, and Issue #42's own body text -- fetch the issue or read the protocol directly, do not trust this summary), but ISSUE42_DECISION.md and docs/experiments/ISSUE42_LOG.md's own "GO gate evaluation, final" table both previously enumerated only five, omitting serving overhead, then called repeated-run agreement "the sole remaining gap." That claim was corrected.
2. Implemented and ran a new serving-overhead measurement (crates/issue42-eval/src/bin/e2b_serving_overhead_eval.rs) -- read the actual source, not a summary. Verdict: PASS at P95/P99 of a heavier execute_ranked operation (both above a pre-declared 1-microsecond timer floor), INCONCLUSIVE at P50 (below the floor) for both indexed_candidates and execute_ranked.
3. Re-ran the repeated-run stability metric with 12 new independent LLM-proposal passes (3 new runs per configuration, decided before seeing results -- see scripts/e2b_stability_rerun_workflow.js's own comments). Result: 1095/1250 (87.60%) pairwise agreement, up from 107/125 (85.60%), still below the 90% preregistered bar. Per-configuration breakdown is in docs/experiments/ISSUE42_LOG.md's new "I42-E2b serving-contract closure" section, item 5.
4. Audited whether WANDS, as actually used in this pipeline, satisfies Issue #42's own "Product/Variant or relationship complexity" dataset requirement. Finding: NOT ESTABLISHED (downgraded from an unaudited PASS) -- WANDS has no real Variant concept and its two oracle-labeled Relationship fields are never materialized or exercised anywhere in the E2b pipeline.
5. Recovered and committed a previously-uncommitted key-name mapping (crates/issue42-eval/src/e2b_key_mapping.rs) that the original 8 LLM passes relied on, and refactored shared accepted-descriptor logic into crates/issue42-eval/src/e2b_pipeline.rs so the serving-overhead binary reuses the exact same "what did the validator accept" computation the accuracy binary uses.

The corrected write-up lives in docs/experiments/ISSUE42_LOG.md's "I42-E2b serving-contract closure" section (search for that heading) and in ISSUE42_DECISION.md's E2b bullet and "verdicts, stated precisely" section. Read BOTH documents in full before reviewing anything else -- they are the artifact under review.

## What to inspect (Issue #42's own required review checklist for this pass)

At minimum, all of the following -- do not skip any:

1. **Original preregistration vs final gate accounting.** Fetch or read docs/experiments/ISSUE42_PROTOCOL.md's E2b section and cross-check the corrected six-criterion table in ISSUE42_LOG.md against it verbatim. Is the six-criterion enumeration actually correct? Is any criterion's wording paraphrased in a way that changes its meaning?
2. **Serving-overhead methodology and arithmetic.** Read crates/issue42-eval/src/bin/e2b_serving_overhead_eval.rs in full. Does the code actually do what the write-up claims? Recompute the reported percentages from the raw numbers in docs/research/artifacts/i42_e2b_serving_overhead_run1/summary.json yourself -- do not trust the write-up's own arithmetic. Is the timer-floor threshold (1 microsecond) actually applied correctly, and was it genuinely fixed before results were seen (check git history / commit order if useful), or does it look like it could have been tuned after the fact? Is comparing indexed_candidates (near-timer-floor by architecture) alongside execute_ranked (which has real above-floor P95/P99) a legitimate way to answer the gate, or does it look like cherry-picking whichever measurement happened to pass? Is the workload (68 queries, fields common to both bundles) actually fair -- does restricting to common fields bias the result in either bundle's favor?
3. **Repeated-run sampling and agreement calculation.** Read crates/issue42-eval/src/e2b_pipeline.rs's build_baselines_2_and_3 function in full, and scripts/e2b_stability_rerun_workflow.js. Recompute 1095/1250 and the per-configuration numbers yourself from the actual dataset_cache/export/e2b_llm_proposals_*.json files (all 20 of them: 4 configs x 5 runs) -- do not trust the reported numerator/denominator. Is the "every pairwise combination of runs" generalization actually equivalent to the original "run 1 vs run 2" definition when N=2 (check this precisely, e.g. write a tiny script or reason through the code)? Was the run count (3 new runs per configuration) genuinely decided before results were seen, or is there any sign it was chosen/adjusted after peeking? Are the 12 new LLM-proposal artifacts (dataset_cache/export/e2b_llm_proposals_*_run{3,4,5}.json) genuinely independent -- do any two of them look suspiciously identical (which would suggest caching/non-independence rather than real fresh model calls)?
4. **WANDS qualification.** Independently verify the three claims in ISSUE42_LOG.md's WANDS-audit subsection against the actual code (crates/issue42-eval/src/e2b_ingest.rs's accepted_typed_keys and build_catalog, crates/issue42-eval/src/e2b_workload.rs) and the actual raw data (grep dataset_cache/wands/product.csv yourself for the two relationship fields' real values). Is the conclusion (NOT ESTABLISHED) actually supported by what you find, or does it overstate/understate the case?
5. **Raw artifacts vs reported numbers.** Spot-check at least 3 specific numbers quoted in the write-up (your choice) against their actual raw source files (docs/research/artifacts/i42_e2b_serving_overhead_run1/summary.json, docs/research/artifacts/i42_e2b_stability_rerun_run1/summary.json, the dataset_cache/export files) and confirm they match exactly.
6. **Self-grading risk.** Does any conclusion depend on an evaluator grading its own generated labels (e.g. does the serving-overhead or stability code use the SAME logic to both produce a result and judge whether that result passes, in a way that could hide a bug)? Is the oracle (e2b_oracle.rs) genuinely independent of the validator/statistics-only baselines, per this repo's own established governance?
7. **Selective dropping.** Does the write-up disclose everything material, or does anything look selectively omitted -- e.g. a run that failed and was quietly excluded, a query that was dropped from the workload without explanation, a percentile or configuration that would have looked worse if included?
8. **Whether corrected numbers alter earlier claims.** Does the corrected six-criterion table and REVISE verdict actually follow logically from the individual measurements, or is there a gap in the reasoning (e.g. does the write-up claim something is "materially strengthened" when the actual delta is small, or claim something is "not attributable to noise" without real justification)?

## Constraints

- You have NO implementation mandate. Do not edit, fix, or "improve" any file. Report findings only.
- Do not trust any number in this brief -- recompute or directly verify everything you report on.
- Distinguish CONFIRMED defects (you independently reproduced/verified them against the actual source) from PLAUSIBLE concerns (you have a real reason to suspect something but could not fully verify it in the time available) -- label each finding accordingly.
- If you find nothing wrong after genuinely trying to falsify each item above, say so explicitly and explain what you checked -- do not manufacture a finding to appear thorough.

## Output format

Return a structured report:
- Summary: overall assessment in 2-3 sentences (does the correction hold up?).
- Confirmed defects: for each, cite the exact file/line, what is wrong, and what the correct value/behavior should be.
- Plausible concerns: for each, cite what you checked and why it's suspicious but unconfirmed.
- Explicitly cleared: for each of the 8 checklist items above, one sentence on what you verified and that it holds (or does not).
`

phase('Independent review')

const [reviewA, reviewB] = await parallel([
  () => agent(REVIEW_BRIEF + '\n\nYou are reviewer A. Focus extra scrutiny on items 2 (serving-overhead methodology/arithmetic) and 5 (raw artifacts vs reported numbers), but cover all 8 items.', {
    label: 'reviewer-A',
    effort: 'high',
  }),
  () => agent(REVIEW_BRIEF + '\n\nYou are reviewer B. Focus extra scrutiny on items 3 (repeated-run sampling/agreement calculation) and 4 (WANDS qualification), but cover all 8 items.', {
    label: 'reviewer-B',
    effort: 'high',
  }),
])

return { reviewA, reviewB }
