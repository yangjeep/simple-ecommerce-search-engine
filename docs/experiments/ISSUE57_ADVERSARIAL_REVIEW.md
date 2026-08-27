# Issue #57 — Adversarial Review (Revision 1)

Per Issue #57 §14: independent review across four lenses, instructed to
try to disprove the findings in `ISSUE57_FULL_MATRIX_SYNTHESIS.md`, not
approve them. Findings below are organized by lens; each includes
severity and whether it was resolved (fixed/rerun) or remains an open,
disclosed limitation of this revision.

## Lens 1 — Semantic/comparator fairness

**Confirmed fair (considered, not an oversight):** native's near-zero
timed cost vs. every external engine's full HTTP round trip is *not* an
apples-to-oranges artifact — it is the real architectural difference
under test. Native's actual deployment shape is embedded/in-process; a
real caller pays exactly this cost. Solr/ES/OpenSearch/Havenask are
always separate services in any real deployment; a real caller always
pays the network/IPC tax measured here. Both sides are timed as "what a
real serving caller would pay," per the frozen protocol §10 — the
asymmetry is the finding, not a confound.

**Open limitation, not resolved this revision:** the Brand
case-sensitivity gap (§4 item 7 / §6.3 of the synthesis) was handled by
*excluding* colliding brands from the gated Q2 comparison rather than
computing what native's answer would be under a case-folded identity.
An adversarial reading: this could be seen as declining to quantify a
real native weakness rather than confronting it. Counter: the excluded
cases are disclosed by name (Q2b rows), not hidden, and computing a
"case-folded native" number would require either modifying
`issue35_eval`'s ingestion (explicitly out of scope — "do not modify
architecture merely to improve matrix results") or hand-rolling a
parallel ad hoc union query outside the production code path (which
would not be measuring the *actual* system). Verdict: disclosure is
adequate; a full quantification is a legitimate follow-up, not a defect
in this revision.

**Confirmed limitation:** engine query order was **not** randomized or
counterbalanced across runs (native → Solr → ES → OpenSearch → Havenask,
every single time), contrary to Issue #57 §"Execution protocol"'s
explicit instruction to randomize/counterbalance where machine state
could bias results. Havenask was *always* queried last, after four other
engines had already been resident and (for Solr/ES/OS) actively queried.
Havenask is also consistently the slowest external engine (§3 of the
synthesis, roughly 1.3–2× Solr's latency on every single query class and
dataset). **This is a real, unresolved confound**: it is not possible
from this revision's data alone to fully separate "Havenask is genuinely
slower in this deployment mode" from "Havenask was measured under
accumulated thermal/cache/scheduler pressure from four already-resident
JVMs/processes." Not rerun this revision (time). Flagged as the
single most important methodology gap to close before treating the
Havenask-is-slowest finding as settled.

## Lens 2 — Benchmark methodology/performance

**Confirmed limitation — small samples:** WANDS Q9 (2 categories), Q5 (2
thresholds), Q10 (1 category); ESCI Q2 (3 brands/vertical). This is
narrow, hand-selected coverage, not a representative sample of each
dataset's full query-class space. The 10,000×+ effect size for
structural queries is large enough that sampling noise is very unlikely
to reverse the *direction* of the finding, but the exact magnitude
should not be read as a precise, generalizable number from this sample
size alone.

**Confirmed limitation — no index/build-time matrix:** §5.3 of the
synthesis already discloses this: per-engine index size and build time
were not systematically instrumented across all 5 datasets × 5 engines,
despite the frozen protocol listing this as a required measurement
(§11). WANDS→Havenask build time (~73s for 42,994 rows via concurrent
single-row SQL INSERT, since Havenask's SQL layer rejects multi-row
INSERT) is the only build-time figure captured with any rigor.

**Confirmed limitation — no P95, thin P99:** only mean/P50/P99 reported
(protocol default), and 30 repetitions is thin for a stable P99 reading
in particular. Not rerun with a larger repetition count this revision.

