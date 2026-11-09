# QP-Faith public alignment: cached replay gate

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

## Stage 2: current-system comparison, not implemented or run here

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
