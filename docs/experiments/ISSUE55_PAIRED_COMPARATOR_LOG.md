# Issue #55 Experiment Log — checkpoint-14 paired comparator freeze (Priority 1A)

Decision: `docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`.

## I55-PAIR-E00 — cohort confirmed identical; Solr comparator was not; root cause found and fixed; corrected picture is -25.05% -> -20.49%, not +5.37%

**Trigger**: the governing task's Priority 1A ("cleanly resolve
checkpoint 14 before more optimization") — checkpoint 14's own
before/after pair showed Solr's NDCG moving (0.3939 -> 0.3455) even
though the native treatment should not itself change Solr's ranking.
Required: freeze the exact structural-routed query IDs; verify the
before/after cohort is literally identical, not merely same count;
capture/freeze Solr results from one controlled state; evaluate native
baseline/treatment against the same frozen Solr results and judgments;
report per-query paired deltas; separate FastPath/Hybrid; explain any
historical Solr ranking drift; reproduce enough times to rule out
ordering/index/state artifacts.

**Infrastructure built** (both additive, no production-behavior
change): `commerce_core::cold_start::compile_lexicon_with_product_type_hyponyms`
(a bool toggle over the existing hard-wired hyponym expansion, needed
because there was no other way to reconstruct the pre-checkpoint-14
lexicon from current code without editing production source) and
`crates/issue55-eval/src/bin/i55_e14_paired_comparator_freeze.rs`
(the paired experiment itself). Two new `commerce-core` unit tests
prove the toggle's `true` path is byte-identical to `compile_lexicon`.

**Protocol**: one catalog/native-index, two lexicons (baseline =
hyponyms off, treatment = hyponyms on) from the identical
`CatalogProfile`. Every WANDS query with judgments compiled and run
under both. Cohort membership (structural-routed: FastPath+Hybrid)
frozen per treatment and compared as exact query-ID sets, not counts.
Every query structural-routed under either treatment gets Solr fired
under BOTH the baseline-compiled and treatment-compiled `(q, fq)`, 5
times each, same run, same live Solr core
(`/home/user/solr_setup/solr-9.10.1`, core `wands_bench`, 42,994
products — the project's existing local install, started via `bin/solr
start -p 8983 --force`, doc count verified before running).

**Results** (raw: `docs/research/artifacts/i55_paired_comparator_freeze/run1.txt`,
`run2.txt`, `run3.txt` — byte-identical across 3 independent runs):

```
cohort freeze:
  baseline structural_routed:  n=21, IDs {7,14,23,79,83,126,160,166,218,224,225,240,241,252,256,295,387,437,440,461,476}
  treatment structural_routed: n=21, IDENTICAL set (0 in either direction only)
  every query's FastPath-vs-Hybrid routing also unchanged across treatments

Solr run-to-run variance (5x identical query text): stdev = 0.000000 both variants
  -> Solr is fully deterministic in this environment; rules out "historical
     Solr ranking drift" (JVM/cache variance) as the explanation

compiled Solr (q, fq) itself DIFFERS between baseline and treatment for
  15/21 queries (71.4%) -- confirms the stated root-cause hypothesis:
  ProductTypeAny changes residual_lexical/constraints, which feeds
  straight into Solr's own query construction

ROOT CAUSE (mechanistically confirmed, not inferred): p9_e02_wands_physical_advantage.rs's
  wands_solr_query_for had NO match arm for StructuralConstraint::ProductTypeAny
  (only Category/ProductType/Enum, falling through `_ => {}` for everything
  else) -- every query where native resolved to ProductTypeAny sent Solr a
  query with NO product-type filter at all, asymmetrically weakening Solr's
  side of exactly the treatment's own cohort.

Fixed p9_e02_wands_physical_advantage.rs (added the missing arm: OR-of-regex
  across every id in the group, same construction as the existing single-id
  ProductType arm) and reran it independently -- reproduces this experiment's
  own numbers exactly:
    native NDCG@10=0.3641, solr NDCG@10=0.4579, relative gap=-20.49%
    (docs/research/artifacts/i55_paired_comparator_freeze/p9_e02_after_productypeany_fq_fix.txt)

CORRECTED aggregate:
  baseline (hyponyms OFF): n=21, native=0.2953, solr=0.3939, gap=-25.05%  (byte-identical to checkpoint 14's own recorded baseline)
  treatment (hyponyms ON, FAIR comparator): n=21, native=0.3641, solr=0.4579, gap=-20.49%
  checkpoint 14's own reported +5.37% does NOT survive a fair comparator --
  the corrected reading is a real, substantial narrowing (-25.05% -> -20.49%,
  ~4.6pp) but not a reversal into a native win.

FastPath vs Hybrid split (treatment routing, fair comparator):
  FastPath: n=7,  native=0.1583, solr=0.4670, gap=-66.11%  (native materially worse)
  Hybrid:   n=14, native=0.4670, solr=0.4533, gap=+3.02%   (native roughly at parity/ahead)
  CONFIRMS the governing task's own named hypothesis: "structural anchors +
  lexical residual are useful; forcing complete structural execution may not
  be." n=7 for FastPath is small -- directionally confirmed, not proven at
  this sample size.

qualitative sample (5 largest |native NDCG delta|): shows the mechanism
  moves BOTH native and the fairly-filtered Solr together for several
  queries (e.g. query 14 "beds that have leds": native -0.2495, solr
  -0.1416; query 256 "high weight capacity bunk beds": native +1.0000,
  solr +0.6422) -- expected once Solr is given a comparably broadened
  filter, not evidence of a new problem.
```

**Reproducibility**: 3 independent runs of the paired-freeze binary,
`diff`-clean (byte-identical). Direct zero-variance measurement (above)
independently confirms no ordering/index/state artifact.

**Adversarial review**: see decision doc's own section — checked the
`ProductTypeAny` fq translation is the natural symmetric extension of
the existing arm (not tuned to a target number); checked the baseline
reproduction is byte-identical to checkpoint 14 before trusting the
treatment-side correction; checked the fix's correctness via two
independently written binaries agreeing; disclosed but did not chase a
real latency-ratio side effect (17.98x vs. checkpoint 14's 1.19x, from
the more expensive OR-of-regex Solr query) since it is outside this
checkpoint's relevance-focused scope.

**Decision**: KEEP the new infrastructure and the `ProductTypeAny` fq
fix. REVISE checkpoint 14's `structural_routed` reversal claim (dated
addendum appended to `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`) — the
`ProductTypeAny` mechanism's own KEEP verdict is unaffected.

**Next selected experiment**: replicate the FastPath-worse/Hybrid-better
split at larger sample size and/or a second structurally-rich vertical,
per Priority 2's semantic-promotion work and the issue's own next-highest-
information-question discipline.
