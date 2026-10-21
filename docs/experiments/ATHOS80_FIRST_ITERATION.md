# Variant-first search + merchandising + supplied personalization: first iteration

## Verdict: REVISE, not an 80/20 result

The experiment asks whether a bounded, variant-first search/merchandising/personalization
slice can eventually cover most small-catalog commerce uses at materially lower
same-traffic hosting cost. Neither weighted 80% feature coverage nor 20% cost has
been demonstrated. PMF, production integration and recommendation profiles are
outside this experiment.

This PR imports the v1.1.1 prototype as [`athos80-eval`](../../crates/athos80-eval/README.md),
without changing existing `commerce-core` or the `/plp` service. It includes the
independent Python scan oracle, safe loopback correctness/resource harness, source
provenance regression and selected historical result summaries. It is not an
Athos-compatible API or production deployment.

## Included behavior

- Same-variant typed filtering, inventory/hide eligibility, explicit product or
  variant output, selected-variant identity and matching-variant count.
- Complete eligible-candidate ranking before product grouping/pagination; exact
  product-distinct or variant counts and self-excluding facets.
- Ordered merchandising groups, scoped rules, eligible-only pins, deterministic
  conflict handling and bounded supplied personalization factors.
- Exact-token, supplied-hit, strict BM25 and literal-token BM25 profiles. Literal
  BM25 treats storefront punctuation as data, not a query DSL; it still requires
  all analyzed tokens. This is not typo, synonym or zero-result fallback.
- Explicit finite numeric bounds and stable ordering; no online model calls.

Personalization here consumes already-supplied factors. Signal generation,
profile storage/fetch/cache, training and tracking are not implemented. Carousel,
bundle and “you may also like” profiles are explicitly excluded.

## Historical validation (2026-10-21)

The pre-PR source checkpoint passed 48 Rust acceptance tests plus one HTTP unit
check, 22 HTTP smoke checks and 353 independent small-fixture checks. Corrected
WANDS replay passed **1,924/1,924** checks: 480 real queries plus one explicitly
synthetic browse case, across base / merch / personal / both.

Pure quality uses a separate 480-query, product-mode, top-10 replay without
filters, offset, stock restriction, merchandising, personalization or synthetic
browse. All returned HTTP 200, but **159/480 queries returned no results (33.125%)**.
Macro NDCG@10 is **0.38637892767840865**, using linear gains Exact=1, Partial=0.5,
Irrelevant/unjudged=0; one zero-IDCG query contributes zero. The 479 scoreable-query
mean is about 0.387186. Duplicate/conflicting judgments are recorded explicitly.
Execution correctness does not establish good recall or relevance parity.

Two descriptive resource configurations used 10 QPS × 60 seconds × 600 requests
per arm, 5 seconds warmup, and 962 preflight requests per arm outside measurement.
All **4,800 measured requests** succeeded without timeout/drop/result mismatch.
However, no configuration had a common qualified four-arm SLO/generator window.

| Workers | Arm | CPU ms/request | RSS median MiB | Scheduled p95/p99 ms | SLO / generator |
|---:|---|---:|---:|---:|---|
| 1 | base | 5.97 | 539.8 | 88.8 / 324.0 | fail / fail |
| 1 | merch | 5.35 | 551.4 | 15.8 / 222.6 | fail / pass |
| 1 | personal | 5.08 | 538.6 | 11.5 / 190.3 | fail / pass |
| 1 | both | 7.55 | 528.2 | 105.2 / 405.1 | fail / fail |
| 4 | base | 5.15 | 595.2 | 27.3 / 120.5 | fail / fail |
| 4 | merch | 5.38 | 602.4 | 11.3 / 27.5 | pass / pass |
| 4 | personal | 11.10 | 579.2 | 178.0 / 525.8 | fail / fail |
| 4 | both | 5.97 | 602.7 | 36.6 / 237.1 | fail / fail |

