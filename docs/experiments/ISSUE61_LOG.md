# Issue #61 Experiment Log — Infra E1: fair baseline + reproducible resource benchmark harness

Append-only, per `docs/experiments/README.md`. First-draft mistakes,
adversarial review, corrections, reruns, and superseded outputs are preserved
rather than rewritten.

Protocol: [`ISSUE61_PROTOCOL.md`](ISSUE61_PROTOCOL.md) (committed before any
measured run).

---

## Governing context

Issue #60 turns this project's infrastructure-cost thesis into a sequential,
merge-gated experiment campaign with a **>=25% materiality bar**. Issue #61 is
its first child, and it deliberately measures the *instrument* rather than the
architecture: if repeated runs on this host cannot resolve a 25% effect, every
downstream number in #62–#70 is undecidable.

---

## Environment as measured at session start

Recorded before anything else, because several campaign assumptions do not
survive contact with this host.

| Fact | Measured value | Consequence for the campaign |
|---|---|---|
| CPU | 4 vCPU, `QEMU Virtual CPU version 2.5+`, L3 16 MiB shared | #66's stated 16/8/4/2-core sweep is **not runnable here**; must become 4/3/2/1 via cgroup `cpu.max`. Surfaced to #60 before #66. |
| RAM | 15 GiB total, ~12 GiB available | #66/#70 RAM sweep ceiling |
| Swap | 4 GiB, **enabled** | RAM-envelope experiments are meaningless unless swap is pinned off per-container (`--memory-swap == --memory`) |
| cgroup | v2 (`cgroup2fs`); controllers `cpuset cpu io memory hugetlb pids` | CPU/RAM limiting and uniform per-container CPU accounting are feasible |
| `perf` | installed, but `perf_event_paranoid=4` and **no sudo** | **hardware counters unavailable.** #63's "cycles/query where practical" degrades to CPU-time accounting. Disclosed, never silently skipped. |
| `cpufreq` | absent (virtualized) | frequency cannot be pinned; steal time must be measured instead |
| Docker | 29.7.2, cgroup driver `systemd`, cgroup v2 | container route viable for all JVM engines |
| Java | **not installed** (`java`, `mvn` absent) | `es-direct-bench` / `lucene-direct-bench` / `opensearch-direct-bench` Maven projects cannot be built on this host; the embedded-JVM route used by Phase 6E is unavailable. Docker is the only route to a JVM engine. |
| `dataset_cache/` | empty except the committed `export/*.json` LLM proposals | **every public dataset had to be re-acquired**; `/dataset_cache/*` is gitignored by design |

---

## Step 1 — dataset re-acquisition (before any measurement)

`dataset_cache/` contained no raw corpora at session start. Re-acquired via the
repository's existing pinned fetch scripts:

```
bash scripts/datasets/fetch_wands.sh
bash scripts/datasets/fetch_esci_electronics.sh
python3 scripts/datasets/filter_esci_electronics.py
```

Results:

- **WANDS** — `product.csv: OK`, `query.csv: OK`, `label.csv: OK` against
  `scripts/datasets/wands_checksums.sha256`. 42,994 products / 480 queries /
  233,448 judgments, matching
  `docs/research/artifacts/p6a_dataset_acquisition/manifest.json` exactly. The
  corpus is therefore byte-identical to the one prior WANDS evidence was
  measured on, so E1 does not silently change the dataset while changing the
  measurement method.
- **ESCI-electronics** — `train0000.parquet` hashes to
  `bd6e1217eef98968103d9731ae52e5e1e640b8af8810956ad144cb13481bf3b9`, matching
  the committed `scripts/datasets/esci_checksums.sha256`. Filtered slice:
  2,075 products / 600 queries; 490/600 queries carry at least one
  non-`Irrelevant` judgment; judgment distribution
  `Exact 1406 / Substitute 422 / Complement 33 / Irrelevant 281`.

No dataset was regenerated or re-derived — both verify against checksums that
predate this issue.

## Step 2 — engine image freeze

```
docker pull solr:9.10.1
```

Resolved digest: `solr@sha256:1f055b0260d3efb177b12d6a46e9ef510fb4d2616473a91f8f4d099384aa176a`
(484,253,522 bytes). The tag's existence was verified with
`docker manifest inspect solr:9.10.1` before committing to the version, since
the historical Solr 9.10.1 used by prior checkpoints was a *local Java install*
that no longer exists on this host.

