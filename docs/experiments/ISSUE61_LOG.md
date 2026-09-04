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

## Step 5 — quantifying D1: how large was the straw man?

The adversarial review asserted that emitting Solr's exact structured filters as
regular expressions inflates the baseline's CPU. That is a claim about
magnitude, not just principle, so it was measured directly rather than
accepted.

First, what the translator actually emits. `case_insensitive_field_regex`
builds a **per-character alternation**, not an inline flag:

```
case_insensitive_field_regex("Beds")  ->  [bB][eE][dD][sS]
fq=product_class:/[bB][eE][dD][sS]/
```

Lucene compiles that into a DFA and runs it across the field's term
dictionary. The production-equivalent form is a single term lookup against a
`KeywordTokenizer + LowerCaseFilter` companion field:

```
fq=product_class_lc:"beds"
```

### Semantics first

Both forms were run against the live 42,994-document WANDS core:

| Form | `numFound` |
|---|---|
| `product_class:/[bB][eE][dD][sS]/` | **1112** |
| `product_class_lc:"beds"` | **1112** |

Identical. The replacement is semantics-preserving, not a relaxation — which
had to be established before any cost comparison was meaningful.

### Then cost

300 queries per form, `{!cache=false}` so `filterCache` could not absorb the
difference, CPU read from the container's own `cpu.stat`:

| Form | CPU delta (300 queries) | CPU/query |
|---|---|---|
| regex (historical) | 13,461,006 µs | **44,870 µs** |
| exact term (Revision 2) | 8,470,499 µs | **28,234 µs** |

**The historical comparator made Solr spend 1.59x the CPU for an identical
answer — a 37.1% handicap.**

### Why this matters more than it first appears

The campaign's materiality bar is a 25% resource reduction. The comparator
defect alone was worth **37%** on structured filters. A "commerce-native
execution uses 25% less CPU than Solr" result could therefore have been
produced entirely by the measuring apparatus, with no architectural content
whatsoever — and it would have passed every gate Revision 1 defined, because
Revision 1 only checked whether the number was *repeatable*, not whether it was
*right*.

This is the concrete justification for Revision 2's mandatory known-effect
calibration arm: repeatability cannot detect systematic accounting bias. A
biased instrument is perfectly repeatable.

The absolute numbers above are inflated (they include HTTP and JVM cost, and
the JVM is only partly warmed) and are **not** citable as Solr's serving cost.
Only the ratio is claimed, and both arms were measured under identical
conditions back to back.

### Consequence for comparability

E1's Solr numbers are therefore **not** directly comparable to `p9_e02`'s and
other historical WANDS numbers, which used the regex filter. This is a
deliberate, disclosed break: the older numbers were measured against a
handicapped baseline. The historical wire format remains the library default
and is pinned by a regression test, so no previously published number changes
retroactively.

---

## Step 6 — hands-on QA catches the defect class a third time, before measurement

The three harness binaries were delivered with a full green gate (fmt, clippy
`-D warnings`, 477 workspace tests, release build) and self-reported
`SELFCHECK_OK`, `ANCHOR_OK` and a correct-looking Solr-shaped response. All of
that was true and none of it was sufficient.

Driving the boundary by hand found two problems a passing test suite could not.

### The 21/459 anchor holds

First, the good news, verified independently:

```
HISTOGRAM FastPath=7 Hybrid=14 Punt=459
SHA256 bfd5935802e392a4774556d7927621f6bb814e61e1d277795cb6b4002a733e70
ANCHOR_OK
```

`7 + 14 = 21` structural-routed, `459` punt-routed — exactly reproducing
P9-E02's published routing split. The frozen workload is the workload prior
evidence was measured on.

### Finding 1 (BLOCKING) — Solr was being asked a different question

`i61_bench::query_once` sent, to **both** engines identically:

```rust
.query("q", &query.text)
.query("rows", &query.rows.to_string())
```

That is the whole request. Consequently Solr received:

- **no `fq` at all** — every structural constraint the frozen workload records
  was computed and then discarded, so Solr answered an *unconstrained* query
  while native applied its filters;
- **no `defType=edismax`, no `qf`** — falling back to the default parser and
  default field rather than the configured lexical fields;
- **no `fl=id`** — so Solr materialized and serialized every stored field per
  document, while native returned ids only.

The comparison would have been incoherent rather than merely unfair: Solr does
*more* work (larger unconstrained result sets, far more serialization) while
answering a *different* question.

**This is the third time this exact defect class has shipped in this
repository.** `ISSUE55_PAIRED_COMPARATOR_DECISION.md` found Solr silently
receiving no product-type filter; `ISSUE55_ROUTING_OUTCOME_REPLICATION_DECISION.md`
found `issue35-eval` sending no `Brand`/`color` `fq` at all. Both were caught
*after* numbers had been published. This one was caught before any measurement
existed — which is the only difference, and the entire point of putting the
equivalence audit and hands-on QA ahead of the measured campaign.