S1 required scheduled-to-complete p95<50 ms, p99<100 ms, error rate<0.1%,
completion/offered and success/offered≥98%, and zero correctness differences.
At 10 QPS the generator required dispatch-lag p99≤10 ms, max≤100 ms and
client+sampler CPU<0.8 core. Accounting validity is not SLO/capacity qualification.
Memory is total sampled service memory, not incremental or lifecycle peak.
[CSV](../../artifacts/athos80/measurements.csv) retains PSS and further diagnostics.

The shared Linux host was not an exclusive VM and CPU affinity was not pinned.
Full-catalog browse (~42,994 candidates) took roughly 0.32–0.58 seconds, causing
head-of-line blocking with one worker. Four workers reduced immediate follower
latencies, but worker count and harness version both changed, so not all changes
can be causally attributed to workers. A scheduling-epoch defect was corrected;
remaining late jitter was not conclusively attributed to GC, GIL or host effects.
A failed startup produced zero measured rows and was retained, not counted as a
successful run. Safety checks were never relaxed to obtain passing numbers.

## Provenance and limits

WANDS is pinned to `3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5`.
All 42,994 product IDs/titles/descriptions/normalized real attributes were checked
against its public source. One synthetic variant per product is used; synthetic
multi-variant fixtures cover correctness but not real-world variant distributions.
Brands, inventory, rules, supplied factors and uniform traffic are synthetic.
Price zero is a missing-price placeholder, not evidence of real price-sort quality.

An earlier naive TSV split preserved outer/doubled quotes for query IDs 208, 285
and 391. Those strings were wrongly attributed to official decoded query text.
The bundled regression preserves both the historical malformed-derived inputs
and the correct CSV-decoded strings. The corrected full replay was rerun; old
results are not silently relabeled. No query was dropped. The aborted old pilot
contained 35 warmup requests and zero measurement rows.

Historical binary SHA256: `1551492f9a4d2aee3816ff11d9f93683e4543a8662e513e2c33df1160c992ae3`.
Historical corrected workload: `392fb42fdf557c358b6c055f2e91abfece481eabe2b85179c3ffdf71209b1e9e`.
Historical goldens: `7e8d91e008cd3263f85a98a6087d4059fe916db5cad035955b5a2a2e4f3d753d`.

The measured workspace also contained older PLP-ablation additions, disabled in
this new crate's standard index build. This PR deliberately does **not** import
those unrelated modifications. Therefore a newly built PR binary is not claimed
to be byte-identical to the historical measured executable. Regenerate correctness
and binary-bound goldens before any new resource run; do not transfer historical
cost numbers as current-commit performance evidence.

This PR retains summary JSON/CSV and runnable harness code, not the large raw
response/timing/trace archive, normalized catalog, downloaded labels or executable.
The retained summaries are historical observations, not a fresh-clone independent
re-score of the archived experiment. Archived evidence ZIP: `d7fc94409baa75c5c03255a4fdc08ef4fcfd9e6a50ef5aba8ecc5299c52bc43c`.
Source revision archive: `454acd059f5ce6d9edbc5e6f8add1a0113ca7a2370abd28226b2eb0bc26b274e`.

Native lexical caches and the additional Tantivy index both reside in memory.
The implementation is an all-candidate correctness baseline, not a bounded-top-K
optimized pipeline. No old PLP-only savings, old product-doc NDCG, multi-tenant
capacity or other-engine comparison transfers to it.

ETL, catalog updates/rebuild overlap, external profile services, tracking, control
plane and operational costs were not measured and must not be treated as zero.
CDN is excluded from the intended comparison. The existing complete-system cost
baseline is unknown. No lowest-VM, maximum-QPS or 20%-total-cost claim is supported.

## Reproduce and extend

See [`scripts/athos80_validation/README.md`](../../scripts/athos80_validation/README.md)
for offline tests, synthetic HTTP validation and WANDS preparation. Future fair
comparisons must freeze equivalent semantics, quality, SLO, traffic and all required
service resources. This PR does not automatically start a new experiment or add a
production integration.
