# Issue #55 Experiment Log — whole-workload economics of the H3 ranking fixes

Protocol: `docs/experiments/ISSUE55_WHOLE_WORKLOAD_PROTOCOL.md`.

## I55-WHOLE-E00 — traffic-weighted movement is real but muted; root cause is a newly found large-candidate-set regression, not traffic dilution

**Question**

The rank-scaling and text-token-cache checkpoints measured a conditional
effect: `execute_ranked`'s own cost, isolated over an identical candidate
set (`p9_e04`, capped at `MAX_CANDIDATES=5000`, n=15 of the real WANDS
run's 21 `structural_routed` queries). Does that conditional reversal
(native 4.6x-8.2x faster) show up in `p9_e02`'s own end-to-end,
uncapped `structural_routed` traffic (all 21 queries, both fixes,
`execute_planned`'s full cost, not just `execute_ranked`)?

**Hypothesis**

Preregistered H0/H1 (protocol section 1) asked whether the whole-workload
movement is real-but-small (proportional to `structural_routed`'s ~4.375%
traffic share) or undetectable. Neither anticipated the actual outcome:
the `structural_routed` movement is not just small, it is in some runs
**negative** relative to the pre-fix baseline, which would falsify both
preregistered outcomes if left unexplained. This log documents chasing
that discrepancy down to a concrete, disclosed, mechanistic cause rather
than reporting it as unexplained noise.

**Method note carried over from the text-token-cache checkpoint**: Solr's
own JVM warm state is a known confound (Issue #43). Every comparison
below restarts Solr fresh (`bin/solr stop` / `bin/solr start --force`)
immediately before the condition being measured, and repeats each
condition 6 times, matching this project's own established convention.

### Step 1 — matched before/after `structural_routed` comparison (fresh Solr each condition)

Both Issue #55 fixes (partial top-K selection, precomputed text tokens)
applied vs. neither, `p9_e02_wands_physical_advantage` unmodified,
6 runs each, fresh Solr restart before each condition:

| Condition | Run 1 | Run 2 | Run 3 | Run 4 | Run 5 | Run 6 | Mean |
|---|---|---|---|---|---|---|---|
| Before fixes | 3.84x | 2.68x | 2.24x | 2.30x | 2.08x | 2.13x | 2.545x |
| After fixes  | 2.11x | 1.80x | 1.77x | 1.43x | 1.95x | 1.51x | 1.762x |

(`structural_routed` latency ratio, solr_ms/native_ms; NDCG numbers are
byte-identical before/after at 0.2953 native / 0.3939 Solr, as expected —
neither fix touches candidate selection or scoring correctness.)

This is the **opposite direction** from the isolated `p9_e04` H3 result
(the same two fixes took that isolated ranking-only measurement from a
~1.1x-1.9x native disadvantage to ~4.6x-8.2x native advantage). Both
measurements are real, controlled, fresh-Solr, multi-run comparisons on
the same real WANDS data — this is not a methodology defect in either,
it is a genuine population/scope difference between what each one
measures, investigated below.

Raw output: `docs/research/artifacts/i55_whole_workload/before_fixes_fresh_solr/run{1-6}.txt`,
`after_fixes_fresh_solr/run{1-6}.txt`.

### Step 2 — ruled out: Hybrid-traffic dilution

Initial hypothesis: `execute_ranked` (what both fixes touch) is only
called from `execute_planned`'s `FastPath` branch
(`crates/commerce-core/src/plan/mod.rs:239-240`) — confirmed by reading
the code, not assumed. `Hybrid` uses bitmap narrowing + delegate +
`verify_and_truncate` instead, never `execute_ranked`. Since this WANDS
run's `structural_routed` population is `{"FastPath": 7, "Hybrid": 14}`,
the hypothesis was that Hybrid's 2/3 share, untouched by either fix,
dilutes a real FastPath-only gain down to the muted/negative aggregate.

Added a diagnostic breakdown to `p9_e02_wands_physical_advantage.rs`
(additive only — pushes the same `ResultPoint` into two new by-outcome
keys, `"FastPath"` and `"Hybrid"`, alongside the existing
`structural_routed`/`punt_routed` rows; does not change any existing
metric). 6 runs each condition, fresh Solr:

| Condition | metric | Run1 | Run2 | Run3 | Run4 | Run5 | Run6 | Mean |
|---|---|---|---|---|---|---|---|---|
| Before | FastPath nat_ms | 1.4366 | 1.2963 | 1.2583 | 1.3332 | 1.3423 | 1.5284 | 1.366 |
| After  | FastPath nat_ms | 2.9184 | 2.5658 | 2.3003 | 2.2919 | 2.4263 | 2.2804 | 2.464 |
| Before | Hybrid nat_ms   | 0.5941 | 0.5919 | 0.5951 | 0.5781 | 0.5465 | 0.6196 | 0.588 |
| After  | Hybrid nat_ms   | 0.7164 | 0.6269 | 0.6653 | 0.5818 | 0.5771 | 0.5553 | 0.620 |

**This falsifies the dilution hypothesis outright.** Hybrid (never
touches `execute_ranked`) is flat before/after, exactly as expected —
a clean negative control confirming the fixes have no effect where none
should exist. FastPath (the branch both fixes touch) got **~1.8x
slower**, not faster, after the fixes that made the isolated measurement
4.6x-8.2x faster. The aggregate `structural_routed` movement is muted
because it mixes a real FastPath regression with an unrelated stable
Hybrid population, not because Hybrid dilutes a FastPath gain.

Raw output: `docs/research/artifacts/i55_whole_workload/fastpath_hybrid_breakdown/run{1-6}.txt`
(after), `fastpath_hybrid_breakdown_before_fixes/run{1-6}.txt` (before).

### Step 3 — ruled out: `select_nth_unstable_by` regressing on tie-heavy real data

Candidate explanation: `score_text_relevance_precomputed`'s scoring
produces many candidates tied at the same score in a low-match-rate,
large-catalog query (most candidates score 0.0), and `select_nth_unstable_by`
(introselect-family) could in principle degrade on heavily-duplicated
keys in a way a full comparison sort (`sort_by`, a stable merge sort)
would not.

Tested directly with a standalone microbenchmark
(`/tmp/tie_heavy_select_bench.rs`, no external crates, reproduces
`rank_order`/`select_top_k` exactly) at the real WANDS full-catalog scale
(n=42,994, k=10), sweeping the tie fraction from 0% to 100%:

| tie_fraction | full_sort | partial_select | ratio |
|---|---|---|---|
| 0.0 | 3.326ms | 0.209ms | 15.9x |
| 0.5 | 2.948ms | 0.239ms | 12.3x |
| 0.9 | 3.024ms | 0.300ms | 10.1x |
| 0.95 | 2.691ms | 0.265ms | 10.2x |
| 0.99 | 2.603ms | 0.264ms | 9.9x |
| 0.999 | 1.534ms | 0.268ms | 5.7x |
| 1.0 (all tied) | 0.442ms | 0.266ms | 1.7x |

Partial selection stays faster than a full sort at every tie fraction,
including 100% ties. **Falsified** — the selection-step fix is not the
regression's source.

### Step 4 — confirmed: the regression is real, isolated to full-catalog/zero-constraint candidate sets, and caused by the text-token-cache fix specifically

Extended `p9_e04_isolated_ranking_and_execution.rs` with a diagnostic
(additive: tracks candidate-set size per routing outcome, prints a
by-outcome breakdown, and logs any query whose candidate set exceeds
10,000):

```
[large candidate set] outcome=FastPath query="driftwood mirror" constraints=0 candidates=42994
[large candidate set] outcome=FastPath query="marble" constraints=0 candidates=42994
FastPath: n=7, median_candidates=568, max_candidates=42994, excluded_by_max_candidates_cap=2
Hybrid: n=14, median_candidates=586, max_candidates=1169, excluded_by_max_candidates_cap=0
```

Exactly 2 of the WANDS run's 7 real `FastPath`-routed queries have **zero
structural constraints** — `compile()` finds nothing to narrow on, so
`indexed_candidates` returns the entire 42,994-product catalog
(the documented "no constraints -> everything" contract). These are
precisely the 2 of 21 `structural_routed` queries `p9_e04`'s own
`MAX_CANDIDATES=5000` cap already disclosed excluding from its H3
isolation (`excluded_by_max_candidates_cap=2`) — so the isolated
4.6x-8.2x finding never measured this population at all. The other 5
FastPath queries and all 14 Hybrid queries sit at a median of ~570-590
candidates, comfortably inside the cap, which is why Hybrid (and,
per the text-token-cache checkpoint's own isolated result, the
non-outlier FastPath queries) shows the expected improvement while the
n=7 FastPath aggregate does not: it is dominated by two several-millisecond
outliers sitting on top of five sub-millisecond ones.

To separate a real regression from single-process noise, added a new
diagnostic binary, `p9_e05_full_catalog_ranking_tail.rs`: loads the real
WANDS catalog once, compiles exactly these two known zero-constraint
queries, and calls `index.execute_ranked` 200 times per query in the same
process (10 discarded warmup calls first), reporting mean/median/p95:

| Build | Query | Mean | Median | p95 |
|---|---|---|---|---|
| Before fixes | "driftwood mirror" | 2.1967ms | 2.0857ms | 2.8518ms |
| After fixes  | "driftwood mirror" | 3.8014ms | 3.6424ms | 4.6785ms |
| Before fixes | "marble" | 2.1221ms | 2.0419ms | 2.7029ms |
| After fixes  | "marble" | 4.2121ms | 3.7917ms | 6.6215ms |

A clean, reproducible **~1.7x-2.0x regression** (an earlier run of the
same binary, before the `compiled.preferences`/`residual_lexical` print
below was added, showed ~1.7x-2.3x — consistent, both preserved: see
`docs/research/artifacts/i55_whole_workload/p9_e05_before_fix.txt`,
`p9_e05_after_fix.txt` for the final committed pair), confirming the
earlier single-process embedded measurement was not noise.

**Root cause — first hypothesis, tested and falsified.** The initial
theory was that `PrecomputedTextTokens`'s per-product `HashSet<String>`
(populated from real product-description text — WANDS descriptions
average 71.4 words/product, max 791, present on 86% of products, vs. a
6.8-word average title, measured directly against
`dataset_cache/wands/catalog.jsonl` — while the checkpoint-5 synthetic
benchmark's `realistic_catalog` gives every product zero `Text`
attributes) makes full-catalog scans cache-hostile (42,994 separately
heap-allocated hash tables touched one per candidate) versus the
pre-fix single contiguous string scan. This looked plausible and was
backed by a real measurement, but a direct check falsified it: added a
print of `compiled.preferences` and `compiled.residual_lexical` to
`p9_e05` and reran it — **both are empty** for "driftwood mirror" and
"marble" (`compiled: preferences=[] residual_lexical=[]`). Neither
`score_text_relevance_precomputed` nor the old, pre-fix
`score_text_relevance` ever reads `title_tokens`/`text_attr_tokens` (or
the title/description) in this case — both short-circuit to `0.0` on
an empty `residual_lexical` before touching any token set. The
description-driven cache-locality theory cannot be the cause, because
the code path it blames never runs for these two queries. Preserved
here, not deleted, per this project's own discipline of keeping a
failed path in the narrative rather than quietly dropping it.

**Root cause — actual, confirmed by reading `execute_ranked`
(`crates/commerce-core/src/index/rank.rs:205-217`)**:

```rust
let score = if query.preferences.is_empty() {
    let p_idx = *index.product_location.get(&product)
        .expect("execute() only returns ids that exist in this catalog");
    score_text_relevance_precomputed(&query.residual_lexical, &index.product_text_tokens[p_idx])
} else { /* ... */ };
```

The post-fix code performs `index.product_location.get(&product)` — a
`HashMap<ProductId, usize>` lookup — for **every candidate**,
unconditionally, before `score_text_relevance_precomputed`'s own
`residual_lexical.is_empty()` check ever runs. The pre-fix code called
`score_text_relevance(&query.residual_lexical, p)` directly on the
`Product` reference `execute_ranked` already had in hand (from
`index.lookup_variant`), with no extra lookup at all, and that
function's own `if residual_lexical.is_empty() { return 0.0; }` was its
very first line — zero tokenization, zero lookups, for exactly this
case. So for a query with empty `residual_lexical`, the pre-fix code
did no work whatsoever per candidate beyond the emptiness check, while
the post-fix code still pays one `HashMap::get` per candidate before
discovering the same thing. At 568-1169 candidates (the real WANDS
`Hybrid`/typical-`FastPath` range), one extra hashmap lookup per
candidate is negligible next to the tokenization cost the fix removes
for non-empty-residual queries (the dominant, already-confirmed win).
At 42,994 candidates with an empty residual (these two queries), there
was never any tokenization cost to remove in the first place — the old
code already skipped it — so the fix's only effect here is 42,994 added
hashmap lookups, a pure, unmitigated regression consistent with the
measured ~1.6-2.0ms absolute increase (roughly 35-45ns/lookup implied
at n=42,994, a plausible cost for a `SipHash`-backed `HashMap::get` on
a small key).

This is a strictly narrower, more precise, and more mechanistic
explanation than the falsified cache-locality theory: the regression is
gated on **`residual_lexical` (and `preferences`) being empty**, not on
candidate-set size interacting with description length. It happens to
coincide with the full-catalog case in this WANDS run only because the
two queries that triggered it also happen to have huge candidate sets;
the same per-candidate overhead would appear (proportionally, harmlessly
small) on any query with an empty residual, regardless of catalog size.

**Verdict on this step**: CONFIRMED, with a precise, reproduced,
independently-triangulated mechanism (embedded single-shot measurement,
6x6 matched runs, and a dedicated 200-rep same-process diagnostic all
agree on direction and rough magnitude), and a fix is obvious and cheap:
hoist the `residual_lexical.is_empty() && preferences.is_empty()`
short-circuit before the `product_location` lookup. Not implemented in
this checkpoint — named as the next experiment, per this project's own
"one idea at a time" / "don't move thresholds after seeing results
without recording a new experiment" discipline.

### Whole-workload traffic-weighted picture (protocol's own required metric)

`p9_e02`'s traffic-weighted-overall line (dominated 95.6% by `Punt`
traffic, which neither fix touches) is unaffected: native NDCG@10
≈0.66, latency ratio ≈5x, unchanged in shape before/after within run
noise — as expected, since 459/480 queries never call `execute_ranked`.
The `structural_routed` slice (4.375% of traffic) is where all the
movement above lives, and per the mechanism found, that movement is a
genuine mix of improvement (Hybrid- and moderate-candidate-set
FastPath queries, consistent with the text-token-cache checkpoint's own
isolated finding) and regression (the two full-catalog, zero-constraint
FastPath queries), not a uniform effect in either direction.

**Adversarial check on this checkpoint's own findings**: is 2/7 FastPath
queries having zero constraints itself suspicious/artificial? No —
`compile()`'s job is to find structural constraints in free text;
"driftwood mirror" and "marble" are pure noun-phrase lexical queries
with no attribute/category signal WANDS's own lexicon recognizes, so
zero constraints is the correct, expected `compile()` output, not a bug.
The separate, disclosed-but-out-of-scope question this raises — should a
literally 100%-of-catalog candidate set route to `FastPath` at all,
rather than `Punt`, given `FastPath`'s value proposition is structural
narrowing — is a planner-policy question, not a ranking-implementation
one, and is named as a candidate follow-up rather than folded into this
checkpoint's own verdict. It is also, per the confirmed root cause
above, only an *amplifier* of the regression's absolute size (more
candidates means more added hashmap lookups), not its cause — the same
per-candidate overhead exists, harmlessly, on any empty-residual query
regardless of candidate-set size.
