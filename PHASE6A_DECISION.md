# Phase 6A Decision (Issue #23, governed by Epic #21)

**Decision: PROCEED** to the next independent Phase 6 dataset / Havenask
comparison, without changing the underlying commerce-native mechanism —
with one explicit narrowing of Phase 5's own claim: the specific
facet-crossover candidate-count Phase 5 reported (~9,000–12,000) does
**not** generalize as a fixed number; the crossover *mechanism* (a
candidate-cardinality-dependent breakpoint in the `_by_scan` facet
method) and every filter/pagination/concurrency finding reproduce
robustly on an independent, genuinely hierarchical dataset.

Phase 5's structural filter, pagination, and concurrency advantages
reproduce almost exactly in order of magnitude on WANDS (a real,
independent commerce vertical substituted for the unreachable Amazon
Reviews 2023 — see the blocker section below), clearing Issue #18's 80x
physical-multiplier floor at every real size tested. Facet and sort
reproduce the *qualitative* breakpoint pattern Phase 5 found (a real
native win at small/medium candidate counts, collapsing at large ones),
but the facet crossover occurs at a substantially *lower* real candidate
count on WANDS (~2,072–2,175) than on ESCI (~9,000–12,000) — a real,
explained (not merely observed) shift, traced to a plausible, concrete
physical cause: WANDS' richer per-product attribute map makes each
scanned candidate more expensive. Sort, which does not touch the
attribute map at all, shows **no** such shift and stays close to Phase
5's own numbers at a matched candidate count — a natural control case
that strengthens rather than undermines the explanation.

This document is Phase 6A's terminal decision artifact for Issue #23,
governed by Epic #21. It does not overwrite `PHASE5_DECISION.md` (a
different dataset/issue) or any earlier phase decision — all remain
historically accurate for their own scope.

## Recap: what Phase 6A was asked to answer

Issue #23: when Phase 5's browse/PLP benchmark is rerun on a genuinely
category-structured commerce dataset, do the same native structural
advantages and cardinality/selectivity breakpoints reproduce? The named
primary dataset was Amazon Reviews 2023 metadata.

## Dataset blocker and substitution