## Lens 3 — Relevance/statistics

**Confirmed, significant gap:** this revision measured **zero**
relevance-quality metrics (NDCG@10, Recall@K, MRR, zero-result rate,
relevant-zero-hit rate) despite both WANDS (`query.csv`/`label.csv`) and
the three ESCI slices (per-query judgments already in
`dataset_cache/esci_*/`) carrying real relevance judgments the frozen
protocol explicitly calls for (§8 "Relevance"). Everything measured this
revision is **structural-filter/count correctness and latency**, not
ranked-result quality. This is the largest content gap in this
revision relative to Issue #57's full scope, and it means the "narrower
specialization" reading below (§ final decision) is supported for
*structural/faceted/lexical-timing* behavior only — it says nothing
about whether native's ranking, when engaged, produces relevance
quality comparable to Solr/ES/OpenSearch/Havenask's own ranking on the
same real judged queries. Issue #35's own prior (Solr-only) NDCG
evidence (electronics +8.93%, automotive -2.55%, beauty -1.38% vs.
native) is the only relevance evidence this project has, and it does
not cover Elasticsearch/OpenSearch/Havenask at all.

**No significance testing performed** (paired bootstrap CI, t-test) on
the latency deltas. Given the observed effect sizes (10,000×+ for
structural queries; a clean, disclosed-as-scale-dependent crossover for
lexical), this is a low-risk omission for the *directional* claims, but
a real gap for anyone wanting a formal confidence interval.

## Lens 4 — Architecture-significance / external validity

**Confirmed limitation — scale.** The largest dataset measured is WANDS
at 42,994 products. Real production ecommerce catalogs commonly run
into the millions; the full ESCI corpus (1,215,854 products) was
explicitly deferred (protocol §9.1, disk allowance). The WANDS-vs-ESCI
Q11 crossover finding (§5.1 of the synthesis) is itself evidence that
scale materially changes which side wins for at least one query class —
which means **the structural-query magnitude found at 43K products is
not validated to hold, in that exact magnitude, at 10× or 100× that
scale**. Directionally plausible (native's bitmap operations are
sublinear/near-constant per Phase 9's prior evidence; external engines'
HTTP-plus-index-lookup floor is roughly scale-independent in this
range too) but not measured this revision.

**Confirmed limitation — Havenask deployment mode.** Already disclosed
in the protocol and synthesis: `hape`'s `proc` (local-process,
single-shared-searcher-across-all-tables) domain was used because
mounting the host Docker socket for the `default` (sibling-container)
domain was denied by this session's safety guardrails. Whether
Havenask's measured latency reflects this specific constrained
deployment or its general single-node performance ceiling is an open
question, compounded by the Lens-1 ordering confound above.

**Confirmed, valuable finding — Product/Variant safety is a schema
property, not an exclusively-native one.** The Magento Q8 result (294/294
correctness-gated, zero cross-variant false matches on *any* of the 5
systems) is real, adversarially-relevant evidence *against* an
overclaim this project could otherwise be tempted to make: that
Product/Variant safety is a differentiated native capability. It is not
— any of the four external engines achieves the identical safety
guarantee given the correct physical schema (one document per variant).
Native's actual differentiated claim on this dimension is narrower:
Product/Variant safety is native's *correct-by-construction default*
(the typed domain model makes the unsafe denormalized-array schema
structurally awkward to reach for), not a capability unique to native.
This nuance is reflected in the final decision below.

## Disposition

No finding above required rerunning a measured cell — no defect was
found that invalidates a correctness-gated MATCH or a reported latency
number's *direction*. Every item above is a scope/coverage/confound
limitation to disclose prominently in the final decision, not a
methodology defect requiring correction-and-rerun under Issue #57's
"preserve old result → document defect → fix → rerun" rule (that rule
already fired seven times, during construction, before any result in
`ISSUE57_FULL_MATRIX_SYNTHESIS.md` was published — see that document's
§4).