Version continuity is deliberate: keeping Solr at 9.10.1 means E1 changes the
measurement method without also changing the comparator identity.

## Step 3 — Solr provisioning, and confirmation that cgroup attribution works

`bash scripts/issue61/provision_solr.sh wands`:

```
==> verifying solr:9.10.1 digest
  digest OK: sha256:1f055b0260d3efb177b12d6a46e9ef510fb4d2616473a91f8f4d099384aa176a
submitted 42994 docs in 15.7s, committing...
commit took 4.4s
total index build time: 20.1s
numFound: 42994
==> forceMerge(1)
==> numFound=42994 (expected 42994)
==> index_bytes=25714386
PROVISION_OK core=i61_wands docs=42994 index_bytes=25714386
```

Corpus parity holds (42,994 = 42,994), so the Solr side is answering over the
same catalog the native side will.

### Scenario S6 — live smoke test

`bash scripts/issue61/smoke.sh` — **PASS (exit 0)**:

```
==> [1/6] container i61-solr is running                      ok: pid=918946
==> [2/6] cgroup v2 path resolves
  ok: /sys/fs/cgroup/system.slice/docker-3e8bbff6f27d9e63e6a1eaf951a7e9b442414519f3771b8532ece5e3f09a0d80.scope
==> [3/6] frozen limits applied
  ok: cpu.max=300000 100000 (= 3 CPUs)
  ok: memory.max=6442450944 (6g)
  ok: cpuset=0-2
==> [4/6] swap disabled for the container                    ok: memory.swap.max=0
==> [5/6] a real query returns results                       ok: numFound=42994
==> [6/6] cgroup CPU accounting moves under load
  cpu_delta_usec=5071982 (200 queries)                       ok: cpu_delta_usec>0
  cpu_usec_per_query=25359
  memory.current=2497318912 memory.peak=2527125504
SMOKE_OK
```

This is the first direct evidence that the campaign's central measurement
mechanism works on this host. Four things are now established rather than
assumed:

1. **The cgroup path resolves from `/proc/<pid>/cgroup`**, not from a guessed
   template. Docker here uses the `systemd` cgroup driver, so the container's
   cgroup is `/system.slice/docker-<id>.scope` — a path a `cgroupfs`-driver
   assumption would have missed entirely.
2. **The frozen limits are actually applied.** `cpu.max = 300000 100000` is
   exactly 3.0 CPUs and `cpuset.cpus.effective = 0-2`; a silently-ignored
   Docker flag would have meant measuring an unconstrained engine.
3. **`memory.swap.max = 0`.** The container genuinely cannot swap, despite the
   host having 4 GiB of swap enabled. Without this every `serving_rss_bytes`
   number — the metric the campaign's >=25% RSS bar depends on — would have
   been silently rescued by swap under pressure.
4. **`memory.peak` exists on this kernel**, so peak (not just instantaneous)
   memory is available without extra instrumentation.

Two observations recorded now, before they can be rationalised later:

- **25,359 µs CPU/query is not a result.** These were the first 200 queries
  after provisioning, with the JVM entirely unwarmed. It is recorded only as
  evidence that the counter moves, and as direct justification for the warm-up
  protocol in §9.1. It must never be cited as a Solr cost.
- **`memory.current` (2.49 GB) exceeds the 2 GiB Java heap.** In cgroup v2 the
  page cache backing Lucene's `MMapDirectory` is charged to the container's
  memory. This is a *feature* for E1's purpose, not a distortion: it means
  `serving_rss_bytes` measures the total memory the container actually needs,
  which is the fair basis for comparison against a heap-resident Rust index.
  Flagged for the Oracle protocol review, because it also means a "cold page
  cache" regime is doubly unavailable — see §9.1's disclosure.

---

## Step 4 — adversarial protocol review, run BEFORE any measurement

A reasoning agent was commissioned with an explicitly hostile brief: falsify
Revision 1, assume it is wrong until checked, and hunt for anything that would
let a later reviewer say *"this apparent resource saving is an artifact of
unfairness or missing work."*

**Verdict: BREAKS.** Six defects were named. Each was then independently
verified against the actual code rather than accepted on assertion — a
reviewer's claim is a hypothesis, not a finding. **All six reproduced.**

