# Issue #55 — mechanism check on the Hybrid-bucket zero-hit pattern (append-only log)

Continues `ISSUE55_ROUTING_OUTCOME_REPLICATION_DECISION.md`'s own named
open thread: automotive's post-fix Hybrid gap (-38.75%) is still large.
Is this a genuine, unexplained relevance gap, or a specific, disclosed
mechanism?

## Entry 1 — qualitative probe

Wrote `crates/issue55-eval/src/bin/i55_e15_automotive_hybrid_gap_probe.rs`:
rebuilds the exact same catalog/lexicon/index/policy
`issue35_eval::eval::run_vertical_eval` uses (same crate's public
ingestion functions, same `MIN_ENUM_FREQUENCY`/`PlannerPolicy`/K),
filters to `Hybrid`-routed, NDCG-scoreable queries, and prints native vs.
Solr ranked titles with real ESCI judgment labels side by side.

First run (automotive, 13 of 32 queries visible before truncating the
terminal output): 6 of 13 had **zero native hits at all** (`hits.is_empty()`,
not merely a low ranking). Of those six, four were Honda-brand queries
with no real relevant product in this catalog either way (Solr also
found nothing judged), a genuine "no answer exists" case, not a bug. But
one stood out: `"castrol 10w30"` -> `Brand(Castrol)`, native
**0 hits**, Solr NDCG=0.4791 with a real `Exact`-labeled match ("Castrol
03093 GTX 10W-30 Motor Oil, 5 Quart") in its top 3.

## Entry 2 — mechanism, confirmed by reading the actual code path

Two independent, compounding reasons a Brand-constrained Hybrid query
can return literally zero native hits when a real match exists:

**(a) The delegate's tokenizer splits on punctuation.**
`BitmapTantivyDelegate::search` (`crates/phase9-eval/src/bitmap_delegate.rs`)
parses residual-lexical text via `tantivy::query::QueryParser::for_index`,
which tokenizes `TEXT`-typed fields with Tantivy's built-in default
tokenizer (`SimpleTokenizer` + `LowerCaser`), splitting on any
non-alphanumeric character. `"10W-30"` in a real title indexes as two
separate tokens `["10w", "30"]`; `"castrol"` is already consumed into the
compiled `Brand` structural constraint, so the only text that reaches the
delegate is the residual token `"10w30"` (no hyphen, one token) -- a term
that never appears in the index (only `"10w"` and `"30"` do), so the
term query matches nothing. This is consistent with, and mechanistically
explains, the observed zero-hit "castrol 10w30" case; it was not
independently verified with an isolated tokenizer unit test, so it is
reported as the well-documented, standard behavior of Tantivy's own
default tokenizer applied to this code path, not as freshly re-derived
from scratch.

**(b) Issue #42 R2's residual-lexical fallback (the mechanism specifically
designed to prevent an empty delegate result from becoming an empty
answer) cannot fire here, for two independent reasons:**

1. `issue35_eval::eval::run_vertical_eval` passes `residual_policy: None`
   to every `execute_planned` call (`crates/issue35-eval/src/eval.rs`) --
   the fallback's own second precondition (`residual_fallback_hits`,
   `crates/commerce-core/src/plan/mod.rs`) is `residual_policy?`, so it
   returns `None` immediately regardless of anything else.
2. Even if a `residual_policy` were wired up, `residual_fallback_hits`'s
   third precondition requires `corroborating_product_type(query)` to
   find a `StructuralConstraint::ProductType` in the compiled query.
   ESCI ingestion (`crates/issue35-eval/src/lib.rs`) never registers any
   product type at all (`UNKNOWN_PRODUCT_TYPE`, left unregistered,
   invisible to the lexicon, exactly as Issue #35's own protocol
   discloses) -- so `corroborating_product_type` returns `None` for
   *every* ESCI query, unconditionally. The fallback is structurally
   inert on this class of dataset, not merely unused by this harness.

**This is not a hidden correctness gap in the fallback's own design**:
`residual_fallback_hits`'s ranking call (`index.execute_ranked`) goes
through `index.execute`, which itself calls
`self.indexed_candidates(&query.constraints)` first -- i.e. Brand/
attribute constraints are *already* enforced before any ranking happens,
the same safety `FastPath` itself relies on. Requiring a `ProductType`
constraint specifically (rather than accepting Brand-level corroboration)
was a deliberate scope choice of the original R2 design
(`docs/experiments/ISSUE42_LOG.md#i42-r2`, `docs/adr/0012-residual-lexical-policy.md`),
not an oversight -- relaxing it is a genuine precision/confidence
question to design and preregister, not a free correctness win, and is
named as an open follow-up below rather than treated as an obvious fix.

## Entry 3 — quantified across all three ESCI verticals

Generalized the probe to accept `<vertical_label> <products_path>
<queries_path> <solr_base_url>` as CLI args (defaults to automotive
when omitted, preserving the original invocation) and added a
zero-hit-mechanism counter: of every `Hybrid`-routed, NDCG-scoreable
query, how many return zero native hits at all, and of those, how many
have Solr find a real judged-relevant (non-Irrelevant) product under the
identical Brand/color `fq`. Raw output:
`docs/research/artifacts/i55_hybrid_zero_hit_probe/{automotive,electronics,beauty}.txt`.

```
                 evaluated (n)   zero-hit         recoverable-miss (zero-hit AND solr found real relevance)
Automotive       32              14 (43.75%)      4 (12.5%)
Electronics      48               8 (16.7%)       1 (2.1%)
Beauty           38              15 (39.5%)       3 (7.9%)
```

Cross-referenced against `ISSUE55_ROUTING_OUTCOME_REPLICATION_DECISION.md`'s
own post-fix Hybrid gaps (automotive -38.75%, electronics -12.00%, beauty
-11.49%): automotive has both the worst NDCG gap and the worst zero-hit/
recoverable-miss rates, consistent with this mechanism being a real,
partial contributor there. The correlation is not clean across all
three, though (beauty's zero-hit rate, 39.5%, is much closer to
automotive's than to electronics', yet beauty's NDCG gap is close to
electronics') -- reported honestly as a partial, not full, explanation.

See the decision doc for the verdict and what this does/does not
establish.
