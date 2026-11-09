# QP-Faith public alignment: replay and native constraint audit

**Current status:** cached public replay and the real native WANDS constraint
audit have run successfully. The native result is mixed, not an architecture
pass: see the measured continuation at the end. Ranked WANDS/ESCI retrieval
and the LLM-installed merchant-context arm remain unrun.

## Question and boundary

Can this project first reproduce an external query-planner failure metric before
using it to evaluate its own deterministic compiler? Hypothesis: the frozen R1.2
prompt-ladder outputs reproduce every published cell with the official scorer.

This is **stage 1, a cached public-artifact replay**, not a completed current-system
benchmark, fresh model replication, retrieval experiment, or architecture verdict.
No serving code changes and no paid/model API calls are needed.

Source: [QP-Faith](https://github.com/rsrijith/qp-faith),
[paper](https://doi.org/10.1016/j.ipm.2026.105174),
[paper's archive](https://doi.org/10.5281/zenodo.22701132).
The lock pins GitHub commit 5c82a21e7904bec61c11be21d9cf7d60bd440d23,
not a claim that this later GitHub commit is byte-identical to the Zenodo snapshot.
Git blob hashes pin all 39 consumed files. Code is MIT; the upstream derived
queries/outputs are for research use. Underlying catalogs retain their owners'
terms. Upstream currently describes ESCI as CC BY-NC-SA 4.0; do not assume the
older project scripts' Apache-2.0 description grants dataset redistribution rights.

## Frozen workload and scorer

- 73 procedural WANDS-derived queries: 55 A_entity motifs, 18 B_attr queries.
- 4 conditions × 9 planner tags, 2,628 persisted rows. No selection by score.
- The R1.2 score uses only the 55 A_entity rows per cell, each with empty known_slots.
- Preserve original query text, family, labels and plans. No synonyms,
  reannotation, query regeneration or catalog-dependent re-selection.
- Use upstream score_conditions.py unchanged. Its emitted predicate treats null
  and placeholder strings as absent, and stringifies other values. Non-object
  plans count as no emission upstream; report their counts rather than interpret
  them as safe behavior. This is replication, not endorsement of those semantics.
- Require exact query coverage, unique query identities, matching family/labels
  and condition, valid JSON, and matching source hashes before scoring.
- Compare every emission rate, category rate and denominator with the published
  JSON, then also run the entire official scorer in a temporary directory and
  compare its complete JSON output. Any mismatch fails the command.

The aggressive expansion condition, mitigation arms, organic WANDS and 1,162-motif
replication are **not** this R1.2 four-condition matrix. Do not describe the replay
as reproducing all headline paper numbers.

## Reproduce (Python 3.12, standard library only)

From repository root:

```sh
python scripts/public_benchmarks/qp_faith.py \
  --source dataset_cache/qp-faith-public --fetch \
  --output artifacts/public_benchmarks/qp_faith_replay.json
python -m unittest discover -s tests/public_benchmarks -v
```

Omit --fetch once the 39 source files are present for a fully offline run.
Downloads are immutable public raw GitHub URLs checked against the manifest.
Existing files with mismatching hashes are rejected, never overwritten.
No planner runners, API clients, model weights, catalogs or retrieval caches are
downloaded or executed. Raw source artifacts remain outside Git; the small
derived report is committed.

## Stage 2: original integration contract (implementation continuation below)

Before claiming system alignment, implement an exporter around the real
commerce_core::ir::compile path using a frozen, provenance-recorded installed
merchant lexicon. Do not hand-construct an empty plan per query or use special
cases for motif terms. Record engine commit, catalog hash, lexicon hash and
compiler/configuration. Export each original probe query exactly once.

Audit proposal, resolved constraint, and **actually enforced** hard-filter stages
separately. A proposed product type that the validator rejects is not an enforced
category filter. Map typed slots explicitly; classify unsupported/ambiguous
operators as such rather than silently dropping them. Report coverage and failures
beside routing, and keep this deterministic compiler arm separate from each LLM
prompt arm. Low slot emission alone can mean a compiler that ignores everything;
B_attr coverage and retrieval relevance are required counterchecks.

## Stage 3: WANDS / ESCI retrieval gates, not run here

Reuse existing maintained retrieval infrastructure rather than create another
comparator implementation. Relevant project entry points:

- scripts/datasets/fetch_wands.sh and wands_checksums.sha256
- scripts/round1/fetch_esci.sh and export_esci.py
- scripts/datasets/fetch_esci_electronics.sh, fetch_esci_automotive.sh,
  fetch_esci_beauty.sh and their filter scripts
- crates/comparator-eval (shared comparator correctness/error handling)
- docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md and sibling slice protocols
- docs/EXPERIMENT_LOOP.md and the Issue #57 full-matrix gate

These existing ESCI vertical/train slices are not the paper's full-catalog ESCI
retrieval population. Do not pool them or compare their scores as though they are
the same benchmark. Pin locale, split, catalog universe, query/qrel set, ID mapping,
gain scale, cutoffs and filtering behavior per arm. Paper motif relevant_pids are
procedural catalog-derived sets, not WANDS human judgments; report separately.

For an actual paired retrieval experiment, freeze candidate catalog and qrels,
run no-filter and enforced-filter arms with the same ranker and cutoff, and report
recall loss, judged ranking quality, coverage, query-level uncertainty, errors and
latency. Errors are failed observations, never zero-quality comparator scores.
Do not retune on the frozen test set.

Upstream explicitly omits source catalogs, intermediate parquet outputs, and
roughly 2.4 GB of derived retrieval caches. Those must be rebuilt and verified
before full-catalog harm/recall claims. Cached routing replay cannot establish
retrieval loss, current-system superiority, or production motif prevalence.

## Execution record (2026-11-09)

Base repository inspected: 623545defe5e81fa8f261a618e6e119850b795ce.
Python 3.12.14, Linux 6.18.57 x86_64. No performance measurements.

**Passed:** all 36 cells exactly match the published output with both the strict
wrapper and unchanged official scorer; 73 queries each / 55 entity denominator;
13 unit tests; Python compilation. Report:
artifacts/public_benchmarks/qp_faith_replay.json.

**Not run:** Rust workspace fmt/clippy/test/release build, current compiler export,
live retrieval services, dataset download/rebuild, full-paper replication.
This executor had no repository checkout; the git mirror clone required unavailable
credentials. Source files were materialized through the authenticated GitHub
connector and verified by Git blob hash. The public --fetch network path was not
exercised here. Changes are isolated Python tooling/docs, not a claim of full
workspace validation.

**Decision:** cached public replay gate passes. Keep the PR draft until the
current-system exporter and retrieval gates can run in an equipped checkout.
Next question: do the installed, actually enforced structural constraints lose
relevant cross-category products on the identical public probe?

## Continuation: actual compiler arm (2026-11-09)

The initial execution record above is preserved. A subsequent implementation adds
crates/issue61-eval/src/bin/qp_faith_native.rs and the Public QP-Faith replay PR
workflow. That workflow provides an equipped checkout without credentials or
model APIs in the evaluator.

This arm reuses issue61_eval::load_dataset(Wands), including its existing
CatalogProfile -> compile_lexicon(min_enum_frequency=1), then calls the real
commerce_core::ir::compile and CommerceQuery::execute for every public query.
It is the existing **statistics-derived lexicon arm**, not an LLM-installed
merchant context, and does not generalize to every system configuration.

It records hard constraints, soft preferences, ambiguity and residual text;
counts ProductType/ProductTypeAny hard-filter emission; and measures the fraction
of each procedural relevant_pids set surviving the compiled hard constraints.
Candidate IDs are deduplicated at product level after variant-safe execution.
Missing/duplicate relevant IDs, incomplete probe counts and an incomplete WANDS
catalog stop execution. Both motif and attributed query rows remain in the report.

This is **candidate-set retention, not ranked Recall@K**. With no hard filters all
catalog products survive, so perfect retention alone says nothing about useful
retrieval or attribute extraction. Inspect B_attr constraints/candidate counts and
the untouched residual text before drawing conclusions. Category constraints
are retained in the debug record but are not silently relabeled product_type.

The workflow fetches WANDS using the existing immutable source/checksum script,
uses prepare_wands.py without changes, tests the adapter, runs its release binary,
and uploads JSON evidence. The report fingerprints the exported catalog and
probe and records GITHUB_SHA, which is the checked-out PR merge commit in PR CI.
The deterministic lexicon's provenance is that commit, catalog hash, and existing
loader/min-frequency setting; no separately installed lexicon is substituted.

Equivalent command in an equipped checkout, after fetching/preparing WANDS and
the pinned public source:

```sh
cargo test -p issue61-eval --bin qp_faith_native
cargo run --release -p issue61-eval --bin qp_faith_native -- \
  dataset_cache/wands/catalog.jsonl \
  dataset_cache/qp-faith-public/data/probe/probe.jsonl \
  artifacts/public_benchmarks/qp_faith_native.json
```

At implementation time CI results were pending; measured results follow below. ESCI retrieval,
ranked WANDS judgments, LLM-installed lexicon export and full-paper headline
replication remain out of this arm's measured scope.


## Measured native result and adversarial countercheck

[Successful run 42680176431](https://github.com/yangjeep/simple-ecommerce-search-engine/actions/runs/42680176431)
tested head 79af38c8f2cc9ebc67e9955fdb6da386227aea82 using PR merge commit
e8448616eb3704348865100d7eb983b0b22dcaed. Full output is preserved in
artifacts/public_benchmarks/qp_faith_native.json, downloaded from CI artifact
13701074968. The ZIP SHA-256 was verified as
c0d6f01c235ae1719fbd039e63ddec29891432d3666a3cf8ddb802922f26754d.

- 39 pinned public files fetched and verified; 36/36 cached score cells match.
- WANDS source checksums verified; existing preparation produced 42,994 products.
- Both native regression tests pass, including real compilation/execution,
  product deduplication, deliberately different source/internal IDs, and the
  cross-variant negative case. All 13 Python tests pass.
- 55/55 A_entity queries emit no product-type hard filter and retain every
  procedural relevant product. **They also retain all 42,994 catalog products.**
  This is abstention at the hard-filter layer, not proof of good ranking.
- 18/18 B_attr queries emit at least one hard constraint. 16/18 match all
  reference slot/value pairs under exact normalized matching. This is slot
  coverage, not overall semantic correctness.
- B_attr mean per-query relevant-candidate retention is **0.4021730074**.
  Sixteen of eighteen queries lose relevant products. "white doormats" retains
  none. Thus matching the requested slot values is not sufficient for faithful
  candidate retention under these representations.
- "metal chandeliers" and "glass chandeliers" retain their material ambiguity;
  only product type is hard-filtered. Both retain all reference relevant products.

### Interpretation and competing explanations

Do **not** declare an overall pass from the motif result. The attributed-query
countercheck exposes a real mismatch that must be understood before an
end-to-end quality claim.

Source inspection identifies a concrete semantic mismatch: QP-Faith make_probe.py
defines B_attr relevance with normalized token membership across all feature
values. Existing prepare_wands.py retains one value per feature key, and the
engine ingestion uses single Enum attributes/exact filtering. This is a
plausible mechanism for the loss, not an exhaustive query-by-query causal audit.
The engine also has documented product-class normalization choices. Do not
change either representation or labels after seeing these scores just to raise
the result; a harmonized arm needs its own declared protocol and baseline.

**Verdict: REVISE for cross-benchmark semantic comparability.** Cached reproduction
passes; the native motif routing arm abstains safely; attributed candidate
retention does not establish comparable semantics. No ranking, speed, production
prevalence or full-system superiority claim follows.

Fresh independent review prompted the exact-slot countercheck, source-ID /
variant-safety regression, and dependency-aware CI triggers. No further concrete
defects were found in that delta. Repository-wide Rust CI is separately tracked
on the PR; the benchmark run is not a substitute for that full quality gate.

Next work: characterize the 16 attributed losses against original feature values,
preregister any representation alignment, then run ranked WANDS and separately
pinned ESCI populations. Keep full-catalog ESCI distinct from existing training
vertical slices.
