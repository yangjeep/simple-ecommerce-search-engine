# Phase 2 Experiment Log

Append-only, continuing the format established by `docs/experiments/LOG.md`
(Phase 0) and `docs/experiments/ROUND1_LOG.md` (Round 1, Issue #5). Phase 2
(Issue #6) executes the NARROW THE PRODUCT branch selected in
`ROUND1_DECISION_TREE.md`: keep the typed structural/facet retrieval core
and the ambiguity-preserving compiler, delegate lexical retrieval and
ranking to an embedded Tantivy index rather than building that from
scratch (`docs/adr/0008-narrow-to-structural-planning-layer.md`). If
evidence here contradicts an earlier entry (from any log), a new entry
records it — nothing in any experiment log is rewritten.

Same evidence-class/independence framing as `ROUND1_LOG.md`: **Evidence
class** (`real`/`synthetic`/`hand-authored`) and **Independence** (was the
query set built independently of the thing being measured) are required
per entry.

---

## P2-E01 — Does an embedded Tantivy index recover Solr's real relevance numbers?

**Evidence class**: real (same 1,215,854-product ESCI catalog and
22,458-query real human-judgment set used throughout Round 1).
**Independence**: yes — same real, third-party ESCI test-split judgments
as every Round 1 relevance measurement; Tantivy's scoring is unrelated to
and independent of both the query set and the judgment set.

**Question**: ADR 0008's central, falsifiable bet is that delegating
lexical retrieval/ranking to Tantivy — rather than continuing to grow
`commerce_core`'s own substring/token matching into a ranking engine
(R1-E07's finding: retrieval is cheap to build, ranking is the actual
gap) — recovers real relevance quality. Does it, measured against the
exact same real catalog and real ESCI judgments Solr was measured
against in R1-E04?

**Hypothesis**: an embedded Tantivy index, using its default BM25
scoring and a competently-configured (not hand-tuned, not
hand-crippled) `TEXT` field over the same title+description+bullets
content Solr's `all_text` copy field covered, will produce NDCG@10,
Recall@10, MRR, and zero-result-rate numbers close to R1-E04's Solr
baseline — because both are, at their core, mature Lucene-lineage BM25
implementations over comparable indexed content, not because Tantivy is
expected to be dramatically better or worse than a system built on the
same underlying retrieval theory.

**Decision threshold**: per ADR 0008's own consequence, "if it does not
[recover relevance close to Solr's], this ADR's decision should be
revisited before further integration work proceeds." No specific
numeric tolerance was fixed in advance beyond "close" — this is
deliberately not a hairsplitting threshold, since the real question is
qualitative (does delegating to a mature engine actually work in this
environment, in-process, with a competent-but-untuned configuration) not
whether Tantivy beats Solr by some margin.

**Implementation**  
New crate `crates/phase2-eval` (depends on `commerce-core` and
`round1-eval` path-wise; reuses `round1_eval::data`'s existing real-data
JSONL loaders read-only — no changes to `commerce_core` or `round1-eval`
from this crate). `crates/phase2-eval/src/bin/tantivy_relevance_eval.rs`:
schema `id: STRING | STORED` (real ASIN, for joining back to judgments),
`all_text: TEXT` (title + description + bullets concatenated, matching
what Solr's `all_text` copy field covered in R1-E04); Tantivy's default
tokenizer (lowercasing + simple alphanumeric tokenization) and default
BM25 similarity — no synonym lists, no custom scoring, no field boosts,
matching R1-E04's own "standard, competently configured... not
hand-crippled" standard for the Solr side. Queries parsed via
`QueryParser::parse_query_lenient` (tolerant of the same special
characters — `#`, `$`, `"` — that broke Solr's *default* Lucene parser
in R1-E04, forcing that experiment to edismax; Tantivy's lenient parser
is the equivalent competent choice here, not a workaround unique to this
experiment). Metric definitions (graded relevance E=3/S=2/C=1/I=0,
NDCG@10 with `log2(i+2)` discount and an ideal ranking built from *all*
of that query's judged grades, Recall@10 against Exact+Substitute, MRR,
zero-result rate) are copied exactly from `scripts/round1/solr_bench.py`
so the two numbers are directly comparable, not just similarly named.
Evaluated against the **full** 22,458-query set (every query with at
least one Exact/Substitute judgment) rather than R1-E04's 1,000-query
`random.Random(seed=7)` sample — Python's Mersenne-Twister-seeded
`random.sample` cannot be reproduced bit-for-bit in Rust, and the full
set is strictly stronger evidence once an in-process query makes it
computationally tractable (it did: ~4 seconds for all 22,458 queries).

**Results** (same environment as R1-E01 through R1-E07: 4 vCPU Intel
Xeon @2.80GHz, 15 GiB RAM, Linux 6.18.5; single run, deterministic
pipeline):

```
Indexing (1,215,854 real products):
  Tantivy (in-process, default writer heap 512MB): 19.9s
  (R1-E04 Solr baseline, HTTP bulk JSON, 5k-doc batches: 321.1s --
   not apples-to-apples, includes network+JSON overhead Tantivy's
   in-process add_document call doesn't pay)

Index footprint:
  Tantivy on-disk index: 565,724,910 bytes (565.7 MB)
  (R1-E04 Solr baseline: 1.9 GB on-disk -- Tantivy's is smaller, though
   Solr's schema also stored separate brand/color fields this one
   doesn't)

Relevance (FULL 22,458-query real set, all queries with >=1 real
Exact/Substitute judgment):
  zero-result rate: 0.6% (125/22,458)
  NDCG@10:          0.3033
  Recall@10:        0.1801
  MRR:              0.4838

  (R1-E04 Solr baseline, 1,000-query sample:
   zero-result rate=0.2%, NDCG@10=0.3052, Recall@10=0.1811, MRR=0.4910)

Query latency (in-process, n=22,458):
  p50=1.0905ms  p95=6.2243ms  p99=22.2627ms
  (R1-E04 Solr baseline, Python HTTP round-trip: p50=1486us;
   Solr's own server-reported QTime: 0-5ms)
```

**Interpretation**

**Confirmed, decisively and closely.** NDCG@10 (0.3033 vs. 0.3052),
Recall@10 (0.1801 vs. 0.1811), and MRR (0.4838 vs. 0.4910) are all
within roughly 0.6-1.5% relative of Solr's numbers — for two
independently-configured systems evaluated on different query samples
(the full 22,458-query set here vs. R1-E04's 1,000-query random sample),
this is a striking, not merely directional, confirmation. Zero-result
rate is slightly higher (0.6% vs. 0.2%) but both are low; the difference
is plausibly explained by evaluating the full query set here (including
whatever long-tail queries a 1,000-query random sample happened not to
include) rather than a like-for-like discrepancy. This is exactly the
outcome ADR 0008 bet on: **a competently-configured, un-tuned, default
BM25 implementation recovers real relevance quality close to another
competently-configured, un-tuned, default BM25 implementation**, because
both are built on the same well-understood retrieval theory a mature
engine already implements correctly. It is direct, positive evidence
that R1-E07's conclusion ("ranking, not retrieval, is the differentiator,
and it's exactly what a mature engine already provides") was correctly
diagnosed, not merely plausible.

The index footprint result is a secondary, additional point in Tantivy's
favor: 565.7 MB on disk vs. Solr's 1.9 GB for a comparable (if not
identical-schema) corpus, and indexing completed in 19.9s in-process vs.
Solr's 321.1s over HTTP (not a fair latency comparison given the
HTTP/JSON overhead difference, but a real, favorable data point for an
embedded, in-process architecture with no separate server process to
run). Query latency (p50=1.09ms) is in the same rough range as Solr's
HTTP-measured figure (1.49ms) despite paying zero network/serialization
overhead — a fully fair comparison isn't possible here either (this
number includes real BM25 scoring + top-10 retrieval + stored-field
lookup, work Solr's number also includes), but it is not a red flag: a
production system embedding this index directly (no HTTP layer between
the query planner and the lexical engine at all) would plausibly do
better still, though that specific claim is not measured by this
experiment and should not be asserted beyond what was actually run.

**What this confirms and what it does not**: this experiment validates
that *delegating* lexical retrieval/ranking to Tantivy is viable and
recovers real relevance quality — the specific, falsifiable bet ADR 0008
made. It does **not** yet validate the *integration* of this delegated
path with `commerce_core`'s structural/facet layer (how the two compose
at query time — e.g. does a genuinely selective structural predicate
narrow the candidate set *before* Tantivy scores it, or does Tantivy
receive the full free-text query independently and results get merged —
this is explicitly Issue #6 priority 5, not addressed here). It also
does not validate the canonicalization stage (Issue #6 priority 2) or
the precision-aware promotion gate (priority 3), both still open.

**Caveats**: single run (deterministic index/query pipeline — no
variance to characterize, matching R1-E02's precedent for deterministic
pipelines). Tantivy's default tokenizer (lowercase + simple alphanumeric
split) is not identical to Solr's `text_general` analyzer chain
(`StandardTokenizer` + `LowerCaseFilter` + `StopFilter`) — "competent
defaults on both sides," not "identical configuration," is the fair
framing, matching R1-E04's own caveat about Solr's schema not being
hand-tuned. The full-22,458-query evaluation here and R1-E04's
1,000-query sample are not the *same* queries, so this is a comparison
of two representative measurements on overlapping-but-not-identical
samples of the same real query population, not a paired test — flagged
per this log's own "never compare results generated from different
query sets without labeling the comparison invalid or adjusted" rule
(inherited from `docs/EXPERIMENT_LOOP.md`). Index-size comparison is
schema-asymmetric (Solr's schema also stores separate filterable
brand/color fields this experiment's schema does not) — a real but
partial explanation for some of the size difference, not a fully
controlled comparison.

**Regression check**: none yet — `phase2-eval` is a new, standalone
experiment crate with no test suite of its own (mirrors `round1-eval`'s
precedent: an experiment harness over `commerce_core`/`round1_eval`,
neither of which this entry modifies). `commerce-core`'s 36 tests remain
green throughout (unaffected — `phase2-eval` only depends on it,
read-only).

**Next question**: ADR 0008's central bet holds — Issue #6's remaining
priorities (canonicalization stage, precision-aware promotion gate,
compiler fixes for R1-E03's disjunction/negation bugs, and the
structural-plus-delegated-Tantivy integration design) are now
appropriately unblocked to proceed, rather than needing the decision
tree revisited first.
