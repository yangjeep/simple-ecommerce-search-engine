# Issue #57 — Full-Matrix E2E Benchmark: Frozen Protocol (Revision 1)

**Status:** FROZEN at the commit below. No architecture tuning, threshold
change, or dataset addition may occur after this point without declaring a
new revision (Revision 2, append-only) and rerunning affected cells.

**Frozen at commit:** `c6953063e2641ceada1992d1372766a2e6ad63cd` (branch
`claude/simple-ecommerce-falsification-loop-6p0bsp`, reset from `main`
after PR #56 merged).

**Frozen at:** 2026-08-28 (session start ~2026-08-27T17:00Z), single
continuous session, single host, no cross-session state reused except the
already-existing local Solr cores from prior sessions (`wands_bench`,
`esci_electronics_bench`, `esci_automotive_bench`, `esci_beauty_bench` —
row counts independently re-verified against `dataset_cache/` source files
below, not merely trusted).

This document is the entry point required by Issue #57 ("Before running
the first measured matrix cell, preregister and freeze..."). It is
intentionally scoped down from Issue #57's maximal ask, with every
reduction stated as a concrete, reproducible reason rather than a silent
omission — see §9 (Exclusions).

---

## 1. Governing question

> Across materially different real ecommerce workloads, when semantic
> behavior and work performed are held equivalent, where does the current
> commerce-native/hybrid architecture materially outperform mature
> engines, where does it merely match them, and where should a mature
> engine remain responsible?

## 2. Entry-gate confirmation (A + B, per CLAUDE.md / Issue #55 stage gate)

Verified before this protocol was drafted:

- A1–A4 complete: hyponym promotion gate (`29d7c4d`), promotion oracle
  (`3bc8b61`), comparator centralization (`e1ac219`/`d27cf54`), doc refresh
  (`43055d2`) — all merged into `main` via PR #56.
- B complete: `docs/experiments/ISSUE57_DATASET_RECOVERY_LOG.md` — dataset
  recovery audit, Havenask and Retailrocket recovered.
- PR #56 is **merged** (verified via `pull_request_read`, `merged: true`,
  `merged_at: 2026-08-27T17:19:29Z`). This session's branch was reset to
  `origin/main` (not continued from PR #56's branch content) per the
  merged-branch protocol.

## 3. Engines (frozen versions, this session, this host)

| Engine | Version | How run | Port | Auth | Heap/resource config |
|---|---|---|---|---|---|
| Native / commerce-core | commit `c695306` (this repo) | in-process Rust, release build | n/a | n/a | n/a (single-threaded per-query unless stated) |
| Solr | 9.10.1 | standalone (`bin/solr start`), user-managed mode, local install at `/home/user/solr_setup/solr-9.10.1` (persisted across sessions, not repo-tracked) | 8983 | none | default JVM heap (512m/512m, `-Xms512m -Xmx512m`, as shipped by `bin/solr`) |
| Elasticsearch | 8.15.0 (official tarball, `artifacts.elastic.co`) | standalone single-node server (real server process, **not** the embedded-test-framework route used by earlier phases) | 9200 | disabled (`xpack.security.enabled: false`) — disclosed, matches Solr's unauthenticated local setup for a fair comparison | `-Xms3g -Xmx3g` |
| OpenSearch | 2.17.0 (official tarball, `artifacts.opensearch.org`) | standalone single-node server (real server process) | 9201 | disabled (`plugins.security.disabled: true`) — same disclosure as ES | `-Xms3g -Xmx3g` |
| Havenask | `ha3_runtime`, tag `latest`/`1.2.0` (same image, retagged locally — see §3.1), digest `sha256:a4fd0269ac54593c894510df783d7aa6e33169cecf9731f7d1a6f08bbec51734`, pulled from `registry.cn-hangzhou.aliyuncs.com/havenask/ha3_runtime`, source HEAD `26bf4c1567b42f6a4b48a74e8cdec0288a0d8faa` | single container, `hape` orchestration tool, **`proc` domain** (local-process mode, not the `default` domain's sibling-container mode — see §3.1), `docker run --init` | 45800 (QRS SQL), internal zk 2181 | none | `hape_conf/proc` defaults (adminMem 5124MB per sub-service budget, not all consumed by a WANDS-scale table) |

Secondary reference (not a mandatory-engine substitute, per Issue #57):
none run this revision — Lucene-direct/OpenSearch-as-secondary is
superseded by OpenSearch already being promoted to mandatory-equivalent
treatment above, since the harness cost was low once ES's real-server
route was built.

### 3.1 Havenask — environment/build changes required (disclosed in full)

Per Issue #57 §"Havenask requirement": every environment/build change is
recorded here.

1. **Docker-in-Docker via `hape`'s `default` domain is not available in
   this sandbox.** `hape`'s `default` domain config
   (`hape_conf/default/global.conf`) spawns three sibling containers
   (`havenask-swift-local`, `havenask-sql-local`, `havenask-bs-local`) by
   calling `docker run` from inside the outer container, which requires
   mounting the host's `/var/run/docker.sock` into that outer container.
   That mount was **blocked by this session's own safety guardrails**
   (auto-mode classifier denial: host Docker socket access is
   equivalent to host-root and is refused). This is a real, reproducible,
   session-level infrastructure constraint, not a Havenask defect.
   **Route taken instead:** `hape`'s shipped `proc` domain
   (`hape_conf/proc/global.conf`, `processorMode=proc`) runs the same
   admin/QRS/BS/swift processes as local OS processes inside one
   container, with no Docker socket dependency. This is an
   **officially-shipped Havenask deployment mode** (not a fork or
   Havenask-inspired substitute), selected because it is the closest
   supported single-node configuration reachable in this sandbox, per
   Issue #57's explicit instruction ("use the closest supported
   single-node configuration that preserves the benchmark's single-node
   scope").
2. **`docker run --init` is required.** Without it, PID 1 inside the
   container is the harness's own `sleep infinity` placeholder, which
   never reaps zombie child processes; `hape`'s `proc` mode spawns many
   short-lived children, and after ~2 minutes the accumulated zombies
   destabilized the admin process (`hape gs havenask` started reporting
   "Havenask admin not ready"). Restarting with `--init` (a standard
   `tini`-based PID-1 reaper) fixed this outright — a legitimate
   operational Docker flag, not a benchmark-relevant change.
3. **Three genuine Python 2→3 porting bugs in the shipped quickstart
   tooling were found and patched**, isolated strictly to the *example
   CLI wrapper scripts*, not the Havenask engine itself:
   - `/ha3_install/example/common/case.py`,
     `run_command_wth_return()`: concatenates a `str` literal to
     `subprocess.Popen(..., stdout=PIPE).communicate()`'s `bytes` output
     (`out + "/n" + error`) → `TypeError: can't concat str to bytes`.
     Patched to decode both streams first.
   - `/ha3_install/usr/local/lib/python/site-packages/hape_libs/utils/sql_query.py`,
     `sql_query()`: encodes the query to `bytes` then passes it as a
     `requests` JSON body value (`json={"assemblyQuery": query}`), which
     `json.dumps` cannot serialize → `TypeError: Object of type 'bytes'
     is not JSON serializable`. Patched to leave the query as `str`.
   - `/ha3_install/usr/local/lib/python/site-packages/hape_libs/utils/havenask_dataset.py`,
     `HavenaskRecord.to_sql()`: same `str.encode('utf-8')`-without-decode
     pattern applied to every field value before `",".join(values)` →
     `TypeError: sequence item 0: expected str instance, bytes found`.
     Patched the same way.

   All three are the identical porting artifact (Python 2 code where
   `str`/`bytes` were interchangeable, run unmodified under the image's
   bundled Python 3.6), each independently reproducible from a clean
   `docker pull` + `docker run` with no other changes. None touch the C++
   QRS/searcher/BS/swift binaries, the SQL query planner, the index
   format, or any query-semantics code path. **Correctness proof
   (§3.2) was obtained using these three patches; the underlying engine
   binaries are stock, unmodified.**
4. **Local retag, not a re-pull:** `hape_conf/*/global.conf` hardcodes
   sibling-service image `ha3_runtime:1.2.0`, while the top-level pull
   used `:latest`. `docker manifest inspect` was not compared byte-for-
   byte across tags before retagging (a residual risk, disclosed); the
   image was retagged locally as `1.2.0` without a second pull to conserve
   this session's disk allowance. Since `proc` mode never actually spawns
   sibling containers from that reference (see point 1), this retag was
   not exercised as a live dependency in this revision and is documented
   for completeness only.

### 3.2 Havenask — correctness proof obtained before benchmarking

The official quickstart (`/ha3_install/example/common/case.py run --case
normal`, `direct`-table type) was run to completion after the patches in
§3.1:

- Table `in0` created and reached `READY` / `WS_READY` status (verified
  independently via `hape gs havenask` / `hape gs table`, not only the
  wrapper script's own exit code).
- 7 real rows inserted via SQL `INSERT` (`assemblyQuery` over
  `QrsService/searchSql`), all reported `ERROR_NONE`.
- `SELECT ... INNER JOIN ... ON in0.id = in0_summary_.id` returned all 7
  expected rows with correct joined columns.
- `SELECT ... WHERE MATCHINDEX('default', '阿里巴巴集团')` returned
  exactly the 2 rows whose `title`/`subject` genuinely contain that
  string (rows 20, 21; row 22 excluded) — a known-expected-output
  full-text correctness check, not a fabricated pass.

This satisfies Issue #57's "prove semantic correctness against known
expected outputs, then benchmark it" gate for Havenask's SQL/QRS query
path before any commerce dataset is loaded into it.

## 4. Hardware / runtime environment (shared across all engines, this session)

- Host: single VM, `Linux vm 6.18.44-fc-v22` (kernel), Ubuntu 24.04.4 LTS
  userspace.
- CPU: 4 vCPU, `Intel(R) Xeon(R) Processor @ 2.10GHz` (cloud/virtualized —
  **disclosed as a measurement caveat**: this is a shared/virtualized CPU
  identity string, not a dedicated bare-metal part number; absolute
  latency numbers should be read as relative cross-engine comparisons on
  this fixed host, not as portable absolute production numbers).
- RAM: 15 GiB total, ~13 GiB available at session start.
- Disk: reported filesystem size 252 GB, but the actual writable
  allowance for this session is a small fixed quota (observed: available
  space dropped from ~22 GB to ~12 GB purely from pulling/extracting the
  four engines' own binaries/images, before any dataset indexing) — see
  §9 for how this bounds the dataset matrix.
- JDK: OpenJDK 21.0.10 (Solr, host default); Elasticsearch and OpenSearch
  each run their own bundled JDK (shipped in their tarballs).
- Python: 3.11.15 (host, used for indexer scripts); 3.6 (bundled inside
  the Havenask container image, used for its example tooling).
- All four engines run on the same host at the same time is possible
  resource-wise (headroom: ES 3g + OS 3g + Solr default 512m + Havenask's
  `proc`-mode processes, against 13 GiB), but **benchmark runs are
  executed one engine at a time**, per Issue #57 §"isolate engines so one
  engine's process/cache state does not contaminate another." Idle
  engines remain started (to avoid cold-JVM confounds from repeated
  restarts) but are not queried while another engine is being timed.
- Disk-allocation watermarks for ES/OpenSearch were raised
  (`disk.watermark.{low,high,flood_stage}` → 97/98/99%) — disclosed in
  §3, a pure infra-safety-valve change (this session's fixed quota is
  reported as a tiny percentage of a much larger shared partition,
  tripping the *default* 85/90/95% watermarks despite several GB
  genuinely free). This does not touch query semantics, relevance, or
  timing.

## 5. Datasets — frozen candidate set and MEASURED/DEFERRED/N/A status

See §9 for the full reasoning behind each status; summary:

| Dataset | On disk this session | Products | Status this revision |
|---|---|---|---|
| WANDS | yes (`dataset_cache/wands/catalog.jsonl`) | 42,994 | **MEASURED** (all 4 engines) |
| ESCI electronics slice | yes (`dataset_cache/esci_electronics/`) | 2,075 | **MEASURED** (all 4 engines) |
| ESCI automotive slice | yes (`dataset_cache/esci_automotive/`) | 1,056 | **MEASURED** (all 4 engines) |
| ESCI beauty slice | yes (`dataset_cache/esci_beauty/`) | 2,093 | **MEASURED** (all 4 engines) |
| Magento configurable | yes (`dataset_cache/magento_configurable/catalog.jsonl`) | 22 | **MEASURED where meaningful** (correctness/Product-Variant only — too small for stable P50/P95/P99) |
| ESCI full corpus (1.2M products) | **no** — not re-fetched this revision | 1,215,854 (per prior manifest) | **DEFERRED** — concrete reason in §9.1 |
| Retailrocket | zip present, not extracted this revision start; extracted during §12 | 2.76M events, no relevance judgments | **N/A for retrieval/relevance; MEASURED for traffic-weighting only**, per Issue #57 §4's explicit instruction not to invent ground truth it doesn't have |

Dataset hashes/licenses: unchanged from
`docs/experiments/ISSUE57_DATASET_RECOVERY_LOG.md` (WANDS/ESCI/Magento
never blocked, hashes already on file; Retailrocket sha256 `5dc06173...`,
CC BY-NC-SA 4.0, non-commercial — disclosed for any downstream use of
this benchmark).

## 6. Semantic equivalence rules (per Issue #57 §"Fairness contract")

For every measured query/workload class, the intended commerce semantics
are defined **independent of any engine** first (in terms of
`commerce_core::ir::ResolvedConstraint` — `Structural` or `Attribute`
variants), then translated per engine:

- **Native**: direct `CatalogIndex` structural query — the reference
  semantics.
- **Solr**: `fq` clauses via `comparator-eval::translate::translate_constraint`
  (12 exhaustively-matched constraint shapes, no wildcard arm — a new IR
  variant fails to compile there rather than silently under-filtering).
- **Elasticsearch / OpenSearch**: `bool` query `filter` clauses
  (`term`/`terms`/`range` on `keyword`/`double` fields with
  `doc_values: true`, mirroring Solr's docValues-backed fields
  field-for-field) — implemented this revision in
  `crates/comparator-eval` (§7).
- **Havenask**: SQL `WHERE` clauses over the SQL/QRS endpoint, on
  `STRING`/`INT64`/`DOUBLE` typed columns with `attribute` indexes
  (Havenask's docValues-equivalent), translated by a dedicated
  `HavenaskComparator` (§7).

An engine that cannot express a given semantic (e.g., a capability gap)
is recorded as a **capability difference**, never silently substituted
with a broader/narrower query. See §7 for the translation-matrix
verification gate that must pass before any cell's timing is trusted.

## 7. Comparator infrastructure

`crates/comparator-eval` is Solr-only entering this revision
(`SolrComparator` implementing `EngineComparator`). This revision adds:

- `ElasticsearchComparator` / `OpenSearchComparator` (share one
  implementation — both engines share the query-DSL/bulk API subset used
  here; differ only in base URL/port) implementing the same
  `EngineComparator` trait, returning the same 4-way `EngineLookup`
  (`Success`/`TransportError`/`QueryError`/`ParseError`) so transport
  failure is never confused with a real empty result.
- `HavenaskComparator` implementing `EngineComparator` against the SQL/QRS
  endpoint (`assemblyQuery` POST), parsing Havenask's own JSON response
  shape and its `error_info` field to distinguish success from failure.

All four comparators are exercised by the same `PairedComparison`
accumulator discipline already established for Solr in PR #56 (A3): no
method exists to record a metric for a failed lookup; a failed comparator
run aborts the cell rather than silently scoring it as zero/absent.

## 8. Query/workload taxonomy (Q1–Q17, per Issue #57 §"Common query/workload taxonomy")

Mapped per-dataset in the dataset-capability matrix
(`docs/experiments/ISSUE57_DATASET_CAPABILITY_MATRIX.md`, produced
alongside this protocol). Classes not populated by a given dataset are
recorded `N/A` with a structural reason, never silently skipped.

## 9. Exclusions (concrete, reproducible reasons — never silent)

### 9.1 ESCI full corpus (1.2M products) — DEFERRED, not BLOCKED

Concrete reason: this session's writable-disk allowance is a small fixed
quota, not the filesystem's nominal 252 GB. Observed consumption before
any dataset indexing: session start ~22 GB available → ~12 GB available
after pulling/extracting only the four engines' own binaries/container
image (Elasticsearch 605 MB tarball → ~1.2 GB extracted, OpenSearch
900 MB tarball → ~1.6 GB extracted, Havenask container 2.3 GB compressed
→ 8.1 GB virtual/2.3 GB on-disk layer). The full ESCI corpus
(1,215,854 products, multi-GB raw + query/judgment files) indexed
redundantly into 4 engines simultaneously would risk exhausting the
remaining ~12 GB allowance mid-matrix, which would corrupt in-progress
index state non-reproducibely rather than fail cleanly. This is a
resource constraint of this specific session/host, not an architectural
or dataset-access blocker (the host is reachable — confirmed HTTP 200 —
and the three ESCI slices drawn from the same corpus **are** measured).
Recorded per Issue #57's own cell-status contract: `DEFERRED — disk
allowance (~12 GB avail at protocol-freeze time) insufficient for a
4-engine-redundant 1.2M-product index without evicting other measured
cells; the three frozen ESCI slices (electronics/automotive/beauty,
5,224 products combined) are measured in full instead and are the
dataset's existing Issue #35 evidence base.` Revision 2 may reattempt
this with a larger disk allowance or a bounded/sampled full-corpus slice,
explicitly declared as such.

### 9.2 Retailrocket — relevance/retrieval classes N/A, not fabricated

Per Issue #57 §4's explicit instruction: Retailrocket is 2.76M real
shopper events with **no accompanying relevance judgments or query
ground truth**. It is used exclusively for traffic/popularity-weighting
in the whole-workload economics synthesis (§13 of the governing issue),
never scored for NDCG/Recall/MRR. This is a dataset-capability fact, not
a benchmark scoping shortcut.

### 9.3 Magento — correctness-only, not a performance/P50-P99 cell

22 products is too small a sample for stable latency percentiles (a
single GC pause or scheduler jitter would dominate P95/P99). Magento is
measured for **Product/Variant correctness and semantic-equivalence
verification only** (its real value per Issue #57: genuine
Product/Variant identity data), not timing.

### 9.4 Lucene-direct / OpenSearch-as-"secondary-only"

Superseded: OpenSearch is run as a full mandatory-equivalent server this
revision (§3), not a lesser secondary reference, since the real-server
route (as opposed to the embedded-library route used in prior phases) was
inexpensive to add once built for Elasticsearch. Lucene-direct is not run
this revision (no incremental evidence value beyond what
Solr/ES/OpenSearch's shared Lucene core already demonstrates for this
matrix's purposes).

## 10. Warmup / repetition / metrics protocol

Reuses Phase 6A's established methodology (`p6a_e00`) rather than
inventing a new one:

- **Warmup:** 5 iterations, discarded.
- **Repetitions:** 30 timed iterations per (engine, query-class, group)
  cell.
- **Timing:** wall-clock `Instant::now()` around the single logical
  operation (native: direct index call; engines: single HTTP
  request/response round-trip including client-side JSON
  parse — i.e., what a real serving caller would pay, not
  server-internal-only timing).
- **Statistics:** mean, P50, P99 in this revision (P95 added where a
  cell's variance report calls for finer resolution).
- **Correctness gate:** every timed cell's result cardinality/content is
  compared across engines *before* its timing is trusted; a mismatch is
  recorded as a correctness failure, not averaged away.
- **Order:** engines are benchmarked one at a time (§4); within a run,
  the same fixed query/group selection (seeded RNG, `SEED = 7`, matching
  `p6a_e00`) is reused across all engines for that dataset so no engine
  sees an easier sample by chance.
- **Cold vs warm:** this revision measures **warm** (post-warmup)
  performance only, consistent with `p6a_e00`'s prior methodology;
  cold-start/build/index-time is measured and reported separately (§11)
  but not conflated with per-query serving latency.

## 11. Index/ingestion economics measured

Build/indexing wall-clock time, document count, and (where obtainable
without adding new instrumentation this revision) approximate on-disk
index size are recorded per (dataset, engine) cell.

## 12. Execution protocol

Per Issue #57 §"Execution protocol": clean index build → correctness
check → warmup → repeated measured runs → raw output preserved → variance
inspected → surprising results rerun → failed/corrected runs preserved →
only then summarized. Raw per-cell CSV/JSON artifacts land under
`docs/research/artifacts/issue57_full_matrix/` and
`benchmarks/manifests/` + `artifacts/manifests/` per this repo's existing
convention.

## 13. Interpretation / pass-fail rules

- A cell is **MEASURED** only after its correctness gate (§10) passes for
  every query in that cell.
- A capability gap (an engine cannot express a semantic) is recorded as
  such and excluded from that cell's timing comparison, never silently
  forced.
- No claim of "Native wins" or "Engine X wins" is accepted from a single
  run; §"surprising wins/losses" reruns (Issue #57 §"Execution protocol")
  apply before any number is written into the final synthesis.
- Whole-workload economics (§10 of the governing issue) are computed
  separately from, and never substituted for, per-query-class conditional
  results.

## 14. Stop conditions (unchanged from the governing task instructions)

Work continues autonomously across all matrix cells except: a real
human product/architecture choice that cannot be experimentally
resolved; credentials/paid data/material spend required; equivalent
semantics cannot be defined without inventing ground truth; a mandatory
engine is genuinely impossible after multiple legitimate attempts
(documented per §3.1 for Havenask — not invoked, since Havenask **was**
achieved); or the complete Issue #57 decision is ready.

---

*Revision history: Revision 1 (this document) is the initial freeze.
Any later revision is appended below this line, never edited in place,
per CLAUDE.md's "preserve corrected and superseded evidence" rule.*