| # | Claim | Verification | Status |
|---|---|---|---|
| D1 | Solr exact filters emitted as regex | `translate.rs` emits `format!("{field}:/{}/", case_insensitive_field_regex(name))` for `Brand`/`ProductType`/`Category` | **CONFIRMED** |
| D2 | Malformed docs silently dropped | `solr.rs`: `filter_map(\|d\| d["id"].as_str()...)` still returns `Success` | **CONFIRMED** |
| D3 | `numFound` discarded | `EngineComparator::search` documents "Returns at most `rows` document ids" | **CONFIRMED** |
| D4 | `queryResultCache` oversized vs workload | `provision_solr.sh` set 4096; `wc -l query.csv` = 481 | **CONFIRMED** |
| D5 | `delta_since` saturates on rollback | `cgroup.rs`: `self.usage_usec.saturating_sub(earlier.usage_usec)` | **CONFIRMED** |
| D6 | Protocol contradicts itself on latency | §6.1 line 217 gates `latency_p50_us`; §7.4 line 275 says never gated | **CONFIRMED** |

### The two that would have done real damage

**D1 made the baseline a straw man.** Against a `string` + docValues field, a
Solr `RegexpQuery` runs an automaton over the term dictionary where a
production deployment would resolve a single term. Every "native uses less CPU
than Solr" number produced under Revision 1 would have been partly manufactured
by the comparator itself. This is the third time this repository has found a
fairness defect in comparator translation
(`ISSUE55_PAIRED_COMPARATOR_DECISION.md`, `ISSUE55_ROUTING_OUTCOME_REPLICATION_DECISION.md`),
and the first time one was caught *before* rather than *after* publishing
numbers.

**D4 was this protocol's own error, not an inherited one.** The provisioning
script written earlier in this same session set `queryResultCache.size = 4096`
against a fixed 480-query workload with three warm-up passes. Every result
would have been cached before measurement began, so "warm Solr CPU/query" would
have measured a hash lookup rather than retrieval — against a native engine
that has no whole-query result cache. It is recorded here with the same weight
as the inherited defects.

The fix is deliberately asymmetric, because the three Solr caches are not
equivalent: `queryResultCache` is disabled (it memoizes the entire benchmark),
while `filterCache` is *retained and generously sized* (it caches filter-context
bitsets and is the closest analogue to the native engine's precomputed Roaring
bitmaps — disabling it would be a straw man in the opposite direction).

### Statistical findings

Three independent problems with Revision 1's gate, all accepted:

1. The percentile bootstrap is **anti-conservative at n=10** — half-width
   ≈5.88% versus a correct Student-t ≈7.15%, roughly 18% too narrow, actual
   coverage ≈90.4% for a nominal 95% interval. More resamples add no
   information the sample does not contain.
2. **Non-overlapping CIs do not establish a ≥25% saving.** With both arms at
   ±7.5% around a 0.75 point ratio, the compatible range is 0.645–0.872 —
   savings from 12.8% to 35.5%. Revision 1's power justification was
   arithmetically correct but answered a Welch test the protocol never runs.
3. Repetitions are **serially dependent** (page cache, engine caches, host
   drift), so an IID bootstrap over raw repetitions is unjustified.

The most important consequence: **repeatability alone cannot detect systematic
accounting bias.** Revision 2 therefore adds a mandatory known-effect
calibration arm — 5 workload passes versus 4, a true ratio of 1.25, which the
instrument must recover — and treats a missing calibration as `FIX MEASUREMENT`
rather than a pass.

### Action taken

Revision 1 was **not edited**. `ISSUE61_PROTOCOL.md` §15 adds Revision 2, which
supersedes it where they conflict, and Revision 1 is preserved verbatim with a
banner. This follows the repository's standing discipline: preserve the
original attempt, document the defect, create a corrected revision. No
measurement had been taken under Revision 1, so no result changes — the cost of
this review was entirely paid in rework, which is the cheapest place to pay it.

---

## Open items

- Elasticsearch adapter/provisioning is time-boxed per protocol §2.4; its
  verdict (`INCLUDED` or `DEFERRED-TO-PRE-E2`) is recorded below when reached.
- Oracle protocol review of the process-boundary resolution (§7) and the gate
  statistic (§11) must complete before any measured run.
