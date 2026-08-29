# Issue #57 — Dataset Capability / Provenance Matrix

Companion to `FULL_MATRIX_PROTOCOL.md`. For each dataset in the frozen
candidate set: what fields/ground truth it actually has on disk this
session, and which Q1–Q17 workload classes it legitimately supports.
`N/A` always carries a structural reason — never a silent gap.

Q1 exact SKU/identifier · Q2 Brand+ProductType · Q3 ProductType+enum ·
Q4 multi-attribute conjunction · Q5 numeric/range+structure · Q6 price ·
Q7 availability/inventory · Q8 same-variant conjunction ·
Q9 taxonomy/category · Q10 facets/PLP · Q11 lexical-first ·
Q12 structural anchor+lexical residual · Q13 ambiguous/Punt ·
Q14 technical identifier tokens · Q15 long-tail/noisy · Q16 multilingual ·
Q17 behavioral/popularity

## WANDS

- Fields on disk: `id, title, description, product_class, category_leaf,
  category_depth_1..6, color, style, primarymaterial, material, shape,
  rating_count, average_rating, review_count` (`dataset_cache/wands/catalog.jsonl`,
  42,994 rows) + `query.csv`/`label.csv` (query-product relevance labels,
  3-point scale).
- No price field (confirmed absent — carried over from prior
  `PHASE6A_LOG.md` acquisition manifest, re-confirmed this session by
  schema scan: zero `price`-named keys in the JSONL).
- No Brand field, no inventory/availability field, no identifier/SKU
  field distinct from the internal `id`, no multilingual content.
- Supports: **Q3** (product_class+enum), **Q4** (multi-attribute:
  color+style+material+shape), **Q9** (6-level category hierarchy),
  **Q10** (facet/PLP — this dataset's primary intended use, per
  `PHASE6A_LOG.md`), **Q11/Q12** (title/description free text +
  structural filters), **Q13** (ambiguous queries exist in `query.csv`).
- N/A: **Q2** (no Brand field), **Q6** (no price field), **Q7** (no
  inventory field), **Q8** (WANDS has no Product/Variant distinction —
  every row is a standalone listing), **Q14** (no technical
  identifier-style tokens observed in a schema scan), **Q16** (English
  only).

## ESCI electronics / automotive / beauty slices

- Fields: Amazon-catalog-derived product records + up to 600 real
  queries/vertical with ESCI's 4-point relevance grade
  (`dataset_cache/esci_{electronics,automotive,beauty}/`). Per
  `ISSUE35_ESCI_*_DECISION.md`: electronics 2,075 products/59
  brand-constrained queries of 600 total, automotive 1,056 products/37
  brand-constrained, beauty 2,093 products/46 brand-constrained.
- Has Brand (unlike WANDS) — this is the dataset class that exercises
  **Q2** (Brand+ProductType) and the brand-collision correctness checks
  already load-bearing in Issue #35's decision records.
- No price/inventory fields carried into the eval harness (ESCI's raw
  metadata has some commercial fields but they were not part of Issue
  #35's ingested schema — re-verify before claiming Q6/Q7 in a later
  revision rather than assuming).
- No Product/Variant distinction (flat product records, same limitation
  as WANDS) → **Q8 N/A**.
- Supports: **Q2** (the dataset's headline capability), **Q3/Q4**
  (product-type + attribute text), **Q9** (ESCI category strings, coarser
  than WANDS's 6-level hierarchy), **Q11/Q12/Q13** (real query relevance
  judgments — NDCG/Recall/MRR all computable, already computed in
  `ISSUE35_ESCI_*_DECISION.md`), **Q15** (marketplace-noisy real Amazon
  listings).
- N/A: **Q5/Q6** (no verified numeric/price field this revision), **Q7**
  (no inventory field), **Q8** (no Product/Variant), **Q14** (not
  systematically present — electronics has some model-number-like tokens
  in titles but no dedicated identifier field/ground truth), **Q16**
  (English-only slices; ESCI's own dataset has JP/ES locales not pulled
  into these three slices).

## ESCI full corpus (1.2M products)

- **DEFERRED this revision** (§9.1 of `FULL_MATRIX_PROTOCOL.md` —
  disk-allowance reason, not a capability gap). Prior manifests
  (`p3e16_finegrained_frontier.yaml` etc.) record 1,215,854 products /
  22,458 queries were previously ingested in an earlier session; schema
  is a superset of the three slices above. Capability classes would be
  identical to the slices, at larger scale (relevant for Q10's
  crossover-scale characterization specifically, which the slices are
  individually too small to stress-test).

## Magento configurable sample

- Fields: real Magento configurable-product export
  (`dataset_cache/magento_configurable/catalog.jsonl`, 22 rows — a small
  demo/sample catalog, not a full store export) — genuine
  **Product → Variant** parent/child structure (configurable product +
  simple-product variants with size/color-style options), the one
  dataset in this matrix with real Product/Variant identity.
- Supports: **Q8** (same-variant conjunction — this dataset's unique
  contribution to the matrix; no other frozen dataset can test this),
  correctness-only (too small for Q1/Q9/Q10 statistical claims — see
  §9.3 of the protocol).
- N/A (too small to populate meaningfully / not present in schema):
  Q5/Q6/Q7/Q14/Q15/Q16/Q17.

## Retailrocket

- Fields: `events.csv` (2.76M real view/addtocart/transaction events),
  `item_properties_part{1,2}.csv` (item property key-value log, sparse,
  not a clean commerce schema), `category_tree.csv`. **No query text, no
  relevance judgments, no product titles/descriptions in a directly
  usable form.**
- Supports: **Q17 only** — traffic/popularity/session-frequency
  weighting for whole-workload economics (§10 of the governing issue).
  This is real, valuable behavioral evidence but it is a workload-shape
  input, not a retrieval-relevance dataset.
- N/A (no ground truth exists, not merely unmeasured): Q1–Q16 as
  retrieval/relevance classes. Using it for those would mean inventing
  ground truth, which CLAUDE.md and Issue #57 both explicitly forbid.
