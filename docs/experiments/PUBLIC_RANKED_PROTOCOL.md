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
