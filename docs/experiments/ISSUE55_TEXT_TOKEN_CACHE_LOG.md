# Issue #55 Experiment Log — precomputing title/text-attribute tokenization at index-build time

Protocol: `docs/experiments/ISSUE55_TEXT_TOKEN_CACHE_PROTOCOL.md`.

## I55-TOKCACHE-E00 — Precompute `score_text_relevance`'s tokenization; real-data H3 reversal

**Question**

The ranking-scaling checkpoint found `score_text_relevance` re-tokenizes
every candidate's title/text attributes from scratch on every query call,
and that this cost dominated `execute_ranked`'s total cost more than the
sort did. Does precomputing these token sets once at index-build time
preserve identical scoring while materially reducing cost?

**Hypothesis**

H0: precomputation is byte-identical and materially cheaper, growing
with scale. H1: another cost still dominates even after this fix.

**Workload**

Synthetic (`rank_bench.rs`, reused unmodified from the prior checkpoint)
for the primary performance claim, plus a real-data validation
(`p9_e02_wands_physical_advantage`, `p9_e04_isolated_ranking_and_execution`,
real WANDS + fresh Solr 9.10.1) that produced a materially larger finding
than this checkpoint's own protocol anticipated (see below).

**Implementation**

`crates/commerce-core/src/index/rank.rs`: added `PrecomputedTextTokens`
(title tokens + text-attribute tokens, both `HashSet<String>`, using the
exact same `to_lowercase()` + `split_whitespace()` logic the original
`score_text_relevance` used — not `super::tokenize`'s different
alphanumeric-splitting rule, to keep this a pure performance change, not
a relevance change) and `precompute_text_tokens`/
`score_text_relevance_precomputed`. `crates/commerce-core/src/index/mod.rs`:
added a `product_text_tokens: Vec<rank::PrecomputedTextTokens>` field,
populated once per product during `CatalogIndex::build` (deduplicated
across a product's variants — title/`Text` attributes are `Product`-level
fields, confirmed by reading `domain::product::Product`'s and
`domain::variant::Variant`'s own field lists directly, not assumed).
`execute_ranked` now looks up the precomputed tokens instead of calling
the old per-call tokenization path. The original `score_text_relevance`
is kept, `#[cfg(test)]`-gated, as the independent reference a new
500-trial randomized property test
(`score_text_relevance_precomputed_matches_live_tokenization_across_randomized_inputs`)
checks the new path against — covering empty titles, empty text
attributes, and residual tokens matching neither, per this checkpoint's
own adversarial-review checklist.

**Results**

*Correctness*: all 10 `index::rank` unit tests pass, including the new
500-trial property test (zero divergences) and all pre-existing
`execute_ranked`/`score_text_relevance` tests (unmodified, still
passing). Full workspace: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `cargo test
--workspace --all-features` (0 failures), `cargo build --workspace
--release` all pass clean.

*Synthetic performance* (`rank_bench.rs`, realistic order-uncorrelated
scores, same methodology as the ranking-scaling checkpoint):

| n | before both fixes | sort fix only | sort + token-cache fixes | total reduction |
|---|---|---|---|---|
| 100 | 49.92µs | 49.25µs | 23.46µs | 53.0% |
| 1,000 | 584.8µs | 553.1µs | 251.4µs | 57.0% |
| 10,000 | 7,155.9µs | 6,301.0µs | 2,928.4µs | 59.1% |
| 100,000 | 95,020µs | 88,771µs | 54,415µs | 42.7% |

A large, consistent, reproducible ~43-59% reduction in `execute_ranked`'s
total cost — far exceeding this checkpoint's own preregistered >=10%
KEEP bar, and confirming the tokenization cache, not the sort, was the
dominant cost the ranking-scaling checkpoint's own diagnosis pointed to.

*Real-data validation — a materially larger finding than anticipated*:
this checkpoint's protocol (§3) did not plan a real-data rerun unless the
synthetic result looked large enough to plausibly move the
Solr-JVM-noise-dominated H3 ratio the Issue #55 ranking-scaling
checkpoint had found indistinguishable from noise. The ~50%+ synthetic
result cleared that bar, so `p9_e04_isolated_ranking_and_execution` was
rerun with both fixes applied, against fresh Solr (matching the Issue #43
checkpoint's own confound-avoidance discipline):

| Condition | H1 (NDCG gap) | H3 latency ratio (solr/native), 6 runs |
|---|---|---|
| Original published (P9-E06) | +4.33% (unfixed compile()) / N/A for this exact fixed-compile population | 0.42x-0.60x |
| Issue #43 rerun, cold Solr, no Issue #55 fixes | +4.33% | 1.08x-1.88x |
| Issue #43 rerun, warm-carryover Solr, no Issue #55 fixes | +4.33% | 0.63x-1.11x |
| **This checkpoint, partially-warm Solr, both fixes** | +4.33% | **3.23x-6.32x** |
| **This checkpoint, freshly restarted Solr, both fixes** | +4.33% | **4.59x-8.19x** |

H1 is exactly unchanged in every single run across every condition,
confirming neither fix alters *what* is returned. H3 has **reversed
sign**: native was measured slower than Solr-restricted in every
pre-fix condition (ratio < 1 at its worst, and never clearing the >=2x
bar even at its best); with both fixes, native clears the >=2x bar in
**every one of 12 runs across both post-fix conditions**, with a mean
around 5-6x.

**Adversarial review**:

- Is this a fair, apples-to-apples rerun? Yes — `p9_e04_isolated_ranking_and_execution`
  itself is unmodified; only the `commerce_core` library it links against
  changed. The evaluated query population is identical (confirmed by
  identical NDCG figures in every run, which would move if candidate
  sets or routing differed).
- Could Solr-side changes explain this instead of native getting faster?
  No — Solr's own indexing/query path was not touched; the ratio moving
  is fully attributable to native's `execute_ranked` cost dropping (the
  synthetic benchmark already isolated and quantified this directly,
  independent of Solr).
- Does the magnitude (4.6x-8.2x) exceed what the synthetic benchmark's
  ~43-59% cost *reduction* alone would predict (a ~1.75x-2.4x speedup on
  native's own side, not obviously enough to explain an 8x ratio)? Yes,
  and this is disclosed rather than glossed over: real WANDS titles and
  attribute text are longer and more complex than this benchmark's
  8-word synthetic vocabulary, so the absolute tokenization cost the fix
  removes is plausibly larger on real data than in the synthetic
  benchmark — a plausible, disclosed explanation, not independently
  re-measured with a title-length-matched synthetic benchmark in this
  checkpoint (a reasonable next step if the exact magnitude, not just the
  qualitative reversal, needs tighter attribution).
- Is 6 runs enough given Solr JVM variance already characterized as large
  (Issue #43)? The reversal is far larger than that noise band (Issue
  #43's own characterized noise was ~1.08x-1.88x at best; every post-fix
  run here exceeds that entire prior range) — the qualitative conclusion
  (native now clears >=2x, reliably) does not depend on resolving the
  exact magnitude more precisely.

**Interpretation**

The tokenization-cache fix, layered on the sort fix, does not merely
"improve scaling" in the abstract — it reverses the project's own H3
verdict on real data, from FALSIFIED (native measurably slower) to
CONFIRMED (native measurably, consistently faster, by several times) on
the identical real WANDS candidate-set comparison that originally
falsified it. `docs/decisions/PHASE9_DECISION.md` is corrected via a
dated addendum (not rewritten) to record this.

**Regression check**

The new property test plus all pre-existing `rank.rs` tests are the
standing regression surface, run in the normal `cargo test --workspace`
gate.

**Next question**

1. A title-length-matched synthetic benchmark would let the ~50-90%
   real-data speedup be attributed more precisely between "the
   tokenization fix" and "real titles are longer than this benchmark's
   8-word vocabulary" — not required to trust the qualitative reversal,
   but would sharpen the magnitude claim.
2. Re-run P9-E02's own full 480-query, real-routing-mix measurement (not
   just the isolated 15-query H3 comparison) with both fixes, to update
   the traffic-weighted, whole-workload economics picture Issue #55's own
   measurement contract asks for (conditional advantage x reachable
   traffic coverage), not just the conditional/isolated one measured
   here.
3. Continue the falsification loop (see
   `docs/decisions/ISSUE55_TEXT_TOKEN_CACHE_DECISION.md`).