The recurrence is itself the finding: centralizing the translator in
`comparator-eval` (Issue #55 A3) removed the *translation* defect but not the
*call-site* defect — a new binary can still simply forget to call it. Section
16.9 therefore makes the frozen workload artifact carry the translated `fq`
list, so the driver replays a checksummed contract instead of reconstructing
one.

### Finding 2 (disclosed, minor) — response payload asymmetry

Structural comparison of the two response bodies:

```
native shape: {"response":{"docs":[{"id":"str"}],"numFound":"int"},
               "responseHeader":{"status":"int"}}
solr   shape: {"response":{"docs":[{"id":"str"}],"numFound":"int",
                           "numFoundExact":"bool","start":"int"},
               "responseHeader":{"status":"int"}}
```

Solr emits two extra scalars (`numFoundExact`, `start`). Small, but it is
serialization work native does not do, and it is exactly lane 2 of the
adversarial results-review brief ("does the native server do equivalent
response work?"). Recorded rather than silently ignored.

### Note on the raw text-query divergence observed during this probe

An ad-hoc probe with `q=chair&qf=title` returned `numFound` 4604 (native) vs
3255 (Solr), and `dining table` returned 1387 vs 4871. These are **not**
evidence of a defect: native's lexical postings cover title *and* `Text`
attributes and combine residual tokens with AND, while an edismax `qf=title`
query covers one field and defaults to OR. Protocol §5.3 already declines to
require identical text-retrieval sets ("different rankers is the premise").
The numbers are recorded here only so a later reader does not rediscover them
and mistake them for a finding. What §5.2 *does* gate — full structural
candidate-set equality — is measured by the T10 audit, not by this probe.

---

## Step 7 — Elasticsearch time-box (§2.4): INFRA_OK, but DEFERRED-TO-PRE-E2

§2.4 preregistered Elasticsearch as a secondary, gate-optional arm with a
time-box. A time-box that is never opened is just an assertion, so it was
actually run.

`bash scripts/issue61/provision_es.sh wands`:

```
==> (re)starting i61-es under the frozen limits
  health: green 1 nodes
==> bulk indexing wands
  bulk submitted 42994 docs
==> refresh + forcemerge(1)
==> count=42994 (expected 42994)
==> exact-term filter product_class_lc=beds -> 1112 hits
==> store_size_bytes=22176120
ES_TIMEBOX_RESULT=INFRA_OK docs=42994 beds_hits=1112 store_bytes=22176120
```

Elasticsearch 8.15.0 runs on this host under the *identical* frozen limits as
Solr (3 CPU, cpuset 0-2, 6 GiB, `--memory-swap == --memory`), reaches
`green`, accepts a competent commerce mapping, and reproduces corpus parity at
42,994 documents.

### An unplanned three-way semantic cross-check

The ES mapping uses `copy_to` into a `keyword` field with a lowercase
`normalizer` — the Elasticsearch analogue of Solr's `copyField` into a
`KeywordTokenizer + LowerCaseFilter` field. Filtering
`product_class_lc = "beds"` returns:

| Engine | Form | Hits |
|---|---|---|
| Solr | `product_class:/[bB][eE][dD][sS]/` (historical regex) | **1112** |
| Solr | `product_class_lc:"beds"` (Revision 2 exact term) | **1112** |
| Elasticsearch | `term: product_class_lc = "beds"` | **1112** |

Three independent implementations agree exactly. This was not the purpose of
the time-box, but it is worth more than the time-box itself: it is evidence
that the lowercased-companion substitution is a genuine semantics-preserving
transformation rather than a Solr-specific coincidence, established on a second
mature engine that shares no configuration with the first.

### Verdict: DEFERRED-TO-PRE-E2, and precisely why

The infrastructure is **not** the blocker. What blocks ES from entering E1's
measured campaign is **adapter scope**:

- `comparator-eval` has no Elasticsearch translator arm (its `translate`
  module emits Solr `fq` syntax);
- the frozen workload artifact (§16.9) carries `native` and `solr` request
  blocks and would need an `es` block;
- `i61_bench` has no ES request shape to replay.

That is real work, and doing it inside this issue would mean writing a new
comparator adapter *during* a measurement freeze — exactly the kind of
concurrent change the freeze exists to prevent.

Recorded verdict: **`DEFERRED-TO-PRE-E2`**, per §2.4. E1's gate is computed on
native + Solr only, as preregistered. The distinction matters for #57 and #62:
this is *not* "Elasticsearch could not be made to run here" — it demonstrably
can, under the campaign's own resource limits, and the provisioning script plus
the verified mapping are merged so the adapter work starts from a working
baseline rather than from scratch.

The container was stopped immediately after the check; ES consumes no resources
during the measured campaign.

---

## Open items

- Elasticsearch adapter/provisioning is time-boxed per protocol §2.4; its
  verdict (`INCLUDED` or `DEFERRED-TO-PRE-E2`) is recorded below when reached.
- Oracle protocol review of the process-boundary resolution (§7) and the gate
  statistic (§11) must complete before any measured run.
