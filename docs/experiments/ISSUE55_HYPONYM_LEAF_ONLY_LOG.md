# Issue #55 Experiment Log — leaf-only restriction for `ProductTypeAny` hyponym expansion

Protocol: `docs/experiments/ISSUE55_HYPONYM_LEAF_ONLY_PROTOCOL.md`.

## I55-HYPONYM-LEAF-E00 — H0 confirmed: all three named false positives eliminated, 94% of the recall win retained, and (new) structural_routed NDCG turns positive for the first time this session

**Implementation**

`crates/commerce-core/src/cold_start/profile.rs`: `product_type_hyponym_groups`
changed to compare each name's trailing path segment (`leaf_segment`,
new: `name.rsplit(" / ").next().unwrap_or(name)`) instead of the full
name; `compile_non_brand_lexicon` re-wired to `ProductTypeAny` exactly
as checkpoint 11 originally did. 5 new unit/property tests added
(`leaf_segment` behavior, the flagship "recliners" case explicitly
preserved, the "candles"/ancestor-bleed case explicitly excluded, the
scattered-word "bed accessories" case explicitly excluded); the existing
500-trial randomized property test needed no change (its vocabulary has
no path-shaped names, so `leaf_segment` is the identity there and the
soundness/completeness property it checks is unaffected).
`crates/commerce-core/tests/cold_start.rs`'s checkpoint-11 regression
test rewritten: "boots"/"hiking boots" now correctly merges (never one
of the disclosed false-positive shapes), and a new end-to-end test
reproduces the "candles" ancestor-bleed exclusion through the public
`compile_lexicon`/`compile` API. `cargo test --workspace --all-features`:
zero new failures.

**P9-E08 re-audit** (the actual falsification test): group count drops
from 245 to 149. All three named false positives from checkpoint 11's
audit are confirmed **gone**:

| Broader term | Before (full-path) | After (leaf-only) |
|---|---|---|
| `"candles"` | admitted "scented oils & diffusers" | **no group at all** (0 narrower names) |
| `"hot tubs"` | admitted "saunas" | **no group at all** |
| `"bed accessories"` | admitted "...shower curtain hooks" | **no group at all** |
| `"bath accessories"` | admitted "...shower curtain hooks" (false) + 2 more | **1 narrower name, the genuine one** ("...countertop bath accessories", whose own leaf literally contains "bath accessories") |
| `"recliners"` (flagship win) | 3 narrower names | **2 narrower names** (the one dropped, `"...chairs & seating / recliners"`, has the *identical* leaf `"recliners"` as the broader term itself -- same word count, correctly excluded by the strict-superset rule. Whether this pairing was a genuine hyponym relation or two records tagged inconsistently for the same real category is not independently resolved here; the measured 94.1% recall retention below already reflects its removal, along with every other change, as an empirical end-to-end number rather than a theoretical estimate) |

**Disclosed residual risk, confirmed present exactly as predicted**:
`"beds"` still admits `"cat beds"`/`"dog beds & mats"` (12 narrower
names total, down from 15) -- both are *clean*, non-path names, so
leaf-only restriction changes nothing for this pair, as the protocol
predicted up front. This is genuine cross-vertical lexical polysemy,
not an ancestor-breadcrumb artifact, and remains an open, quantified,
disclosed limitation of this mechanism (not claimed fixed).

**P9-E04 recall/ranking/speed**:

| | Baseline (0.4562) | Checkpoint 11 full-path (REJECTED) | This checkpoint (leaf-only) | Retention |
|---|---|---|---|---|
| Mean candidate-set recall | 0.4562 | 0.6968 (+24.06pp) | **0.6825 (+22.63pp)** | **94.1%** of the win |
| H1 native NDCG@10 (identical candidate set) | -- | 0.5813 vs. Solr 0.5292 (+9.84%) | 0.5780 vs. Solr 0.5292 (+9.21%) | still FALSIFIED (native not worse) |
| H3 latency ratio | -- | 2.54x (checkpoint 11's own final number) | **1.93x / 2.05x / 2.04x across 3 independent runs** (median 2.04x) | borderline, see below |

Recall retention (94.1%) clears the preregistered >=80% bar
comfortably. H1 continues to hold. **H3 is genuinely borderline**: one
of three runs (1.93x) falls just under the 2x bar, the other two clear
it (2.05x, 2.04x); native's own absolute latency (0.83-1.01ms) is
comparable to or better than checkpoint 11's 0.9091ms, while Solr's
absolute latency varied more across runs (1.70-2.31ms) -- consistent
with this project's already-disclosed Solr JVM-warmup measurement
variance (`ISSUE43_DECISION.md`) rather than a new, systematic
native-side cost. Reported honestly as borderline-but-net-positive
(median 2.04x, 2/3 runs individually clearing the bar), not smoothed
into an unqualified "CONFIRMED." Full detail:
`docs/research/artifacts/i55_product_type_hyponym/p9_e04_h3_leaf_fix_3runs.txt`.

**P9-E02 end-to-end re-check (not itself one of this checkpoint's
preregistered gates, but run per this session's own established
discipline whenever a change touches this area) -- a new, major
finding**:

| | Baseline (post-ingestion-fix) | This checkpoint (leaf-only) |
|---|---|---|
| `structural_routed` NDCG@10 relative gap | **-25.05%** (native worse) | **+5.37%** (native *better*) |
| `structural_routed` latency ratio | 1.60x | 1.19x |
| Tool's own printed verdict | STOP-leaning (per `ISSUE55_PRODUCT_CLASS_INGESTION_DECISION.md`) | **REVISE**: relevance now clears its own bar, latency does not |

This is the **first time in this entire multi-checkpoint session**
that `structural_routed` (`FastPath`+`Hybrid`) traffic's own NDCG@10 has
been measured as *better than Solr*, not worse -- reversing a gap this
project has characterized and re-measured across many prior
checkpoints (`PHASE9_DECISION.md`, the empty-residual/text-token-cache
checkpoints, the product_class ingestion checkpoint). It comes at a
real, disclosed cost: broader `ProductTypeAny` candidate sets are more
expensive to rank than a single precise `ProductType`, and
`structural_routed`'s own latency ratio drops from 1.60x to 1.19x,
below the `>=2x` bar this project has used throughout for a clean "both
axes" verdict. The qualitative sample's one native-empty-result query
("hardwood beds") is confirmed **pre-existing**, unchanged from the
baseline run (`p9_e04_after_revert.txt`'s own equivalent line already
showed `native top-3: []` before this checkpoint) -- not a new
regression this change introduced.

## Decision

See `docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`.
