## Product/system boundary

This describes the evidence-backed system boundary as of Phases 0–5. For
what is actually implemented today (as opposed to targeted by future
phases), see `docs/architecture/`.

### Commerce Core / typed Commerce IR

Product/Variant/ProductType/Brand/Category/Price/Inventory/typed attributes
are first-class domain types (`commerce_core::domain`), not generic
JSON/document fields. A query compiles into a typed Commerce IR
(`commerce_core::ir`) that explicitly represents which spans are
structurally resolved, which are unresolved lexical residual, and where
ambiguity exists — ambiguity is preserved, not silently collapsed to a
best guess, when confidence is insufficient (see ADR-0002).

### Structural native execution

Where a query's constraints are fully or partially resolved to typed
facts, physical indexes (compact IDs, bitmaps, typed columns, minimal
postings — ADR-0003) execute the resolvable part directly: exact
filtering, faceting, sorting, and pagination as set/structural operations
rather than ranked lexical retrieval. This is the mechanism behind every
"native is N times faster" result in Phases 2–5, and it is real and
substantial where it applies.

### Semantic forwarding / admission

The serving-time decision of whether to run a request natively or forward
it unmodified to Solr is a first-class, explicit admission step
(`commerce_core::admission`), not an implicit fallback. Three disjoint
mechanisms are currently kept (Phase 3): full structural resolution with no
residual, structurally-anchored lexical narrowing, and single-token
residual admission. A compiled semantic-implication lookup
(`commerce_core::control_plane::implication`, Phase 4) can enrich a query's
resolved facts before the same admission check runs. Admission is
deliberately conservative: abstention (falling back to Solr) is preferred
over an unsafe admission, and the fallback path is designed to stay
statistically indistinguishable from the unmodified Solr baseline in cost.

### Cardinality/cost-aware planning (targeted, partially evidenced)

Phase 5 established, empirically, that not every structural operator
should always execute natively: facet computation and full-result-set
sorting both have real, measured cardinality-dependent breakpoints where a
generic engine's specialized data structures (docValues, incremental
faceting) win. The architectural implication — a planner should select
native execution only within its measured advantage region, and delegate
or pick an alternative native implementation outside it — is stated
explicitly in Issue #21 and is a design target for Phase 6+, not yet a
built cost-based planner today.

### Mature lexical/ranking backend delegation

Free-text relevance ranking is not reimplemented. Solr is the reference
mature backend for the unresolved/ambiguous/genuinely-lexical tail of
traffic, used as-is (not weakened) as the fair baseline every native result
is measured against. Havenask is named as a second, specialized-performance
anchor for Phase 6 (not yet integrated).

### Immutable structural index + mutable commerce-state overlay (built for one field class; not yet a full tenant bundle)

The variant-availability slice of this split is real, built code, not just
a target: `commerce_core::state::CommerceStateOverlay` (Issue #8) applies
in-place `RoaringBitmap` mutation for availability/OOS state, kept
deliberately independent of the immutable `CatalogIndex` — the two compose
only through `execute_with_overlay` (`commerce_core::plan`), and neither
module knows the other's internals. This validated, in an evidence-gated
experiment, that Havenask's own architecture independently converges on
the same idea for this class of field (see
`docs/research/havenask-realtime-update-archaeology.md`): true in-place
mutation instead of a full reindex, for fields where that is safe.

What is **not** yet built: this exists only for availability, not for
price or general typed attributes; it is in-memory only with **no
durability or replay across a restart** (tracked, unresolved, as Issue
#12); its concurrency primitive is a single coarse-grained `RwLock`
(tracked, unresolved, as Issue #11); and there is no per-tenant bundle
concept at all yet — that composition (immutable tenant bundle + this
kind of overlay, generalized) is a Phase 6+ target motivated by Issue
#21's multi-tenant economics questions.

### Versioned, learned semantic context

Learned knowledge (e.g. brand-implication rules) is never applied directly
from a live model call. It is proposed offline from real catalog/query
signal, replayed against held-out real judgments, gated by an explicit
false-positive-rate ceiling, and only then compiled into a versioned,
inspectable lookup table actually served (`ImplicationTable::compile`,
Phase 4). No mechanism in this codebase calls a model in the query hot
path, and none is planned to.

### Multi-tenant isolation model (target, not yet built)

Tenant isolation, noisy-neighbor behavior, per-tenant resource accounting,
and packing density are Phase 7 research questions (Issue #21). No
multi-tenant serving code exists yet; today's evaluation harnesses
(`round1-eval`, `phase{2,3,4,5}-eval`) operate against a single ingested
catalog with no tenant concept.

## Explicit non-goals (current epic)

Carried forward from `CLAUDE.md`'s hard rules and confirmed by what has
and has not been built through Phase 5:

- **Not a whole-engine replacement.** Falsified in Phase 2 — see
  `docs/WHY.md`. Solr (or an equivalent mature backend) remains the
  permanent fallback for unresolved/ambiguous/lexical traffic.
- **No LLM/model call in the default query hot path.** Every model-assisted
  signal used anywhere in this codebase runs offline and is validated
  before being compiled into deterministic serving state.
- **No generic query DSL.** Query semantics are typed Commerce IR, not a
  generic Elasticsearch-Query-DSL-style document filter language.
- **No authentication, tenancy enforcement, or multi-tenant serving code
  yet.** Multi-tenancy is a Phase 7 research question, not an implemented
  feature.
- **No high-availability or cluster coordination.** Single-node,
  single-process evaluation only through Phase 5.
- **No distributed-systems work** (sharding, replication, consensus) until
  the single-node thesis is fully measured — explicitly deferred per
  `CLAUDE.md`.
- **No production polish or UI.** All current code is research/evaluation
  infrastructure (`*-eval` crates and benchmark binaries), not a deployable
  service.
- **No incremental/mutation update path for the structural index itself.**
  `CommerceStateOverlay` (Issue #8) covers in-place mutation for
  availability specifically, with no durability across a restart (Issue
  #12, open) and a coarse-grained `RwLock` (Issue #11, open). `CatalogIndex`
  — the structural/facet/sort index Phase 5 benchmarked — has no
  incremental-update API at all for any field; Phase 5 found and disclosed
  this, and mutation/churn sensitivity for that index is explicitly out of
  scope until it is addressed (a Phase 6+ item).
- **No claim of universal search-engine superiority.** Every "faster"
  result in this repository is scoped to a specific, disclosed operator
  and cardinality range — see the relevant `PHASE*_DECISION.md` for the
  actual measured boundary.
