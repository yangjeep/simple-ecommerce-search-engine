# Scripts

Two conventions coexist here, deliberately:

## Phase-scoped scripts (existing, frozen paths)

`scripts/round1/` and `scripts/phase2/` hold the dataset-acquisition,
indexing, benchmarking, and corpus-building scripts specific to Round 1 and
Phase 2. Their paths are referenced by name in frozen, historical
experiment logs (`docs/experiments/ROUND1_LOG.md`, `PHASE2_LOG.md`,
`docs/research/brand-adjudication-rubric.md`) and in Rust doc comments.
Per `CLAUDE.md` ("do not rewrite history to make the project look
cleaner") and the general archive discipline in Issue #21 ("never
silently edit historical experiment conclusions"), these are **not**
moved into the functional layout below even though Issue #21 asks for a
`scripts/datasets/`, `scripts/build-indexes/`, `scripts/run-benchmarks/`,
`scripts/reproduce/` structure going forward — moving them would make
every historical log reference a path that no longer matches what was
actually run.

Phases 3–5 needed no new phase-scoped scripts (their real-data adapters
live in `round1_eval` and are reused as a Rust dependency, not a script).

## Functional scripts (new, for Phase 6+)

New, cross-phase infrastructure — the kind Issue #21's Phase 6 needs
(acquiring Amazon Reviews 2023 / WANDS / Retailrocket / eCommerceSearchBench,
building indexes across multiple engines, running the same workload against
several backends) — goes under the functional layout Issue #21 specifies:

- `scripts/datasets/` — acquisition + verification for each new external
  dataset (one acquisition script per dataset, mirroring `round1/fetch_esci.sh`'s
  pattern: download, checksum, and a documented one-command re-run).
- `scripts/build-indexes/` — build a native `CatalogIndex` and/or a Solr
  (and, once integrated, Havenask) index from an acquired dataset.
- `scripts/run-benchmarks/` — execute a workload manifest (see
  `benchmarks/manifests/`) against one or more engines and write raw results.
- `scripts/reproduce/` — thin, documented wrappers chaining the above three
  steps end-to-end for one promoted result, per `benchmarks/README.md`'s
  reproduction contract.

These directories are created empty by this commit (Phase 6 has not started
yet — see `PHASE5_DECISION.md`'s "recommended Phase 6 starting point") and
will be populated as Phase 6 dataset/engine work actually happens, not
speculatively ahead of it.
