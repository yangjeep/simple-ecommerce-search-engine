# ADR 0007: Benchmark decision package methodology, and the Elasticsearch baseline blocker

## Status

Accepted (Gate 7, Issue #2).

## Context

Gate 7 asks for reproducible benchmark evidence covering index size,
resident memory, P50/P95/P99 latency, a CPU-normalized throughput measure,
QPS/core, facet latency, index build time, and relevance/correctness on
the maintained query fixture — with an Elasticsearch baseline "if
practical in the cloud environment," and otherwise a recorded blocker plus
a reproducible Rust-only scaling curve.

## Decision

- **Elasticsearch baseline: blocked, recorded rather than skipped
  silently.** `docker info` in this session's container fails with
  `dial unix /var/run/docker.sock: connect: no such file or directory` —
  the `docker` CLI is present but no daemon is reachable, so a
  containerized Elasticsearch cannot be started. A non-Docker
  (tarball + JVM) Elasticsearch install was considered and rejected as
  disproportionate: it requires pulling a large distribution over this
  session's proxied network egress and meaningful setup/warm-up time for
  a single comparison point, when CLAUDE.md explicitly permits recording
  the blocker instead. This is the honest "not practical in the cloud
  environment" case the gate anticipates, not an avoided task.
- **A hand-written CLI report (`examples/decision_bench.rs`), not
  Criterion, for the percentile numbers.** Criterion (used in Gates 0/3)
  reports a smoothed mean-ish "time" estimate designed for regression
  detection, not raw P50/P95/P99 — extracting real percentiles from it
  would mean parsing its internal sample files, which is more fragile
  than measuring directly. `decision_bench.rs` times each call with
  `Instant::now()` in a loop, sorts the nanosecond samples, and reports
  the requested percentiles directly. This matches
  `docs/EXPERIMENT_LOOP.md`'s "no UI unless required to inspect an
  experiment and a CLI/report would not suffice" — this is exactly that
  CLI, not a UI.
- **Three scale-ladder tiers in one run: 1,000 / 10,000 / 100,000
  products** (2,000 / 20,000 / 200,000 variants), reusing the same
  deterministic `benches/common::synthetic_catalog` generator Gates 0 and
  3 already used (shared via `#[path]`, not duplicated). 100,000 products
  is the "medium" tier `docs/EXPERIMENT_LOOP.md`'s scale ladder names;
  reaching it directly answers E003's explicit open question ("behavior
  at the medium... tier... 14.4x at 10k is not a claim about the curve's
  shape at 100x that size").
- **Index size is `RoaringBitmap::serialized_size` summed across every
  bitmap** plus flat byte counts for the ordinal/numeric/price vectors
  (`CatalogIndex::approximate_size_bytes`, a new public method — Gate 7
  needed this metric exposed, not just computed ad hoc in a test).
  Explicitly not a precise allocator-level accounting (documented on the
  method itself): `HashMap` bucket overhead and `String` heap allocations
  for attribute/value names are not itemized. Good enough for
  cross-tier/cross-run comparison, not for a byte-exact memory budget.
- **RSS is read from `/proc/self/status` (`VmRSS`) immediately before and
  after `CatalogIndex::build`**, Linux-specific (this environment is
  Linux) and process-wide rather than isolated to just the index
  allocation (the delta also includes the synthetic catalog itself,
  already generated before the RSS-before sample is taken, so the delta
  is close to index-only, not catalog+index).
- **QPS/core is derived from the P50 latency of a single-threaded run**
  (`1_000_000 / p50_microseconds`), not measured with concurrent load.
  This is an honest per-core throughput bound assuming no lock contention
  (there is none — `CatalogIndex`/`Catalog` reads never mutate), not a
  claim about aggregate multi-core throughput, which would need an actual
  concurrent benchmark this gate does not build.
- **Relevance/correctness is not re-measured here** — it is the existing,
  already-passing test suite (33 tests across Gates 1-7 as of this
  commit, all run in CI on every push) plus the coverage numbers already
  recorded in E004/E006. Re-deriving a relevance metric from scratch in
  this report would duplicate evidence that already exists and is already
  regression-checked.

## Consequences

- See `docs/experiments/LOG.md` E007 for the full three-tier data table
  and interpretation, including the notable finding that the
  indexed-vs-linear-scan speedup *grows* with scale (roughly 6x at 1k,
  15x at 10k, 57x at 100k, averaged over 3 runs) rather than staying flat
  — the opposite of what a fixed per-query overhead in the indexed path
  would predict, and evidence the linear scan's cost genuinely does not
  amortize the way the bitmap/range structures do.
- `examples/decision_bench.rs` is not run in CI (multi-second wall clock
  per tier); it is a manual/scheduled experiment-loop step, the same
  status Gate 3's Criterion benches already have.
- No Elasticsearch (or any other external system) comparison exists
  anywhere in this repository's evidence. Every latency/throughput number
  is the Rust engine measured against itself at different scales, not
  against an industry baseline. This is a real, stated limitation of the
  evidence base, not resolved by this gate.

## Alternatives considered

- **Skip the medium (100k) tier and only extend E003's variance
  measurement at 10k.** Rejected: Gate 7 explicitly wants a scale-ladder
  benchmark package, and E003 already flagged "behavior at the medium...
  tier" as its own unresolved next question — doing both in one report
  (repeated runs at each of three tiers) closes more open threads than
  variance-only re-measurement at a single tier would.
- **Attempt a non-Docker Elasticsearch install anyway.** Rejected per the
  Decision section above — CLAUDE.md's own text ("if not practical...
  record the blocker") anticipates exactly this situation rather than
  requiring heroics to avoid it.
