# Public ranked evaluation: preregistered native comparison

Preregistered before executing the new ranked binary. This complements, rather
than changes, the QP-Faith procedural probe. No label/catalog rewrites, tuning,
model calls, or production changes are authorized by favorable or unfavorable
scores.

## Populations

- WANDS: the existing immutable official source/checksums and prepare_wands.py;
  full supplied product catalog and all source query.csv IDs, with label.csv
  human judgments. This is distinct from QP-Faith's procedural motif references.
- ESCI: the existing pinned electronics TRAIN-shard export using
  fetch_esci_electronics.sh and filter_esci_electronics.py. Use all exported
  products/queries. Do not call this the full ESCI catalog, held-out test, or the
  QP-Faith paper's full-catalog experiment. Preserve its existing selection rule;
  do not select by resulting scores. Export row IDs are identifiers when the
  existing export lacks original query IDs.

## Arms and fixed settings

Both arms share exactly the same loaded catalog and lexical delegate/index.

1. Lexical baseline: original unmodified full query, no structural restriction,
   BitmapTantivyDelegate, k=10.
2. Existing compiled pipeline: catalog-only lexicon with min enum frequency=1;
   commerce_core::ir::compile followed by execute_planned with the same catalog,
   native index and delegate. Preserve real FastPath/Hybrid/Punt ranking,
   selectivity threshold 0.05, oversample 20, residual policy None.

This is a within-project ablation, not a Solr/Elasticsearch/production superiority
claim. Keep results for the two datasets separate. Use existing lenient Tantivy
query-parser semantics, but surface diagnostic errors rather than silently
scoring failed parsing/search as zero.

## Outcomes and errors

Fixed primary metrics at k=10: NDCG, Recall and reciprocal rank.
WANDS gains Exact/Partial/Irrelevant = 2/1/0; ESCI E/S/C/I = 1/0.1/0.01/0.
Use linear DCG with those gains; gain>0 is relevant. Unjudged hits gain zero,
with judged-top10 coverage recorded to expose incomplete judgments.

Retain every query and its qrels, both rankings, route, constraints and errors.
Queries with no positive qrel have null metrics and explicit counts. Parser,
search or ID/ordinal failures null both paired metrics and fail the run; do not
drop failures silently or turn them into baseline-favoring/native-favoring zeros.
Means use only positive-qrel, diagnostic-free paired queries, with coverage
counts. Missing-catalog qrels remain in the denominator and are counted.

Record input hashes, engine revision, parameters and per-query results. No
latency comparison is planned; build/first-touch times are not serving evidence.

## Decision discipline

Report deltas and coverage, including negative results. No fixed architecture
promotion threshold is asserted. Do not infer causal benefit from a single
aggregate: separate routing/constraint changes from ranking path changes and
preserve the known WANDS feature-representation mismatch. A failed acquisition,
parser or execution stage is a blocker with logs, not a measured score.

## Amendment v2: duplicate judgments, before any successful score output

Initial run 42698435456 compiled both datasets and passed regression tests, but
stopped during qrel loading before emitting any ranking scores:
WANDS encountered duplicate product judgment 41180; ESCI electronics encountered
B07D377Q6D. Failure artifacts are preserved. No interpretation of model quality
or query selection follows from these input failures.

For the next run, declare these rules before scores:

- Retain every original judgment row in per-query evidence.
- Identical repeated query-product labels are one metric judgment, not multiple
  products. Collapse them while reporting duplicate counts.
- Conflicting labels for the same query-product pair are unresolved input
  diagnostics. Do not select max/min/first/last label. Both paired scores are
  null for that query; preserve raw judgments and continue the workload.
- Conflicts join the diagnostic query count and make the completed run nonzero.
  Report positive-qrel/diagnostic-free subset means only with explicit coverage,
  never as a clean full-workload result.
- Label gains, query text, catalog and ranking settings remain unchanged.

This is an input-integrity amendment, not tuning against ranked results.

## Measured v2 results: REVISE, not a win

Run [42699839852](https://github.com/yangjeep/simple-ecommerce-search-engine/actions/runs/42699839852) executed both complete query workloads. Summaries, input hashes, initial failures and v2 failures are checked in under `artifacts/public_benchmarks/`. Full raw qrels and ranked rows are CI artifacts; their verified archive and queries-file hashes and expiry are in `ranked_artifact_sources.json`. These artifacts expire on 2027-02-07; rerun the pinned workflow afterward. Raw catalogs are not redistributed here.

WANDS: 42,994 catalog products, 480 queries, 465 paired valid scores. Fourteen queries have conflicting original judgments; one further query has no positive judgment. 1,561 identical repeated judgment rows were counted and collapsed. Native versus full-query lexical: nDCG@10 0.663673 vs 0.688746; Recall@10 0.055737 vs 0.058138; MRR@10 0.851702 vs 0.871558. These are descriptive subset means. The declared whole-run gate remains failed. All 465 scores were independently recomputed from retained rows with no discrepancy. No search/parser errors occurred in WANDS.

ESCI electronics TRAIN slice: 2,075 products, 600 exported query rows, 489 paired valid scores. Four queries have execution diagnostics; 110 have no positive judgments (sets overlap). 85 identical repeated judgment rows were collapsed, no conflicting judgments. Native versus lexical: nDCG@10 0.303422 vs 0.309732; Recall@10 0.393426 vs 0.403265; MRR@10 0.315567 vs 0.322314. Again the gate fails and these are only valid-pair means, not full ESCI or held-out results.

WANDS mechanism evidence: `marble` and `driftwood mirror` consume lexical text into ambiguity, then have no hard constraints or residual terms. FastPath returns IDs 0–9 with zero scores: nDCG drops from 1.0 and 0.90035 respectively to zero. This is lexical signal loss, not overfiltering. `decorative wall clocks` and `feather wall art` have attribute/type constraints and no residual text, then zero native hits; the artifacts alone do not distinguish incorrect intent from incompatible catalog representation. These observations motivate separate preregistered ablations, not a post-hoc change to this run.

No semantic fix, qrel relabeling, LLM call, significance test, or whole-system superiority claim is made.

Independent recomputation also verified all 489 ESCI pairs. Its four failures are actual Tantivy query-parser diagnostics in both arms: row 79 treats `delivered:` as a nonexistent field; rows 320 and 332 have malformed leading wildcard/operator syntax; row 399 has an unparsed expression tail. The first three also lack positive judgments: exclusive accounting is 489 scored + 107 no-positive-only + 4 diagnostic rows. No silent parser fallback was used. A concrete negative is `amazon basic mini display port to hdmi cable`, compiled with HDMI as a color constraint plus brand, nDCG 1→0. Sparse judgments/unjudged-zero scoring and combined filtering/text/ranking changes limit causal attribution.
