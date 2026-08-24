# Issue #35 Experiment Log — Generalize the Specialization Methodology

## Governing context

Issue #35 asks whether domain specialization itself can be *learned from
an unseen workload* rather than engineered per vertical, run in parallel
with Issue #34 (Phase 9 evidence closure) but kept epistemically separate:
share infrastructure (dataset acquisition, profiling, manifests, benchmark
runners, correctness/relevance tooling), never share hidden conclusions.
The methodology must be frozen *before* its blind-replay score is
evaluated against Phase 9's benchmark truth — tuning the methodology
after seeing that truth would invalidate the eventual methodological claim.

## I35-E00: Workstream A — freeze the domain-neutral representation

**What this is**: a specification task, not a benchmark. Issue #35's own
required first step ("Before blind replay, define the smallest
domain-neutral representation needed...").

**Method, stated to keep this epistemically clean**: the representation
in `docs/research/DOMAIN_NEUTRAL_REPRESENTATION.md` was derived from (a)
Issue #35's own listed primitives, and (b) a direct, static read of what
`commerce_core`'s existing type system already generalizes vs. does not
— never from any Phase 9/WANDS *benchmark outcome*. The Phase 9 (P9-E02)
WANDS re-run was running in a sibling git branch/PR during this same
session; this document does not cite any of its measured numbers, and
was written by reading `commerce_core`'s source directly, not by
reasoning backward from what P9-E02 found. This is a real, disclosed
process discipline, not merely an assertion — recorded here so a future
reader auditing the epistemic boundary can verify it: the only WANDS-
related fact cited in the representation document is `phase6a_eval::catalog`'s
*structural shape* (its depth-6 hierarchy, its lack of brand/price data)
— a fact about the ingestion code, true regardless of any query ever
being run against it, not a measured result.

**Result**: `docs/research/DOMAIN_NEUTRAL_REPRESENTATION.md` written and
frozen. Key findings (see that document for full detail and citations):

- Already domain-neutral, reusable as-is: `Constraint`'s attribute-name-
  keyed Enum/MultiEnum/Boolean/Numeric/Text shape; `AttributeValue`;
  `cold_start::profile::{CatalogProfile, compile_lexicon}` (confirmed
  reusable unmodified against WANDS in Phase 9, itself a real,
  cross-catalog generalization data point, though not this document's
  own conclusion — it is Phase 9's, cited here only as "this exists and
  works," not re-derived); `Candidate.confidence`; `AmbiguousSpan`; the
  bitmap/postings physical layer.
- Not domain-neutral, matching Issue #35's own falsification criteria
  directly: `StructuralConstraint`'s fixed six-variant entity-family enum
  (Brand/BrandAny/ProductType/Category/PriceUnderCents/PriceOverCents) —
  a genuinely unseen vertical's entity families have no representation
  without a Rust source change, exactly the "representation requires
  vertical-specific serving code" falsification criterion, found true of
  the *current* architecture by direct reading, before any blind replay
  or unseen-vertical test.
- No Relationship, first-class Hierarchy, Unit, or Provenance primitive
  exists anywhere in the codebase today.
- A target type shape (`EntityFamily`, `HierarchyPath`, `Unit`,
  `Relationship`, `Provenance`, `MerchantSemanticProfile`) is specified as
  the representation to freeze — not yet implemented; implementation is
  separate future work under Workstream B.

**Explicitly out of scope for this entry**: whether to actually implement
these new types in `commerce_core`, whether doing so would help or hurt
Phase 9's measured system, and the discovery *algorithm* itself
(Workstream B) — all deferred.

## Shared infrastructure inventory (per Issue #35's "Parallel execution rule")

Surveyed once, here, so neither epic re-derives it. All of the below
already exist, are catalog-agnostic, and are safe to reuse from either
Issue #34 or Issue #35 work without leaking conclusions:

- **Dataset acquisition/profiling**: `scripts/datasets/` (WANDS
  fetch/prepare/profile scripts, `replicate_wands_scale.py` for
  controlled-stress replication), `crates/phase6a-eval/src/{data,catalog}.rs`
  (WANDS-shaped ingestion — a concrete template for what a *second* real
  vertical's ingestion module should look like structurally, not a
  reusable library itself, since field names are WANDS-specific).
- **Cold-start profiling/lexicon compilation**: `commerce_core::cold_start::{CatalogProfile, compile_lexicon}`
  — confirmed catalog-agnostic (see I35-E00 above), directly reusable for
  any future vertical's ingestion without modification, *as long as* that
  vertical's entity families still fit `StructuralConstraint`'s fixed
  enum — the one real, disclosed limitation this reuse currently has.
- **Manifests**: the `benchmarks/manifests/*.yaml` +
  `artifacts/manifests/*.json` two-file convention
  (`benchmarks/README.md`, `artifacts/README.md`) — dataset-agnostic,
  reusable verbatim by any Issue #35 experiment.
- **Benchmark/statistical-rigor runner**: `crates/bench-harness` —
  `Distribution::compute`, `measured_repeat`, `bootstrap_ci_diff_of_means`,
  `RunManifest` — all generic, no ESCI/WANDS-specific assumptions.
  Deliberately does *not* include relevance/NDCG scoring (its own doc
  comment scopes it to timing variance only).
- **Correctness/relevance tooling**: `round1_eval::query_taxonomy::{classify9, QueryClass9}`
  operates on any compiled `CommerceQuery`, not ESCI-specific — directly
  reusable for a future vertical's own query classification. NDCG/Recall/
  MRR scoring is *not* generic today: `round1_eval::relevance` is
  `EsciLabel`-specific (4-way scale), `phase9_eval::wands_relevance` is
  `WandsLabel`-specific (3-way scale) — each new vertical's own judgment
  scale has so far required its own small label-to-gain module. A
  genuinely generic `ndcg_recall_mrr(hits, judged: &BTreeMap<String, f64>, k)`
  taking pre-computed gains directly (trivial to factor out of either
  existing implementation) would remove this duplication; not yet done,
  named here as a concrete, low-risk shared-infra improvement available
  to either epic.
- **Artifact traceability**: `docs/research/artifacts/<experiment_id>_run1/`
  raw-output convention, already dataset-agnostic.

## Next steps for this epic (not yet started)

Workstream B (unknown-catalog discovery pipeline), Workstream C
(blind-replay protocol + scoring rubric, frozen before running),
Workstream D (>=3 unseen verticals), Workstream E (merchant
heterogeneity), Workstream F (cold-start artifact, now type-shaped above
but not populated by any real pipeline) — all substantially larger,
multi-session workstreams, not attempted in this pass.
