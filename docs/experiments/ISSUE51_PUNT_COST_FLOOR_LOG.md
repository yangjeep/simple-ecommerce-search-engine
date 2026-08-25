# Issue #51 Experiment Log — is the Punt-path delegate cost inherent, or a fixable treatment-side inefficiency?

Protocol: `docs/experiments/ISSUE51_PUNT_COST_FLOOR_PROTOCOL.md`.

## I51-PUNTFLOOR-E00 — H0 confirmed: the gap is dominated by the delegate's own inherent cost, not a fixable treatment-side inefficiency

**Implementation**

New binary `crates/issue42-eval/src/bin/r1_punt_cost_floor.rs`, reusing
`r1_full_gate_scale_rerun.rs`'s exact catalog scale/shape (42,995
products: 5 real fixture + 42,990 inert decoys). Measures, with the
same `LATENCY_BATCH=200`/`LATENCY_TRIALS=7` median-of-medians
methodology used throughout this session:

1. A same-process reproduction check: Treatment A vs. Treatment E's
   row-1 ("size 22") cost, to confirm this binary's own methodology
   before trusting any new number.
2. The **isolated floor**: `index.identifier_lookup("22")` followed by
   `BitmapTantivyDelegate::search(&["22".to_string()], None, 10)`,
   called directly via public API with zero `CommerceQuery`/`compile()`/
   `resolve_e`/`execute_planned`/`plan()` in the timed region — exactly
   the two real operations `execute_planned`'s `Punt` arm performs for
   this row, and nothing else.
3. A supplementary comparison: the same delegate call for `"decoy"`
   (present in all 42,990 decoy titles — a genuine, large posting-list
   match forcing real BM25 top-10 selection), to check whether the
   zero-hit `"22"` case understates a real match's cost.

**Reproduction check** (two independent runs):

| | Run 1 | Run 2 | Prior checkpoint (`ISSUE51_FULLGATE_SCALE_DECISION.md`) |
|---|---|---|---|
| Treatment A row 1 | 0.00018ms | 0.00022ms | ~0.0001-0.0003ms |
| Treatment E row 1 | 0.00901ms | 0.00993ms | ~0.0097-0.0104ms |

Same order of magnitude both runs; methodology confirmed consistent
with the prior checkpoint before trusting the new floor measurement.

**Isolated floor result**:

| | Run 1 | Run 2 |
|---|---|---|
| Isolated floor (`identifier_lookup` + `delegate.search`, "22", 0 hits) | 0.00891ms | 0.00936ms |
| Treatment E row 1 (same run) | 0.00901ms | 0.00993ms |
| Floor as % of measured | **98.8%** | **94.3%** |

Both runs land well clear of the preregistered >=80% H0 threshold (14-19
percentage points above it), so the protocol's own "run a second
confirmation only if near a threshold" clause does not apply — the
result is decisive without a third run. **H0 confirmed**: essentially
all of Treatment E's row-1 cost is the delegate call itself, not
surrounding treatment machinery (`resolve_e`'s `find_size_numeric_token`/
`lexicon_alternatives`/`corroborating_product_type` calls, `Resolution`
construction, `compile()`, or `execute_planned`'s own dispatch/`plan()`
overhead collectively account for at most ~1-6% of the measured gap).

**Supplementary finding, not in the original protocol but directly
relevant**: the zero-hit `"22"` floor (0.0089-0.0094ms) dramatically
*understates* a real match's cost. The same delegate call for `"decoy"`
(a genuine large-posting-list match, 10 hits returned, real BM25
top-10 selection over 42,990 matching documents) costs **0.42719ms —
45.6x more** than the zero-hit case. This means row 1's own ~50x
overhead vs. Treatment A (already far over the `<=5%` bar) is a
*conservative, best-case* example of the Punt-path's real cost, not a
worst case: a genuinely ambiguous query whose residual term actually
matches real catalog text (the typical case for real traffic, not this
synthetic fixture's deliberately-non-matching "22") would cost
substantially more, not less.

**Sanity check on the zero-hit measurement's validity**: `"22"` matches
nothing in this synthetic corpus (decoy titles embed it only inside a
larger numeric token like `"1000022"`, which Tantivy's default
tokenizer does not split into separate digit sequences), so this is a
real term-dictionary miss, not a data artifact. This does not weaken
the floor comparison's validity — the isolated call and the production
Punt-path call are the *literal same function with the literal same
argument* against the *literal same index*, so whatever Tantivy does
internally for this specific term is identical in both measurements by
construction.

## Decision

See `docs/decisions/ISSUE51_PUNT_COST_FLOOR_DECISION.md`.
