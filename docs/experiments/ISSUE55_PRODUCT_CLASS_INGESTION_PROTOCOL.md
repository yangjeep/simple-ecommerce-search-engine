# Issue #55 Preregistered Protocol — fix WANDS ingestion's `product_class` gaps, measure structural coverage/relevance impact

Committed before this round's fix is implemented.

## 0. What this is testing

This session's own prior work established: (1) native's ranking signal
is not materially worse than Solr's on an identical candidate set
(`p9_e04`'s H1, FALSIFIED at +4.33%); (2) native's structural candidate
sets contain only ~44.6% of a query's truly judged-relevant documents
on average (n=15); (3) `structural_routed`'s own FastPath population has
NDCG@10=0.1611 vs. Solr's 0.4670, while Hybrid (which uses the lexical
delegate's own ranking over a narrowed set) is roughly on par with Solr
(0.3624 vs. 0.3574) — together indicating the gap is a **coverage**
problem in what gets structurally admitted, not a ranking-quality
problem.

A focused investigation (real WANDS data, `compile()`/`indexed_candidates`
source reading) found a concrete, disclosed, verified ingestion-time
data-quality gap in `phase6a_eval::catalog::build_catalog`
(`crates/phase6a-eval/src/catalog.rs:95-109`): WANDS's own `product_class`
field is `None`/empty for 2,852 of 42,994 products (6.64%, confirmed by
direct count against `dataset_cache/wands/catalog.jsonl`) — these fall
back to a sentinel `UNKNOWN_PRODUCT_TYPE` that can never satisfy any real
`ProductType` constraint, even when the same record's `category_leaf`
unambiguously implies a real, otherwise-lexicon-known type (e.g. product
#10018 "parkash bunk bed" has `product_class=None` but is genuinely a
"...Kids Beds" item per its category hierarchy). A further 2,247
products (5.23%) have pipe-delimited multi-class strings (e.g.
`"Stackable Chairs|Dining Chairs"`) ingested verbatim as one opaque,
un-matchable compound string instead of being split.

## 1. Hypothesis

**H0**: fixing these two ingestion gaps (null `product_class` falls back
to the deepest available `category_depth_N` segment already present in
the record; pipe-delimited `product_class` uses its first segment
instead of the whole compound string) recovers real coverage — measured
via `p9_e04`'s own candidate-set relevant-document recall diagnostic and
`p9_e02`'s own `structural_routed`/`FastPath` NDCG@10 — without any
correctness regression (no query stops resolving that used to, no
existing test failure), because 11.86% of the catalog moving from
unmatchable-or-garbled to a real, resolvable product type can only add
candidates, never remove correct ones, and no query text changes.

**H1**: the fix has no measurable effect, because the specific queries
affected are not among (or barely overlap with) the queries this
project's own judged corpus actually exercises, or because WANDS's own
labeling is broad enough (mechanism 4 from the investigation: judgments
spanning multiple genuinely distinct product classes for one query
intent) that recovering exact-class matches does not materially move
NDCG even where coverage improves.

Both outcomes are informative. This is a data-ingestion-quality fix,
not a new architectural mechanism — consistent with this project's own
"ingestion-time profiling... absorbs merchant/category diversity" bias.

## 2. Baseline / dataset / treatment

Baseline: current branch HEAD. Dataset: the same real WANDS catalog +
480 queries + fresh Solr 9.10.1 used throughout this session — **the
Solr `wands_bench` core is built directly from `catalog.jsonl` by a
separate Python script
(`scripts/datasets/solr_index_wands.py`) and is unaffected by this
Rust-side ingestion fix**, so Solr's own numbers are an unchanged,
valid baseline for comparison; only `commerce_core`'s own catalog
(built via `phase6a_eval::catalog::build_catalog`) changes. Treatment:
the two disclosed ingestion fixes above, applied only in
`crates/phase6a-eval/src/catalog.rs` (eval-crate data-adapter code, not
`commerce-core` production code — this is WANDS-specific data cleaning,
not a change to the generic ingestion contract).

## 3. Metrics / gates

- **Correctness**: `cargo test --workspace --all-features` unchanged
  (zero new failures); no existing query that previously resolved a
  `ProductType`/`Category` constraint changes which constraint it
  resolves to (only previously-`UNKNOWN_PRODUCT_TYPE`/
  previously-garbled-compound-string products gain a *new*, real type).
- **Coverage**: `p9_e04`'s own "candidate-set relevant-document recall"
  diagnostic (mean fraction of a query's judged-relevant documents
  present in native's structural candidate set, n=15 `structural_routed`
  queries with candidate sets <=5000) — **KEEP/document as real**: this
  recovers materially (>=5 percentage points, a threshold chosen to be
  clearly distinguishable from run-to-run noise given this metric's own
  prior 0.4460 baseline) from its pre-fix baseline. **Flag as
  insufficient**: no material recovery.
- **Relevance**: `p9_e02`'s own `FastPath`/`structural_routed` NDCG@10
  (real end-to-end `execute_planned`, same methodology as every prior
  Issue #34/#43/#55 checkpoint) — **KEEP/document as real**: FastPath's
  own NDCG@10 improves from its pre-fix baseline (0.1611) without
  regressing `Hybrid`'s or `Punt`'s own NDCG (both untouched by this
  fix's mechanism, expected flat). **Flag as insufficient**: no material
  movement, or a regression anywhere.
- Whole-workload: report `p9_e02`'s own traffic-weighted-overall numbers
  too, per Issue #55's own measurement contract, without expecting a
  large movement given `structural_routed`'s own small (4.375%) real
  traffic share — consistent with, not contradicting, this project's
  own prior findings about that share's ceiling on whole-workload impact.

Repetitions: NDCG/coverage are deterministic given fixed judgments and
ingestion (no repetition needed); no new latency claim is made in this
checkpoint, so no fresh-Solr-restart discipline is required.