Amazon Reviews 2023's only real distribution channels (huggingface.co and
every CDN subdomain, mcauleylab.ucsd.edu, amazon-reviews-2023.github.io)
are blocked by this environment's egress policy — confirmed as an
organization-policy 403 across multiple independent hosts, not a
transient failure, with no GitHub-LFS or GCS mirror of the raw metadata
found anywhere. Retailrocket (Epic #21's other named alternative) is
Kaggle-only, also blocked. **WANDS** (Wayfair ANnotation Dataset,
`github.com/wayfair/WANDS`, pinned commit `3b74dcf4`) is fully reachable
and was substituted with explicit user sign-off. See
`docs/experiments/PHASE6A_LOG.md`'s "Dataset blocker and substitution"
section for the full investigation, and
`docs/research/artifacts/p6a_dataset_acquisition/manifest.json` for
provenance/checksums/profiling.

This substitution is a real, disclosed scope change, not a transparent
swap: WANDS has genuine category hierarchy and an independent
product-type taxonomy (both real improvements over ESCI for this
purpose), but no price field, no parent-ASIN/variant grouping, and a
42,994-product ceiling (vs ESCI's 1,215,854).

## Architecture tested

Identical to Phase 5: `commerce_core::index::CatalogIndex`'s existing
structural filtering (`indexed_candidates`), the `_by_scan` facet methods
(now extended to `category`/`product_type`, mirroring `brand`), and
`execute_ranked`-adjacent top-K sort — against a fairly-configured Solr
baseline (docValues on every filter/sort/facet field, a dedicated
`title_sort` field). No new engine optimization, planner heuristic, or
LLM/control-plane behavior was introduced, per Issue #23's own governing
rule.

## Datasets / workloads

- **WANDS**: 42,994 real products, real category hierarchy (depth 0–6),
  real `product_class` (860 distinct), `color` (2,825 distinct, high
  cardinality), `style` (65 distinct, low cardinality),
  `material`/`primarymaterial`/`shape` (secondary facets).
- Real category-leaf and depth-3 subtree groups selected across real
  size buckets (seeded, ChaCha8Rng seed=7); a targeted depth-1 sweep
  (real values, not random) to bracket the facet-crossover region
  precisely, since leaf categories cap out at 1,103 real products.
- 41 measured request rows (P6A-E00) + a 4-level concurrency sweep
  (P6A-E01), both with a full correctness gate before any timing claim.

## Measured results

**Correctness**: 41/41 rows match on true candidate/filter count. The
only 2 residual facet-map mismatches are the exact same explained,
non-bug missing-value-sentinel pattern Phase 5 already documented for
`BrandId(0)`, reproducing here for `ProductTypeId(0)`.

**The 80x physical-multiplier floor**:

| request class | clears 80x at every real size tested? |
|---|---|
| category render / subtree browse (filter-only) | **yes** — 8,020x–26,769x |
| deep pagination | **yes** — 19,203x–26,330x |
| concurrency (1–8 workers) | **yes** — native's single thread beats Solr's best 8-worker throughput by ~717x–2,578x |
| color facet (high cardinality, 2,825 distinct) | no — fails at "medium" (13.3x), reverses to a loss at "large" (1.39x) and beyond (0.07x at 16,039 candidates) |
| product_class facet (860 distinct, no ESCI analog) | no — fails at "large" (26.2x), holds through "medium" (232x) |
| style facet (low cardinality, 65 distinct) | no — nearly identical threshold to color despite far lower cardinality (1.74x at "large") |
| sort by title | no — fails at "large" (16.6x), matching Phase 5's own brand-scoped "large" number (16.97x) closely |
| sort by rating desc (disclosed substitute) | no — fails at "large" (8.95x) |

**Facet-crossover characterization** (real depth-1 subtrees): parity at
2,072 candidates (1.01x), a loss by 2,175 (0.51x), continuing to 0.07x at
16,039. See `docs/experiments/PHASE6A_LOG.md` for the full table and the
attribute-map-size explanation.

## Cross-dataset comparison: Phase 5 (ESCI) vs Phase 6A (WANDS)

Classified individually per Issue #23's framework — not averaged into
one headline number.

| Phase 5 finding | Phase 6A classification | Basis |
|---|---|---|
| Structural filter-only clears 80x at every real size | **ROBUST** | 8,020x–26,769x here vs 2,799x–18,354x on ESCI — same order of magnitude, same qualitative result |
| Deep pagination clears 80x at every real size | **ROBUST** | 19,203x–26,330x here vs 5,331x–12,715x on ESCI |
| Concurrency: native single-thread beats Solr's best multi-worker throughput by orders of magnitude | **ROBUST** | ~717x–2,578x here vs ~460x–1,780x on ESCI |
| Facet fails 80x floor at medium/large sizes, reverses to a loss at scale | **SHIFTED** | Same qualitative pattern, but the native-loss crossover occurs at ~2,072–2,175 candidates here vs ~9,000–12,000 on ESCI — a real, mechanistically-explained (not just observed) threshold shift |
| Sort fails 80x floor at large sizes | **ROBUST** | 16.6x at 1,103 candidates here vs 16.97x at 1,249 candidates on ESCI (brand-scoped) — closely matching numbers, and mechanistically expected: sort never touches the per-product attribute map, so WANDS' richer map doesn't shift its cost the way it shifts facet's |
| (No ESCI analog) product_class facet — a genuinely real, dedicated-bitmap facet dimension ESCI could never test (`ProductTypeId(0)` sentinel always) | **NOT COMPARABLE** (novel) | Demonstrates the *same mechanism* (dedicated-bitmap `_by_scan` faceting) generalizes to a new field with real, non-sentinel data; no Phase 5 number to compare against |
| Any category/collection/PLP-specific finding at all | **NOT COMPARABLE → now testable** | Phase 5 could not test this at all (ESCI has zero category data); Phase 6A is the first real test — see above |
| Price-range / price-sort workloads | **NOT COMPARABLE** | WANDS has no price field (confirmed absent) |
| True variant-scoped constraints | **NOT COMPARABLE** | WANDS has no parent-ASIN/variant-grouping equivalent |
| Availability gating | **NOT COMPARABLE** | WANDS has no availability/inventory field |
| — | **FALSIFIED: none** | No Phase 5 finding was contradicted or reversed by this dataset |

## Failed / fixed experiments (preserved, not erased)

Two real methodology bugs were found and fixed before any timing claim
was trusted (both documented in full in `docs/experiments/PHASE6A_LOG.md`):
a casing-representation mismatch on `product_class` (the same pattern
Phase 5 found for `brand`), and a facet-bucket-sum used as a stand-in for
true candidate count — a self-repeat, within this same session, of the
exact bug already found and corrected in `PHASE5_DECISION.md` during
Issue #21's repo-normalization pass. Both are recorded, not hidden,
including the specific real discrepancy (a "Rugs" group reporting 554
candidates against a ground truth of 2,002) that caught the second bug.

## Scope decisions (stated explicitly, not silently dropped)

- **Catalog scale beyond WANDS' own 42,994-product ceiling**: not tested;
  no synthetic upsampling was applied. The scale/cardinality axis varied
  here is real group/subtree size (2 to 16,039 products), the same axis
  Phase 5 varied against ESCI's own fixed catalog size.
- **Multiple simultaneously-active filters** (e.g. category AND color as
  two live constraints): not separately measured; the closest tested
  analog is a subtree filter plus a facet. A concrete Phase 6B/6C
  candidate.
- **Price-range/price-sort, true variant-scoped constraints, availability
  gating**: NOT COMPARABLE for this dataset (see table above).
- **Havenask comparison**: explicitly deferred by Issue #23 itself; not
  required to block this issue.

## Unresolved risks

- The attribute-map-size explanation for the facet-crossover shift is
  **plausible, not proven** — no controlled ablation isolated attribute-
  map size as the sole variable (e.g. by re-running WANDS' facet
  benchmark against a stripped-down catalog with only a color attribute,
  matching ESCI's own attribute-map size). A real next-step experiment,
  not assumed true here.
- WANDS' 42,994-product ceiling means the facet-crossover mechanism has
  now been observed at two real scales (ESCI's ~9,000–12,000-candidate
  regime, WANDS' ~2,000-candidate regime) but never beyond ~16,039
  candidates on either dataset — whether the crossover threshold keeps
  shifting predictably (e.g. inversely with attribute-map size) or
  behaves differently at genuinely large scale (100k+ candidates) is
  untested.
- `average_rating` as a business-order-sort substitute is a real,
  disclosed stand-in, not price or a genuine popularity-rank field; any
  future claim about "business order" sort performance should be
  re-validated against a dataset that actually has one.

## What would be built next if scaling up (conditional — see decision)

1. **A controlled attribute-map-size ablation** on WANDS itself (e.g. a
   facet benchmark variant that only carries `color` in the attribute
   map) — the single most valuable next experiment, since it would
   convert the "plausible explanation" above into an isolated, tested
   planner input (attribute-map complexity, alongside candidate
   cardinality) rather than a hypothesis.
2. **The next independent Phase 6 dataset** (per Epic #21's plan:
   Amazon Reviews 2023 if network access is ever granted, or another
   reachable real hierarchical dataset) to further triangulate whether
   the facet-crossover threshold is a function of attribute-map size, or
   some other dataset-specific property not yet identified.
3. **A Havenask baseline**, per Epic #21's Phase 6 plan, once its harness
   is ready — not blocking this issue by its own text.
4. **A genuine multiple-active-filters selectivity sweep**, correlating
   breakpoint movement with "number of active filters" as its own
   physical variable, exactly as Issue #23 asks.

## What should explicitly not be built yet

- **Any facet "fix" for the lower WANDS crossover threshold** — the
  crossover is a measured, explained property of the current
  architecture at this schema's attribute-map complexity, not a defect
  to patch before the ablation above actually isolates the cause.
- **Distributed/sharded serving, cluster coordination, multi-tenancy,
  HA** — unaffected by this phase's results, per CLAUDE.md's standing
  rule for this epic.
- **A production LLM-backed reasoning path in the hot path** —
  unaffected by this phase's results, per CLAUDE.md's hard rule.
- **A claim that WANDS' results generalize to Amazon Reviews 2023 or any
  other unreachable dataset** — this substitution's own trade-offs (no
  price, no variants, smaller ceiling) are a real scope boundary, not a
  small extrapolation.

## What this decision does and does not claim

**Claims**: on the real 42,994-product WANDS catalog and a fresh,
same-environment, fairly-configured Solr baseline, with correctness
verified before every timing claim (41/41 exact matches, the only 2
residual facet-map mismatches traced to the same already-documented
non-bug pattern Phase 5 found) and two real implementation bugs found,
fixed, and disclosed: commerce-native structural filter, subtree-browse,
pagination, and concurrency all **robustly reproduce** Phase 5's ESCI
findings in order of magnitude on an independent, genuinely hierarchical
commerce dataset. Facet and sort both reproduce Phase 5's *qualitative*
breakpoint pattern; sort's specific threshold is itself robust (matching
ESCI closely at a comparable candidate count, consistent with sort never
touching the per-product attribute map), while facet's threshold is real
but shifts substantially lower, with a plausible, stated, not-yet-proven
physical explanation (attribute-map size). Nothing measured here
contradicts or reverses any Phase 5 finding.

**Does not claim**: that this result generalizes to Amazon Reviews 2023
or any other untested dataset; that the attribute-map-size explanation
for the facet-crossover shift is proven (it is a stated hypothesis, not
an isolated finding); that the facet/sort floor violations are fixable
or unfixable at scale (unchanged from Phase 5's own disclosure); that
price, true variant-scoped, or availability-gated workloads have been
tested at all (they have not — WANDS lacks the data); that a
multiple-active-filters selectivity sweep has been performed (it has
not); or that this single substituted dataset satisfies Epic #21's full
Phase 6 cross-validation plan (it is the first of several planned
datasets/engines).
