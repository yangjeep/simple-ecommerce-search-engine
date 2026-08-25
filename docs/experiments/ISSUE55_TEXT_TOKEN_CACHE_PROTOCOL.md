# Issue #55 Preregistered Protocol — precomputing title/text-attribute tokenization at index-build time

Committed before any production code is changed, per this repository's
governance.

## 0. What this is testing

The ranking-scaling experiment
(`docs/experiments/ISSUE55_RANK_SCALING_LOG.md`) found that
`score_text_relevance` (`crates/commerce-core/src/index/rank.rs`) —
`execute_ranked`'s real, shipping default ranking signal — re-lowercases
and re-tokenizes every candidate's title and every `Text` attribute on
**every single query call**, and that this cost is a larger contributor
to `execute_ranked`'s total cost than the full sort was (an ~18x cost
increase from turning on realistic scoring alone was observed in that
experiment's own synthetic benchmark). This is exactly the catalog-
dependent-work-repeated-per-query pattern this project's own core thesis
says to move to ingestion/compile time instead
(`CLAUDE.md`: "learn merchant-specific semantics offline... keep normal
query serving model-free, deterministic and cheap"; here the "learning"
is nothing more than tokenizing a title once, not an LLM step, but the
same principle applies).

## 1. Hypothesis

**H0 (fixable, real win)**: precomputing each product's title/text-attribute
token sets once at `CatalogIndex::build` time, and having `execute_ranked`
look them up instead of recomputing them, produces byte-identical scores
(same tokenization, just cached) while reducing `execute_ranked`'s
per-query cost, with the improvement growing with candidate-set size.
**H1 (not the full picture)**: even after this fix, per-query cost still
scales materially with candidate-set size for a reason this experiment
does not remove (e.g. the `HashSet::contains` lookups themselves, or
`CatalogIndex::execute`'s own candidate materialization) — a genuine,
disclosed finding, not assumed away.

## 2. Baseline

Current branch HEAD (already contains the Issue #55 ranking-scaling
fix). `crates/commerce-core/src/index/rank.rs` and
`crates/commerce-core/src/index/mod.rs` are production code — the same
RED-tests-before-production-fixes discipline applies as the prior
checkpoint.

## 3. Dataset

Synthetic only, matching the prior checkpoint's own (corrected) `rank_bench.rs`
methodology: realistic, order-uncorrelated titles built from a shuffled
vocabulary, so per-candidate scores vary and are not degenerate (a
lesson directly carried over from that checkpoint's own found-and-fixed
benchmark artifact). A real-data WANDS+Solr rerun is not planned for this
checkpoint unless the synthetic result is large enough to plausibly move
the already-measured, Solr-JVM-noise-dominated H3 ratio — the prior
checkpoint already established that bar is high (Solr variance alone
spans ~1.08x-1.88x at WANDS's real candidate-set sizes).

## 4. Treatment

Add `PrecomputedTextTokens` (title tokens, text-attribute tokens, both
`HashSet<String>`, computed with the exact same `to_lowercase()` +
`split_whitespace()` logic `score_text_relevance` already used — not
`CatalogIndex::tokenize`'s different alphanumeric-splitting tokenizer,
to avoid a scoring *behavior* change riding along with a *performance*
change). Computed once per product in `CatalogIndex::build`, stored
indexed by product array position (deduplicated across a product's
variants, since scoring is product-level, not variant-level).
`execute_ranked` looks up the precomputed tokens instead of calling the
old per-call tokenization path.

## 5. Metrics

- Correctness: `execute_ranked`'s existing tests (unmodified) must still
  pass, plus a new property test comparing the precomputed scoring path
  against the original live-tokenization function directly, across many
  randomized product/title/residual-token combinations.
- Performance: `rank_bench.rs` (already exists, reused unmodified) run
  before/after, at the same candidate-set sizes (100 to 100,000).

## 6. Preregistered gates

- **KEEP**: byte-identical scores proven across the property test suite,
  AND a material, reproducible reduction in `execute_ranked`'s own cost
  at scale (e.g. >=10% at n=10,000+, following the same "material, not
  incidental" standard this project applies elsewhere).
- **REFINE**: correct and some improvement, but smaller than expected or
  another cost now dominates — recorded precisely.
- **REJECT**: the property tests reveal the precomputed path cannot be
  made byte-identical without a materially different tokenization
  (a real behavior change, not merely a performance one), or no
  measurable improvement survives.

## 7. Scope boundary

Only `score_text_relevance`'s tokenization is targeted. This does not
touch `score_preferences` (the `query.preferences`-driven path, which
`compile_lexicon` never emits on real queries per `execute_ranked`'s own
P1-D comment, and which was already found provably inert as an
optimization target in that same comment's history). Does not change
tokenization semantics (no adoption of `CatalogIndex::tokenize`'s
different splitting rule) — that would be a relevance question, not a
performance one, and out of scope here.

## 8. Adversarial review checklist

- Does the property test cover products with zero title tokens, zero
  text attributes, and residual tokens that match neither?
- Is the precomputed store correctly deduplicated by product (not
  needlessly recomputed/duplicated per variant), and does that
  deduplication introduce any risk of stale data if two variants somehow
  had different effective title/text state (they cannot — title and
  `Text` attributes are `Product`-level fields, not `Variant`-level, per
  the domain model — verified by reading `domain::product::Product`'s
  own field list, not assumed)?
- Does removing the per-call tokenization change memory footprint
  materially (one extra token set per product, held for the index's
  lifetime) in a way worth disclosing even if not gating the verdict?
