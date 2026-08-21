# Phase 7 economic cost-per-tenant model (Issue #21 Phase 7 "economic output")

## Status and scope

This is a **synthesis document**, not a new experiment: it combines
already-measured, already-reproduced Phase 7 results (P7-E00 through
P7-E05, hypotheses H1/H5/H6/H7/H8) into an explicit pooled-vs-isolated
deployment cost model, as Issue #21's Phase 7 "economic output" section
asks for. No new binary was written and no new measurement was taken to
produce this document; every number below traces to a raw artifact
already committed under `docs/research/artifacts/`, cited by exact path,
and re-verified directly against that artifact (not transcribed from
memory or from `PHASE7_LOG.md`'s own summarized tables) — see
"Adversarial review" below for the correction history this document's
own first draft required.

**Deliberately not attempted**: converting any of these figures to a
dollar cost. Issue #21 itself asks to "keep hardware/cloud-price
assumptions separate from architecture-normalized metrics so the result
remains reproducible when prices change" — staying in physical memory
units (KB/MB/GB) throughout follows that instruction directly rather
than sidestepping it.

**Memory only**: this model says nothing about CPU/scheduling overhead,
network/connection handling, or I/O cost per tenant. Issue #21's
"Economic output" section names exactly seven required outputs; this
document addresses four of them well, addresses one partially, and
names two as explicit, undelivered gaps (see "Coverage against Issue
#21's Economic output ask" below) rather than silently omitting them.

## The three measured inputs

| Input | Value | Source | Order-sensitivity |
|---|---|---|---|
| In-process pooled marginal cost | 1.2558-1.2881 KB/product across the full swept range (~2.6% spread); `ratio_to_linear` stays 0.994-1.001 throughout | H5, P7-E02, `docs/research/artifacts/p7_e02_packing_ceiling_run1/results_run{1,2}.csv` (full ~65-row sweep, not just PHASE7_LOG.md's 5 printed checkpoints) | None — controlled-stress replication holds the per-tenant/schema shape fixed and varies only count; the tight `ratio_to_linear` band is the better-anchored basis for "no super-linear growth," not the raw KB/product ratio's precision |
| Per-OS-process floor (bare, spawn-and-exit) | 2,144-2,152 KB | H6, P7-E03, `docs/research/artifacts/p7_e03_cross_process_run1/results_run{1,2,3}.csv` | None — each measurement is a fresh, independent process |
| Per-OS-process floor (idle-resident, live worker pool) | 2,430.7-2,432.0 KB (peak within a 20s window; mean 2,431.1 KB) | H7, P7-E04, `docs/research/artifacts/p7_e04_long_running_run1/results_run{1,2,3}.csv`'s `idle_resident_mean` rows | None — confirmed stable (zero further growth) over a 9x longer window by H8 |
| Active-serving overhead (near-empty tenant) | 196 KB, exactly reproduced | H7 only, P7-E04, `results_run{1,2,3}.csv` (both "Faux Plants and Trees" and "Water Filter Pitchers" rows, all 3 runs) | None across 3 runs, all at the 20s window — P7-E05/H8 never re-tested near-empty tenants at 180s (`PHASE7_LOG.md` names this as untested) |
| Active-serving overhead (largest real tenant, Furniture, 16,039 products) | 896-1,024 KB, decelerating toward a bound (not fully flat) | H7 (20s: 896/896/900 KB) + H8 (180s: 1,004/1,004/1,024 KB) | Confirmed decelerating (~98% of total growth in first half of a 180s window) but not proven fully flat — see "Known gaps" |

The real 55-tenant WANDS partition (P7-E00's tenant model) sums to
**41,438 total products across the 55 real `category_depth_1`
categories** — not 42,994, which is the whole raw `catalog.jsonl` file
including records with no `category_depth_1` tag that never get
assigned to any tenant (confirmed against
`docs/research/artifacts/p7_e00_tenant_packing_run1/h1_forward_run2.csv`'s
`cumulative_products` column at `tenant_count=55`, which reads 41,438).

## Why the real-55-tenant raw measurement is NOT used as the pooled-cost anchor

P7-E00's own raw H1 measurement at the real 55-tenant scale shows the
exact same 41,438-product final state costing very different amounts of
marginal RSS depending on build order alone: **51,864-51,912 KB
forward** (largest tenant first; the three real per-run values are
51,864 / 51,912 / 51,896 KB, per `forward_run1.log` and
`h1_forward_run{2,3}.csv`) vs. **37,432-37,476 KB reversed** (largest
tenant last) — a ~39% difference for identical final data. This is the
exact build-order/allocator artifact P7-E00's own corrected finding
already named: at only 55 tenants, allocator/page-level effects are
large enough, relative to the total, to swamp the true per-product
signal.

H5's controlled-stress measurement is a better anchor for the pooled
per-product coefficient specifically because it operates at a scale
(100 to 6,500 tenants) where those same allocator effects become a
rounding error against the aggregate: 82,866 products at 1.263 KB/product
predicts 104,660 KB against an observed 104,448 KB (0.2% off). **This
document uses c = 1.263 KB/product as the pooled marginal-cost
coefficient** (the midpoint of H5's measured range, and PHASE7_LOG.md's
own headline figure), while being explicit that the FULL raw sweep shows
more local variability (1.2558-1.2881 KB/product, ~2.6%) than
PHASE7_LOG.md's 5-checkpoint table alone suggests (1.260-1.265, 0.4%) --
the tight, full-sweep `ratio_to_linear` band (0.994-1.001) is the more
defensible basis for "this scales linearly, not super-linearly," and is
what this document actually relies on for that claim. P7-E00's
real-55-tenant raw numbers are used here as evidence for *why* a
small-scale raw measurement should not be used as the model input, not
as a model input themselves.

## The model

**Pooled (N tenants, one process, P total products):**

```
RSS_pooled(P) ~ F + c*P + A_active_pooled
```

**Isolated (N tenants, one process each, P total products distributed
across them):**

```
RSS_isolated(N, P) ~ N*F + c*P + sum_i(A_active_i)
```

Where:
- `F` = per-process floor. Use 2,148 KB (H6 bare) as a conservative
  floor, or 2,431 KB (H7 idle-resident mean, `(2430.7+2430.7+2432.0)/3`)
  if every process is modeled as holding a live worker/connection-handler
  pool even when not actively serving a request — the more realistic
  assumption for a genuinely deployed service.
- `c` = 1.263 KB/product (H5).
- `A_active_i` = active-serving overhead while tenant `i`'s process is
  genuinely handling sustained query load: confirmed at 196 KB for a
  near-empty tenant and 896-1,024 KB for the largest real tenant
  (H7/H8); no data exists for the 52 real tenants of intermediate size.
- `A_active_pooled` = the SAME kind of overhead for the one pooled
  process, while it serves whichever tenant(s) are currently busy —
  genuinely unmeasured for the case of MULTIPLE simultaneously busy
  tenants inside one process (see "Known gaps" item 1); left
  unspecified here, not assumed zero.

**The `c*P` term is identical in both models** (the same aggregate data
has to live somewhere either way) and therefore **cancels out of the
isolation-vs-pooling premium**:

```
Premium = RSS_isolated - RSS_pooled ~ (N-1)*F + [sum_i(A_active_i) - A_active_pooled]
```

## Worked example: the real 55-tenant WANDS population

Using `F = 2,148` KB (H6 bare floor, the conservative choice) and
`N = 55`:

```
Floor-only premium = (55-1) * 2,148 KB = 54 * 2,148 = 115,992 KB ~ 113.3 MB
```

Using `F = 2,431` KB (H7 idle-resident mean, the realistic
always-warm-worker-pool choice):

```
Floor-only premium = 54 * 2,431 KB = 131,274 KB ~ 128.2 MB
```

This is a **lower bound**: it counts only the redundant per-process
floors isolation pays that pooling does not. It does NOT yet count any
active-serving overhead, because the model does not have per-tenant
active-serving figures for all 55 real tenants — only 3 sizes were ever
sampled (largest, middle, smallest; see H6/H7/H8). The middle-sized
sample ("Faux Plants and Trees", 5 products) showed the SAME 196 KB
active-serving overhead as the smallest (1 product) tenant, suggesting
active-serving overhead may be dominated by tenant SIZE at the small end
(most of WANDS' 55 real tenants are far closer to this small end than to
Furniture's 16,039 products) rather than scaling smoothly across the
whole real distribution — but this is an inference from 2 data points,
not a validated scaling law, and is named as such.

If every one of the 55 real tenants were (unrealistically) simultaneously
under sustained heavy load at once, and each contributed independently
somewhere between the near-empty (196 KB) and largest-tenant (~1,024 KB)
active-serving figure, isolation's additional active-serving cost alone
would range from roughly **10.5 MB (55 x 196 KB) to 55.0 MB (55 x 1,024
KB)** on top of the floor-only premium above — but seeing all 55
simultaneously under sustained heavy load is an extreme scenario Phase 7
never tested (P7-E01/H4 tested breadth of *touched* tenants, not all 55
under sustained heavy load at once), and is presented here only as an
illustrative bound, not a claimed real-world figure.

## Worked example: controlled-stress scale (up to 6,500 tenants)

H5's replicated population lets the SAME floor-only premium calculation
run at a scale no real WANDS partition reaches:

| N (tenants) | Floor-only premium (F=2,148 KB) | Floor-only premium (F=2,431 KB) |
|---|---|---|
| 100 | 99 * 2,148 = 212,652 KB (~207.7 MB) | 99 * 2,431 = 240,669 KB (~235.0 MB) |
| 1,000 | 999 * 2,148 = 2,145,852 KB (~2.05 GB) | 999 * 2,431 = 2,428,569 KB (~2.32 GB) |
| 6,500 | 6,499 * 2,148 = 13,959,852 KB (~13.3 GB) | 6,499 * 2,431 = 15,799,069 KB (~15.07 GB) |

At 6,500 tenants, the floor-only isolation premium alone (13.3-15.07 GB)
is comparable to or exceeds this project's ENTIRE ~15 GB container
budget referenced throughout P7-E02's own safety-cap discussion — while
the pooled model's actual measured cost at that same scale was **~5.9
GB** total (H5, `results_run{1,2}.csv`: 6,223,372 / 6,223,228 KB,
÷1,048,576 ≈ 5.94 GB — the real, safety-capped measurement). This is the
sharpest illustration Phase 7 has produced of the pooling advantage this
project's thesis document (`docs/WHY.md`) opens with: at scale, the
per-process floor genuinely dominates for an isolated deployment in a
way it structurally cannot for a pooled one.

## Illustrative "cost per million requests" (memory-only proxy)

Issue #21 names "cost per million requests" as a required economic
output. Phase 7 measured no CPU or dollar cost, but H7/H8's raw CSVs
already contain a `total_queries_served` column, so a memory-only proxy
(KB of active-serving overhead per million requests served) can be
computed with no new measurement:

| Tenant | Window | Active-serving overhead | Requests served | KB per million requests |
|---|---|---|---|---|
| Furniture (16,039 products) | 20s (H7) | 896-900 KB | 5,878-6,002 | ~145.8-149.5 MB/million |
| Furniture (16,039 products) | 180s (H8) | 1,004-1,024 KB | 50,764-51,608 | ~19.0-19.7 MB/million |
| Water Filter Pitchers (1 product) | 20s (H7) | 196 KB | 195.8M-198.1M | ~0.99-1.00 KB/million |
| Faux Plants and Trees (5 products) | 20s (H7) | 196 KB | 81.98M-83.58M | ~2.35-2.39 KB/million |

**This proxy is extremely window-length-sensitive, and that
sensitivity is itself a real finding, not just noise**: Furniture's
implied memory-cost-per-million-requests drops by roughly **7.5x**
between the 20-second window (~146-150 MB/million) and the 180-second
window (~19.0-19.7 MB/million) for the SAME tenant, because H8 already
established that Furniture's active-serving memory growth decelerates
sharply while its request volume keeps climbing roughly linearly. A
real production tenant, served for hours or days rather than seconds,
would very likely show a per-request memory cost far below even the
180-second figure — this proxy should be read as successive upper
bounds narrowing toward an unmeasured lower asymptote, not as a stable
number. Near-empty tenants' per-request cost is negligible (sub-3-KB
per million requests) at either window length, consistent with H1/H5's
finding that fixed per-tenant cost is small relative to aggregate data.

## Coverage against Issue #21's "Economic output" ask

Issue #21 names exactly seven required economic outputs. Scored
directly against the document above:

| Required output | Status |
|---|---|
| idle/low-QPS tenant fixed cost | **Delivered** — this is the document's strongest section (the `F` floor and its worked examples) |
| capacity stranded by isolation policy | **Delivered** — the `(N-1)*F` premium formula is exactly this metric |
| cost per active tenant | **Partial** — real numbers exist for only 2 of 55 real tenant sizes (near-empty, largest); no figure for the 52 intermediate-sized real tenants |
| cost per catalog-size tier | **Partial** — same 2-3 sample-point limitation; not an independent tier sweep |
| cost per million requests | **Partial, newly added above** — a defensible memory-only proxy, computed from already-cited data, but explicitly not a CPU/dollar cost and shown to be highly window-length-sensitive |
| tenants per fixed hardware envelope at target SLO | **Not delivered.** The ~15 GB container budget is invoked only as a size comparison, never inverted into a stated tenant count at a latency SLO. Phase 7's real latency/isolation evidence (H2's cross-tenant p99 ratios, H4's throughput-under-breadth results) is never combined with this memory model to produce an SLO-conditioned tenant count. This is a genuine, named gap — the ingredients exist in `docs/experiments/PHASE7_LOG.md` but combining them is future work. |
| backend requests avoided | **Not delivered.** Phase 7's experiments (P7-E00 through P7-E05) are pure in-process/cross-process memory measurements over `commerce_core::index::CatalogIndex` and never touch Solr or issue a lexical-fallback request. This metric requires Phase 3/4's admission-rate evidence (the ~5.80%/0.38% coverage figures) combined with a multi-tenant request-volume model neither exists yet for Phase 7's tenant population. Named here as a genuine gap, not silently dropped. |

## Known gaps in this model (named explicitly, not implied away)

1. **Concurrent multi-tenant active-serving cost inside ONE pooled
   process is untested.** H7/H8 measured active-serving overhead for
   exactly ONE tenant being queried in an otherwise-idle process. Whether
   a pooled process serving MANY tenants simultaneously accrues each
   tenant's active-serving overhead additively, shares it, or something
   else entirely is not measured — this model conservatively omits it
   from the pooled side (`A_active_pooled` is left unspecified /
   effectively treated as unknown rather than assumed zero) rather than
   guessing.
2. **Only 3 of 55 real tenant sizes have any active-serving figure at
   all** (H6/H7/H8 sampled largest/middle/smallest only). The "10.5-55.0
   MB" illustrative range above is a bound from those 3 points, not a
   validated per-tenant model across the real size distribution.
3. **H8's deceleration is not proven to be a full plateau.** Furniture's
   `second_half_growth_kb` was positive in all 3 P7-E05 runs (20 / 28 /
   20 KB) — small, roughly two orders of magnitude below the initial
   climb, but real. `PHASE7_LOG.md`'s narrative additionally describes a
   continuing multi-sample tail creep specifically in 2 of the 3 runs;
   that finer per-sample distinction is not independently re-verifiable
   from a committed artifact (P7-E05 archived only summary CSVs, no
   per-run `.log` files). The 896-1,024 KB Furniture figure used here is
   the peak observed within a 180-second window, not a proven asymptote.
4. **Two of Issue #21's seven "Economic output" metrics are not
   delivered** (tenants-per-envelope-at-SLO; backend requests avoided) —
   see "Coverage" table above for exactly what is missing and why.
5. **Memory only** for every output this document DOES deliver.
   CPU/scheduling and network/connection overhead per tenant are
   entirely unmeasured by Phase 7.
6. **One tenant model.** All figures come from WANDS' real category
   partitions or controlled-stress replicas of them (Phase 7's
   standing, previously-disclosed limitation) — real independent SaaS
   tenants would differ in ways this partition cannot capture.

## Adversarial review

A 3-lens Workflow review (arithmetic/consistency, scope-honesty, and
does-this-answer-Issue-#21's-ask) plus a synthesis pass checked this
document's first draft against the raw CSVs/logs directly (not against
`PHASE7_LOG.md`'s own summarized tables) and against Issue #21's actual
text (fetched directly via the GitHub API). It found nine real issues,
all fixed in this version, matching this project's "record and fix, do
not silently rewrite" discipline:

1. **A fabricated `F` figure.** The first draft's "H7 idle-resident
   mean" of 2,462 KB (drawn from a stated range of "2,431-2,493 KB")
   traced to neither committed artifact nor any real Phase 7 measurement
   — 2,462 was exactly the midpoint of an invented range. Root cause:
   the draft was written from a recollection of an ad-hoc, uncommitted
   regression-sanity-check run performed while refactoring the P7-E04
   binary (never saved as an official run, deliberately reverted from
   git), not from the three actual committed `results_run{1,2,3}.csv`
   files. Fixed: `F` is now 2,431 KB, the real mean of
   2,430.7/2,430.7/2,432.0 KB from the three official runs, and every
   downstream "F=2,431" table entry was recomputed from that corrected
   value (independently re-verified with a fresh calculation, which also
   caught a small arithmetic slip in the review's own draft correction
   for the N=6,500 row — 15,799,069 KB, not 15,798,069 — before this
   version was written).
2. **Misattributed near-empty-tenant reproducibility.** The first draft
   claimed 196 KB was confirmed across "5 total runs (3 at 20s, 2 more
   at 180s)," citing both H7 and H8. P7-E05 never re-tested near-empty
   tenants — its committed CSVs contain only `idle_resident` and
   `active_resident,Furniture` rows, and `PHASE7_LOG.md` itself already
   names this as untested. Fixed: now correctly attributed to H7/P7-E04
   only, 3 runs, 20s window.
3. **Overstated H5 precision.** The first draft's "1.260-1.263 KB/product
   (0.4% spread)" was both internally inconsistent (that range is a
   0.24% spread, not 0.4%) and, once checked against the FULL ~65-row
   sweep in both raw CSVs (not just `PHASE7_LOG.md`'s 5 printed
   checkpoints), an understatement of the real variability
   (1.2558-1.2881 KB/product, ~2.6% — independently re-verified with a
   direct Python scan of both files). Fixed: the full range is now
   stated, and the "no super-linear growth" claim is now anchored on the
   tighter, more defensible `ratio_to_linear` column (0.994-1.001,
   independently re-verified) rather than the raw ratio's precision.
4. **An omitted real run.** The first draft's forward-order N=55 range
   (51,896-51,912 KB) missed a third genuine run recorded only in
   `forward_run1.log` (51,864 KB, independently re-verified by reading
   the log directly) rather than a standalone CSV. Fixed: range is now
   51,864-51,912 KB.
5. **A unit-conversion error.** "~6.2 GB" for the pooled model's actual
   6,500-tenant cost was computed inconsistently with the document's own
   1024-based convention used everywhere else; the correct figure is
   ~5.9 GB (independently re-verified). Fixed.
6. **A self-contradictory restatement.** The body text's correctly-
   derived "10.5-55.0 MB" illustrative bound was restated in "Known
   gaps" as "10.6-56.3 MB" — a different, non-reproducing pair. Fixed to
   match the correct figure throughout.
7. **Two of seven required Issue #21 outputs silently absent.**
   "Tenants per envelope at SLO" and "backend requests avoided" were
   neither delivered nor named as gaps in the first draft, despite the
   document otherwise following this project's "name gaps explicitly"
   discipline well. Fixed: both now named explicitly in "Coverage" and
   "Known gaps," and "cost per million requests" — trivially computable
   from data the document already cited — was actually computed rather
   than left as an unnamed gap.
8. **This section itself was an unfilled placeholder** in the first
   draft ("[Findings and corrections, if any, recorded below.]"). Fixed:
   populated with the above.
9. **A minor overstatement of P7-E05's reproducibility granularity**
   ("persists in 2 of 3 runs") where the committed CSVs show the
   residual as positive in all 3 runs, with the finer 2-of-3 distinction
   traceable only to `PHASE7_LOG.md`'s narrative text, not to a
   separately archived P7-E05 per-run log. Fixed to state what the
   committed artifact actually shows, with the narrative distinction
   named as such.

Every fix above was independently re-verified against the raw CSVs/logs
before being applied (not merely accepted from the review's transcript),
including catching one small arithmetic error in the review's own
proposed correction (item 1's 6,500-tenant figure).
