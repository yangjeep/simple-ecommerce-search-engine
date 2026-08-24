# Phase 6B Experiment Log — Issue #21 Phase 6: WANDS Scale Ladder + Blocked-Dataset Survey

## Governing context

Phase 6A (Issue #23, `PHASE6A_DECISION.md`, PROCEED) found that WANDS'
facet-scan crossover (native `facet_counts_by_scan` stops beating Solr)
sits at 2,072–2,175 real candidates — lower than Phase 5's ESCI finding
of ~9,000–12,000 — and attributed the shift to WANDS' richer
per-candidate attribute maps (more attributes per product means more
hashmap work per scanned candidate during the facet scan).

That explanation was never directly tested: Phase 6A had only two data
points (ESCI, WANDS), each varying attribute complexity AND candidate-set
size AND facet cardinality together. Phase 6B asks the follow-on
question Issue #21's Phase 6 goal ("determine whether Phase 5's
structural wins and breakpoints are real commerce properties or
artifacts... characterize where native execution wins, where the
advantage falls below the 80x reference, where it crosses 1x") requires:
**is the crossover governed by candidate-set size, by per-candidate
attribute complexity, or both — and does the facet-scan cost model
continue linearly past WANDS' real ~43K-product ceiling, or does it show
genuine super-linear scaling (a real algorithmic problem, echoing Phase
5's own facet finding), or does it plateau?**

## Falsifiable hypothesis (stated before running the ladder)

If Phase 6A's "per-candidate attribute-map complexity, not raw scale"
explanation is the whole story, then holding attribute complexity and
facet cardinality FIXED while scaling only candidate-set size (via
controlled replication) should leave the crossover point roughly fixed
in absolute candidate count. If the crossover point instead continues to
shift downward (in relative terms) or the native/Solr speedup ratio
keeps degrading well past the real ceiling, that indicates a genuine
scale-dependent (not just complexity-dependent) cost in the native
`facet_counts_by_scan` implementation — falsifying "attribute complexity
alone" as a complete explanation and revealing an additional, distinct
mechanism.

**Pass/fail interpretation defined in advance**: measure native
mean-time-per-1,000-candidates at each replication tier; if it stays
within run-to-run noise (roughly flat) across 1x→20x, the "complexity,
not scale" explanation holds cleanly. If it grows monotonically and
materially beyond noise, the explanation must be revised to include a
genuine super-linear scaling component.

## Blocked-dataset survey (documented, not silently dropped)

Issue #21 names Retailrocket, H&M, and Amazon Reviews 2023 as further
Phase 6 datasets, and Havenask as the second required engine. Before
building the scale ladder, each was re-checked from this environment:

- **Retailrocket** — Kaggle-only distribution
  (`kaggle.com/datasets/retailrocket/ecommerce-dataset`).
  `curl` to `kaggle.com` fails with `CONNECT tunnel failed, response
  403` (organization-policy block, same class of failure Phase 6A found
  for Hugging Face). No GitHub/GCS mirror of the raw data was found.
  **Blocked**, consistent with Phase 6A's own prior finding.
- **H&M Personalized Fashion Recommendations** — also Kaggle-only
  (`kaggle.com/competitions/h-and-m-personalized-fashion-recommendations`).
  Same `kaggle.com` 403. **Blocked.**
- **Amazon Reviews 2023** — already documented blocked in Phase 6A;
  re-confirmed still blocked (Hugging Face, mcauleylab.ucsd.edu, and the
  GitHub Pages docs site all remain organization-policy 403'd; the
  authors' companion GitHub repo still contains only fetch scripts, not
  data).
- **Havenask** (Alibaba's specialized search engine, Issue #21's
  required second engine) — the source repository
  (`github.com/alibaba/havenask`) is reachable, but its own documented
  quickstart requires either (a) prebuilt Docker images from
  `registry.cn-hangzhou.aliyuncs.com` — not on this environment's network
  allowlist, confirmed unreachable — or (b) a source build needing
  `cpu > 2 cores, memory > 10G, disk > 50G` plus its own Bazel-driven
  dependency graph, most of which resolves against hosts outside the
  allowlist. Independently of the network question, this container has
  no Docker daemon at all (`docker info` fails: `dial unix
  /var/run/docker.sock: ... no such file or directory` — the same
  daemon-unreachable failure Round 1 (`docs/experiments/ROUND1_LOG.md`)
  already recorded for this project's environment). **Blocked on two
  independent grounds** (registry egress + no container runtime), not
  merely inconvenient. Havenask is also, by its own nature, a
  distributed-systems-oriented engine; CLAUDE.md's "avoid distributed
  systems work until the single-node thesis has been measured" is a
  secondary reason to defer this specific engine rather than force a
  fragile single-node workaround.
- **eCommerceSearchBench** — no accessible, confidently-identified source
  was located from this environment. Recorded as unresolved rather than
  guessed at.

Issue #23 (Phase 6A) itself anticipated exactly this situation and named
the fallback explicitly, in its own "Scale handling" section: *"Do not
duplicate/rescale WANDS merely to manufacture 100k/1M/10M product tiers.
Large-scale characterization belongs to subsequent Phase 6 datasets such
as Retailrocket, H&M where practical, Amazon Reviews 2023 when unblocked,
**or clearly labeled controlled stress datasets**."* With every named
real dataset and the second engine confirmed blocked, Phase 6B takes the
explicitly-sanctioned "clearly labeled controlled stress dataset" path —
this is a different thing from Issue #23's own non-goal ("no fake
scaling of WANDS to claim large-catalog evidence"), which is about not
*presenting* a replicated catalog as organic large-catalog evidence, not
a prohibition on controlled stress testing as a distinct, honestly
labeled methodology. Every result below is labeled accordingly.

## Methodology: controlled-stress scale ladder

`scripts/datasets/replicate_wands_scale.py` reads the real, checksummed
WANDS `catalog.jsonl` (42,994 products, re-verified against
`scripts/datasets/wands_checksums.sha256` this session) and writes K
byte-identical copies of every record, changing only the `id` field
(`<real_id>-r<replica_index>`) so Solr's unique-key constraint and the
Rust ingestion's per-line identity both hold. Tiers: 1x (real, 42,994),
2x (85,988), 5x (214,970), 10x (429,940), 20x (859,880).

**What this holds fixed by construction, and why that is the point**:
facet cardinality (distinct category/color/style/... values) and
per-candidate attribute-map shape are IDENTICAL to the real 1x catalog at
every tier. Only candidate-set size per real bucket scales linearly with
the replication factor. This isolates candidate-set size as a variable
from attribute/facet complexity, which Phase 6A's ESCI-vs-WANDS
comparison varied together. **This is not a model of organic catalog
growth** — a real 860K-product WANDS-like catalog would almost certainly
have more distinct categories/colors, not 20x-deeper buckets of the same
55 depth-1 nodes. Every artifact/doc in this experiment says so
explicitly.

Both native (`crates/phase6a-eval/src/bin/p6b_e00_scale_ladder.rs`) and
Solr (fresh core per tier — `wands_bench`, `wands_bench_2x`,
`wands_bench_5x`, `wands_bench_10x`, `wands_bench_20x` — indexed via
`scripts/datasets/solr_index_wands.py`) read the identical replicated
JSONL per tier, reusing Phase 6A's schema/mapping unchanged.

The benchmark itself reruns Phase 6A's own depth-1 color-facet crossover
sweep (same 7 real depth-1 checkpoints: Rugs, Storage & Organization,
Lighting, Outdoor, Décor & Pillows, Home Improvement, Furniture) at every
tier, plus two new workload classes never attempted in Phase 6A:

1. **Numeric-range filter on `average_rating`** (Issue #21's
   "numeric/price ranges" PLP operator class — WANDS has no price field
   at all, so P6A-E00 could not attempt this operator; `average_rating`
   is a real, already-ingested WANDS numeric attribute, exercising
   `commerce_core`'s existing generic `numeric_index` structural
   physical index, not a new mechanism).
2. **Mixed request**: a real depth-1 category filter AND an
   `average_rating` floor in the same query (Issue #21's "mixed PLP
   requests" class).

Timing methodology (REPS=30, WARMUP=5, percentile computation) is
unchanged from Phase 6A/Phase 5. Every row's native/Solr counts are
checked for exact equality before any timing number is trusted, same
discipline as every prior phase.

## Self-caught bug: `solr_index_wands.py`'s non-idempotent schema setup

Re-running `solr_index_wands.py` against an already-initialized core (a
path Phase 6A never needed, since each of its cores was indexed exactly
once) failed on the second attempt with `400: Multiple values
encountered for non multiValued copy field title_sort`. Root cause,
confirmed via `GET /schema/copyfields`: unlike `add-field` (which Solr
correctly rejects as "already exists"), Solr's `add-copy-field` silently
appends a duplicate rule on a repeat POST instead of erroring — two
identical `title -> title_sort` copy rules then attempt to copy the same
single value twice into a non-multiValued destination field, which Solr
correctly rejects. Fixed by checking `GET /schema/copyfields` first and
skipping the POST if the rule already exists (see the script's
`setup_schema` diff). This bug was latent since Phase 6A and only
surfaced because Phase 6B is the first time this repository re-runs the
WANDS indexing script against a live core — recorded per this project's
"record failed experiments, do not erase evidence" discipline even
though it is a tooling bug rather than a `commerce_core` bug.

## Raw results

`docs/research/artifacts/p6b_e00_scale_ladder_run1/results.csv` (50 rows:
5 tiers x 10 rows/tier), plus per-tier console logs
(`tier_{1x,2x,5x,10x,20x}.log`). **Correctness: 50/50 rows had exactly
matching native/Solr counts across all 5 tiers — zero mismatches.**

### Depth-1 color-facet crossover sweep, native mean time (ms) and candidate count, by tier

| checkpoint | 1x n / ms | 2x n / ms | 5x n / ms | 10x n / ms | 20x n / ms |
|---|---|---|---|---|---|
| Rugs | 2,002 / 1.62 | 4,004 / 4.80 | 10,010 / 15.71 | 20,020 / 29.58 | 40,040 / 81.97 |
| Storage & Organization | 2,175 / 3.35 | 4,350 / 9.08 | 10,875 / 20.22 | 21,750 / 50.99 | 43,500 / 99.92 |
| Lighting | 2,072 / 1.71 | 4,144 / 4.49 | 10,360 / 16.98 | 20,720 / 41.81 | 41,440 / 86.26 |
| Outdoor | 3,394 / 6.40 | 6,788 / 15.52 | 16,970 / 35.11 | 33,940 / 81.22 | 67,880 / 161.33 |
| Décor & Pillows | 4,612 / 9.56 | 9,224 / 20.20 | 23,060 / 42.62 | 46,120 / 108.21 | 92,240 / 168.32 |
| Home Improvement | 4,686 / 8.60 | 9,372 / 19.17 | 23,430 / 39.10 | 46,860 / 78.14 | 93,720 / 158.63 |
| Furniture | 16,039 / 28.70 | 32,078 / 56.59 | 80,195 / 116.19 | 160,390 / 225.72 | 320,780 / 467.92 |

Solr's own facet-compute mean time stayed roughly flat (2.2–9.0 ms)
across every tier and checkpoint — expected, since Solr facets off its
global inverted-index/docValues structure filtered by the query's
bitset, not a per-candidate scan.

### Native/Solr speedup (>1 = native wins), by tier

| checkpoint | 1x | 2x | 5x | 10x | 20x |
|---|---|---|---|---|---|
| Rugs | 2.40 | 0.51 | 0.17 | 0.09 | 0.06 |
| Storage & Organization | 1.00 | 0.27 | 0.12 | 0.05 | 0.03 |
| Lighting | 1.88 | 0.49 | 0.13 | 0.06 | 0.03 |
| Outdoor | 0.42 | 0.15 | 0.07 | 0.04 | 0.02 |
| Décor & Pillows | 0.27 | 0.11 | 0.06 | 0.04 | 0.04 |
| Home Improvement | 0.31 | 0.12 | 0.06 | 0.04 | 0.02 |
| Furniture | 0.11 | 0.05 | 0.03 | 0.03 | 0.02 |

**Important correction (see "Adversarial review" below): the 1x speedup
values in this table are NOT directly comparable, point-for-point, to
Phase 6A's own archived 1x numbers for the identical query.** Phase 6A's
`PHASE6A_LOG.md` reports Rugs (n=2,002) at 1.00x and Storage &
Organization (n=2,175) at 0.51x (a loss); this session's 1x tier measured
the identical real catalog and query but got 2.40x and 1.00x
respectively — driven by a real, identified cross-session confound (this
session's Solr JVM has all 5 scale-ladder cores loaded simultaneously,
sharing one 4GB heap, unlike Phase 6A's single-core session), not by
anything having changed in either system. The WITHIN-session ladder
trend (1x→2x→5x→10x→20x, all measured under the same multi-core-loaded
JVM throughout) is internally comparable and is the load-bearing claim
here; the exact absolute crossover value is not re-asserted
cross-session. Qualitatively, the real (1x) crossover still sits in the
same 2,000-2,200-candidate neighborhood Phase 6A found, and every
checkpoint is already in native-loss territory by 2x (n≈4,000+),
degrading further through 20x.

### Native per-1,000-candidate cost — corrected after adversarial review

The original draft of this log illustrated a "super-linear" trend using
only the Rugs checkpoint. Independent review (below) found that claim
did not survive checking all 7 checkpoints, and an aggregate across all
of them (total native ms / total candidates per tier) is **flat within
~13% across the whole 1x-20x ladder (1.71 -> 1.86 -> 1.63 -> 1.76 -> 1.75
ms/1,000-candidates)**, with no monotonic trend — the smallest two
checkpoints (Rugs, Lighting) show growth, the three largest (Décor &
Pillows, Home Improvement, Furniture) show flat-to-*declining*
per-candidate cost, and Storage & Organization/Outdoor are weak/mixed.
Per this log's own pre-registered pass/fail rule, this aggregate result
is a **pass** for Phase 6A's original "attribute complexity, not raw
scale" explanation, not the reversal the original draft claimed.

To resolve whether the Rugs/Lighting growth was a real, tier-size-linked
effect or REPS=30/WARMUP=5 in-process noise (the review's specific
concern, backed by the Phase 6A/6B discrepancy above), a follow-up
measurement was run: 5 independent process relaunches (fresh `cargo run`
each time, not just in-process reps) of the 1x, 2x, and 5x tiers for
Rugs/Lighting/Storage & Organization, plus a reversed-execution-order
check (5x measured chronologically *before* 1x, to rule out a
time/thermal-drift confound masquerading as a tier-size effect):

| checkpoint | 1x (5 runs, forward order) | 2x (5 runs) | 5x (5 runs) | 5x (reversed order, 3 runs) | 1x (reversed order, 3 runs) |
|---|---|---|---|---|---|
| Rugs | 0.603 ms/1000cand | 0.648 | 1.147 | 1.210 | 0.629 |
| Lighting | 0.645 | 0.637 | 1.212 | 1.148 | 0.640 |
| Storage & Organization | 0.763 | 0.780 | 1.422 | 1.499 | 0.727 |

Two things are now clear that were not clear from the single-run data:

1. **1x -> 2x is flat** for all three checkpoints once averaged over 5
   independent runs (e.g. Rugs 0.603 -> 0.648) — the apparent growth in
   the original single-run table between these two tiers was
   measurement noise, exactly as the review predicted.
2. **2x -> 5x shows a real, ~80-100% jump** in per-1,000-candidate cost,
   consistent in direction and magnitude across all three checkpoints,
   and **robust to reversing execution order** (the reversed-order 5x
   numbers, measured chronologically first, land at 1.21/1.15/1.50 —
   statistically indistinguishable from the forward-order 5x numbers
   measured chronologically fourth; the reversed-order 1x numbers,
   measured last, land back down at 0.63/0.64/0.73, matching the
   forward-order 1x numbers measured first). Reversing which tier was
   measured first/last did not change which tier has higher per-item
   cost, which rules out a simple time-order/thermal-drift explanation.

**Corrected interpretation**: there is a real, order-independent,
noise-robust scaling irregularity, but it is a discrete jump concentrated
somewhere in the ~4,000-11,000-candidate range for these specific
checkpoints, not a smooth super-linear curve across the whole 1x-20x
range, and it does not appear at all in the three largest checkpoints
(which are already well past this range even at 1x). The most plausible
unproven hypothesis is a candidate-set-size threshold related to CPU
cache locality (working set outgrowing some cache level) rather than an
algorithmic complexity defect in `facet_counts_by_scan` itself — no
profiling evidence was collected to confirm this in this pass, and it is
recorded as an unresolved risk / follow-up, not asserted as confirmed.
The originally-proposed mechanism (HashMap collision/resizing) is
**withdrawn**: `facet_counts_by_scan`'s accumulator and `AttributeMap`
are both `BTreeMap`, not `HashMap` (confirmed by reading
`crates/commerce-core/src/index/mod.rs` and
`crates/commerce-core/src/domain/attribute.rs`), and the one HashMap
that does grow with tier (`variant_location`) would predict a uniform
effect across every checkpoint at a given tier, which the data
contradicts.

Solr's own facet-compute time is **not uniformly flat** either, contrary
to the original draft's blanket "~2.5-9.0ms" characterization: Furniture
grows 3.04 -> 8.96ms (1x->20x, ~2.9x) and Décor & Pillows 2.61 -> 6.35ms
(~2.4x) — both correlated with bucket density, and both larger than the
range originally quoted.

### Tier-level build/RSS/index-size

| tier | products | build (ms) | index (bytes) | bytes/product | RSS after build (MB) |
|---|---|---|---|---|---|
| 1x | 42,994 | 850 | 8,108,054 | 188.6 | 191 |
| 2x | 85,988 | 1,703 | 15,413,890 | 179.3 | 364 |
| 5x | 214,970 | 4,857 | 37,129,734 | 172.7 | 873 |
| 10x | 429,940 | 9,900 | 73,346,846 | 170.6 | 1,734 |
| 20x | 859,880 | 20,484 | 145,660,040 | 169.4 | 3,451 |

Build time and RSS scale close to linearly with product count (roughly
24x time and 18x RSS for a 20x product-count increase — a mild
super-linear component in build time, consistent with the same
non-flat-per-item-cost pattern seen in the facet scan). Bytes/product
*decreases* slightly with scale (188.6 -> 169.4) — **flagged explicitly
as a likely artifact of the replication design** (fixed dictionary/facet
cardinality amortizing over a larger, but not more diverse, candidate
population), not evidence that native indexing becomes more
memory-efficient at real organic scale.

### Numeric-range filter (`average_rating`) crossover — new operator, first characterization

| threshold | 1x speedup | 2x | 5x | 10x | 20x |
|---|---|---|---|---|---|
| >= 4.0 | 5.62 | 2.17 | 0.73 | 0.50 | 0.21 |
| >= 4.5 | 5.47 | 2.37 | 1.14 | 0.51 | 0.23 |
| Furniture + >= 4.0 (mixed) | 4.98 | 2.03 | 0.77 | 0.47 | 0.30 |

Crossover for this operator sits between the 2x tier (≈57,000–64,000
candidates, native still ahead) and the 5x tier (≈142,000–160,000
candidates, roughly parity to slight loss) — a materially higher
absolute crossover point than the color-facet operator's ~2,100
candidates, consistent with `numeric_index` being a proper sorted-array
binary-search structure rather than a per-candidate hashmap scan.

## Adversarial review

Run as a 3-lens-plus-synthesis Workflow (confound analysis, statistical
rigor, honesty/scope, then an independent synthesis agent that
re-derived every headline number from the raw CSV itself rather than
trusting the three reviews). Full transcripts:
`/root/.claude/projects/-home-user-simple-ecommerce-search-engine/42c216bb-068c-521f-bf1b-8df9ff5387ed/subagents/workflows/wf_29bc6dd0-80e/journal.jsonl`.

**Confirmed real problems** (all addressed above or in this section):

1. The original "genuine super-linear scaling, ~2.5x over 20x" claim
   used only the Rugs checkpoint and did not survive checking all 7 —
   the sweep-wide aggregate is flat within ~13% with no monotonic trend.
   **Addressed**: replaced with the corrected, checkpoint-by-checkpoint
   analysis above, backed by 5 independent process relaunches plus a
   reversed-execution-order check.
2. No mechanism in the actual code supports a HashMap-collision growth
   story (`facet_counts_by_scan`'s accumulator and `AttributeMap` are
   both `BTreeMap`). **Addressed**: mechanism claim withdrawn; the real
   finding (a candidate-range-specific jump, hypothesized but not
   confirmed to be cache-locality-related) is stated with appropriate
   uncertainty instead.
3. "Solr facet time stays flat" overstated — Furniture and Décor &
   Pillows grow 2.4-2.9x across the ladder. **Addressed**: qualified
   above.
4. Design confound: candidate-set size (n) and total catalog size (N)
   are perfectly collinear in this ladder (every tier scales both by the
   same factor K), so this design cannot separate "cost is a function of
   candidates touched" from "cost is a function of total resident
   index/heap size" (RSS grows 191MB->3.45GB over the same tiers).
   **Not addressed this pass** — recorded as an explicit unresolved risk
   in `PHASE6B_DECISION.md`; a genuine decoupling (fixed-N/varying-n or
   vice versa) is named as a follow-up experiment, not fabricated after
   the fact.
5. Solr process lifecycle (all 5 scale-ladder cores loaded in one shared
   4GB-heap JVM throughout this session, unlike Phase 6A's single-core
   session) is a real, identified cross-session confound.
   **Addressed**: the cross-session absolute-value comparison to Phase
   6A is no longer claimed; only the within-session relative ladder
   trend is asserted.

**Concerns raised and rejected** (per the synthesis's own independent
re-verification): correctness/methodology being broken (rejected — 50/50
exact matches, re-confirmed); fabricated/undisclosed workload (rejected —
prominently and repeatedly self-labeled a controlled stress catalog);
REPS=30/WARMUP=5 being inadequate as a blanket claim (rejected — only
marginal for the smallest/fastest rows, already captured above); the
monotonic speedup-decay itself being proof of a scaling defect (rejected
as a standalone argument — with Solr's cost held near-constant by the
fixed-cardinality design, `speedup = k/(c*n)` necessarily decays toward
zero as n grows even under perfectly linear native cost; only the
per-item-cost table can distinguish "linear but structurally
disadvantaged" from "genuinely super-linear"); `index_bytes_per_product`
decreasing with scale being an error (rejected — real, monotonic,
corroborated trend, already correctly caveated as a replication-design
artifact).

**Overall verdict (synthesis agent's own words)**: crossover-shifts-
with-candidate-size and the new numeric-range crossover are "sound
enough to write up as-is, with the caveats... added." The original
"genuine super-linear scaling component" framing was "NOT sound enough
to write up as a confirmed finding as currently worded" and required the
additional between-run measurement performed above. The build-time-per-
product trend (19.78 -> 23.82 microseconds/product, smooth and monotonic
across all 5 tiers) was independently flagged as "a more credible
standalone signal of mild super-linear cost at scale than anything in
the facet-scan timing data" and is kept in the write-up on its own
merits.

## Interpretation (post-adversarial-review, corrected)

1. **Phase 6A's crossover finding reproduces qualitatively, not as an
   exact cross-session number.** The real 1x tier's crossover still sits
   in the same ~2,000-2,200-candidate neighborhood Phase 6A found; the
   exact speedup values differ from Phase 6A's archived numbers because
   of an identified cross-session Solr JVM confound (multiple cores
   sharing one heap this session vs. Phase 6A's single core), not
   because either system changed. This is itself a useful, disclosed
   finding about the limits of cross-session absolute-latency
   comparison, distinct from the within-session ladder trend.
2. **Phase 6A's "attribute complexity, not raw scale" explanation
   substantially holds**, contrary to this log's own first-draft
   conclusion: aggregated across all 7 checkpoints, native
   `facet_counts_by_scan`'s per-candidate cost is flat within ~13% across
   the whole 1x-20x ladder. It does NOT sharpen into a general
   super-linear law.
3. **A real, narrower, noise-robust exception exists**: for the three
   smallest real depth-1 checkpoints (Rugs, Lighting, Storage &
   Organization; n=2,002-2,175 at 1x), per-candidate cost is flat 1x->2x
   but jumps ~80-100% between 2x and 5x (n≈4,000-4,400 -> n≈10,000-
   10,900), confirmed robust to 5 independent process relaunches and to
   reversing measurement order. This is a genuine, order-independent
   phenomenon, not measurement noise, but its cause (hypothesized:
   candidate-set working set crossing a CPU cache-locality threshold) is
   unconfirmed and named as a follow-up, not asserted.
4. The native-loss region Issue #21 asks to characterize is real and
   substantial regardless of the above: every depth-1 checkpoint is
   already in native-loss territory by the 2x tier, and by 20x/Furniture
   (320,780 candidates) native is roughly 50x slower than Solr for the
   color-facet operator.
5. A genuinely new, previously-unmeasured operator (`average_rating`
   numeric-range filter, untestable in Phase 6A for lack of a price
   field) has its own real, distinct crossover at a materially higher
   candidate count (~57,000-160,000) than the facet operator's (~2,100)
   — evidence that crossover behavior is operator-specific, not a single
   catalog-wide constant, reinforcing the planner implication Issue #21
   already states ("native execution is promoted only inside a measured
   physical-advantage region").
6. Build time and RSS both scale close to linearly with product count,
   with a mild, consistent super-linear component in build time
   (19.78 -> 23.82 microseconds/product across 1x-20x) independently
   endorsed by adversarial review as the most credible standalone
   scaling-cost signal in this experiment.
