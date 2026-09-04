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

---

## Open items

- Elasticsearch adapter/provisioning is time-boxed per protocol §2.4; its
  verdict (`INCLUDED` or `DEFERRED-TO-PRE-E2`) is recorded below when reached.
- Oracle protocol review of the process-boundary resolution (§7) and the gate
  statistic (§11) must complete before any measured run.
