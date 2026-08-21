# Architecture — how the system actually works today

This describes the **implementation that exists in `main` today**, through
Phase 5. It is deliberately narrower than Issue #21's Phase 9 target
architecture — see `docs/WHAT.md` for the explicit non-goals and what is
still only a design target. Where this document says a mechanism "exists,"
it means running code with tests and/or a real-data experiment behind it,
traceable to an ADR and/or an experiment log entry.

For the durable *why* and the evidence-backed *what*, see `docs/WHY.md` and
`docs/WHAT.md`. For architectural decisions and their rationale, see
`docs/adr/`. For the experiment-by-experiment evidence behind every claim
below, see `docs/experiments/` and the `PHASE*_DECISION.md` files.

## Crate map

- **`commerce-core`** — the engine itself: typed domain, Commerce IR,
  physical indexes, admission, control plane, state overlay, planning. No
  crate outside `commerce-core` is a dependency of it (kept deliberately
  minimal — see ADR-0001).
- **`round1-eval`** — real-data adapters (ESCI catalog ingestion, Solr
  client helpers, relevance judgment loading) and the first real-corpus
  evaluation binaries. Every later `phaseN-eval` crate depends on this for
  real data rather than re-implementing ingestion.
- **`phase2-eval`** through **`phase5-eval`** — one crate per research
  phase, each holding that phase's experiment binaries
  (`src/bin/pXeNN_*.rs`) and any phase-specific eval-only helpers. These
  are evaluation harnesses, not product code — see `docs/WHAT.md`'s
  non-goals.
- **`bench-harness`** — shared repeated-measurement/timing/statistics
  infrastructure (percentiles, bootstrap CIs) used across phases so timing
  methodology doesn't drift between experiments.
- **`realtime-eval`** — Issue #8's variant-availability fast-path
  evaluation.

## 1. Catalog compilation

Real product data (Amazon ESCI: title, description, bullet points, brand,
color — no price/category/product-type field in the source at all) is
adapted into `commerce_core::domain` types by `round1_eval::catalog`. Since
the real data has no product-type/category/price, every real product is
assigned an explicit sentinel (`ProductTypeId(0)`, `CategoryId(0)`,
`Price::usd(0)`) rather than inventing ground truth — this is recorded
explicitly in the ingestion code's own doc comment, not hidden, and it is
the reason Phase 5 could not test genuine category/PLP workloads (see
`docs/WHY.md`). `BrandId(0)` is a similar sentinel for "no brand field on
this real product" and is a real adversarial hazard any brand-based
mechanism must exclude explicitly (Phase 4 found and fixed exactly this
bug).

## 2. Query compilation (Commerce IR)

`commerce_core::ir::compile` turns a raw query string into a typed
`CommerceQuery`: spans are resolved against a `SemanticLexicon` into
`ResolvedConstraint`s (structural facts — brand, color, etc.) or left as
`residual_lexical` (free text the lexicon didn't resolve). Ambiguity
(`AmbiguousSpan`) is a first-class outcome, not collapsed to a guess, when
the lexicon has multiple competing candidates for a span and no signal to
choose between them (ADR-0002) — this is the mechanism CLAUDE.md's
"preserve ambiguity explicitly" rule actually cashes out as. Coverage
(`ir::coverage::measure_coverage`) reports what fraction of a query
corpus resolves fully structurally, as a real, measured metric, not an
estimate.

## 3. Physical operators (`commerce_core::index`)

`CatalogIndex` is the immutable structural index: compact IDs, per-value
`RoaringBitmap`s for enum attributes (brand, color, ...), used for:

- **Filtering** (`indexed_candidates`) — bitmap AND across every resolved
  structural constraint. This is the operation behind every large
  filter-only speedup measured in Phases 2–5.
- **Faceting** (`facet_counts`, `brand_facet_counts`, and their `_by_scan`
  counterparts) — count candidates per distinct attribute value. Phase 5
  found the original implementation is `O(global attribute vocabulary)`
  regardless of candidate-set size (a real, measured 35–420ms cost at this
  catalog's 175K–206K-value cardinality); the `_by_scan` variants
  (`O(|candidates|)`, added and parity-tested in Phase 5) fix this for
  small-to-medium candidate sets, but scan cost is still linear in
  candidate-set size, so a real crossover exists (measured at roughly
  9,000–12,000 candidates in this catalog) past which the *original*
  vocabulary-scan implementation, or Solr, wins instead. **No operator
  selects between the two automatically today** — this is exactly the
  cardinality-aware planning gap named in `docs/WHAT.md` and targeted for
  Phase 6+.
- **Sorting** (`execute_ranked` / `native_title_sorted`-style helpers) — a
  full sort of the candidate set. Phase 5 found and disclosed this is a
  naive full `O(n log n)` sort even though only a page of results is ever
  needed, so its speedup collapses for large result sets (measured as low
  as 1.65x on an 11,264-product group) — a known, real, *not yet fixed*
  inefficiency, not a fundamental property of native sorting.
- **Ranking within a candidate set** (`index::rank`) — Phase 2 (P2-E17)
  found this has no real relevance signal when `query.preferences` is
  empty (which it is for the compiled baseline lexicon), so ties break on
  ascending `(product_id, variant_id)`, not relevance. This is the direct
  cause of the NDCG gap Phase 3/4 both measured between native and Solr on
  the same admitted queries — a known, unfixed, disclosed limitation, not
  an oversight discovered late.

## 4. Admission (the serving-time routing decision)

`commerce_core::admission` is the single decision point between routing a
query to the native path and forwarding it unmodified to Solr
(`docs/adr/0008-narrow-to-structural-planning-layer.md`,
`docs/adr/0009-structural-lexical-execution-contract.md`). It is
deliberately cheap — no delegate call, no index execution beyond a
selectivity check — because its cost is paid on every query, including
every rejected one. Three mechanisms are independently verified and kept,
each strictly additive with the others (Phase 3, `P3-E06`/`P3-E10`/
`P3-E16`):

1. **`admit`** — fully structurally resolved query, no residual.
2. **`admit_structurally_anchored_lexical`** — at least one structural
   constraint plus a lexically-narrowed residual.
3. **`admit_single_token_lexical`** — exactly one residual token
   (structural constraint optional).

A rejected query is forwarded to Solr **exactly as if commerce-native did
not exist** — this is what keeps the measured fallback tax statistically
indistinguishable from zero (Phase 3, P3-E01).

### Semantic enrichment before admission (Phase 4)

`commerce_core::control_plane::implication` sits in front of admission: a
compiled `ImplicationTable` can add resolved facts to a query (e.g. "air
force 1" implies Brand=Nike) before the same three admission mechanisms
run. `ImplicationTable::compile` enforces two safety properties
structurally, not just by test: only `Promoted`-status rules are ever
served, and conflicting `Promoted` rules sharing the same trigger cause
that trigger to abstain entirely at compile time (found and fixed as a
real bug during Phase 4's own adversarial review, not from a test
failure).

## 5. Backend delegation (today: an eval-harness integration, not a real adapter contract)

`commerce_core::plan` defines a `LexicalDelegate` trait and composes it
with `CatalogIndex` into three execution outcomes (Phase 2, ADR-0009):
**FastPath** (fully structural, delegate never called), **Hybrid**
(structural narrowing then delegate ranks the narrowed set), and **Punt**
(delegate searches, native verifies). `commerce_core` itself has zero
dependency on any concrete lexical engine — the only implementation of
`LexicalDelegate` lives in `phase2-eval`, wrapping a Tantivy index. Solr is
used throughout Phases 3–5 as the fair-baseline comparison target via
`round1_eval::solr`, not as an implementation of `LexicalDelegate` — **there
is no production "backend contract/adapter" abstraction yet** that would
let Solr or Havenask be swapped in behind the planner at serving time; that
is a Phase 9 target (Issue #21), not built today. Do not read
`round1_eval::solr`'s HTTP client as that contract.

## 6. Mutable commerce state (Issue #8)

`commerce_core::state::CommerceStateOverlay` is a real, running mechanism
for one specific field class: variant availability/OOS. It is
deliberately independent of `CatalogIndex` (the immutable structural
index knows nothing about mutable state, and the overlay knows nothing
about brand/category semantics) — they compose only through
`commerce_core::plan::execute_with_overlay`. The mechanism is in-place
`RoaringBitmap` mutation, the same physical idea Havenask/IndexLib
independently converged on for this class of field per
`docs/research/havenask-realtime-update-archaeology.md` (a clean-room
implementation, not derived from Havenask's source). Two real limitations
are tracked as open issues, not silently accepted: no durability/replay
across a restart (**Issue #12**), and a single coarse-grained `RwLock`
rather than a finer-grained concurrency primitive (**Issue #11**). This
overlay does **not** cover price or general typed attributes, and there is
no per-tenant bundle concept yet.

## 7. Control-plane learning lifecycle (offline, never in the hot path)

`commerce_core::control_plane` implements observe → propose → replay →
promote (Gate 5, ADR-0005), the *only* place a model/LLM signal is allowed
to enter this codebase, and only offline:

- **`observe_residual_terms`** — find real unresolved query terms from a
  corpus.
- **`ModelProvider`** (trait; `FixtureModelProvider` for tests — CLAUDE.md's
  "no test may require a real model API key" rule) — proposes a candidate
  resolution for an observed term.
- **`replay`** — measures whether adopting a candidate lexicon strictly
  improves coverage on the query corpus with no regression, before it is
  ever considered for promotion.
- **`check_precision`** (`PrecisionOracle` trait; `FixtureJudgmentOracle`
  for tests) — a second, independent gate added after Round 1 (R1-E06)
  found the coverage-only gate structurally cannot reject a nonsensical
  mapping for a previously-unseen term. `try_promote_with_precision` is
  additive; it does not change `try_promote`'s existing behavior.
- **`ImplicationTable`** (Phase 4) is the same discipline applied to a
  different rule shape (conjunction-of-facts rather than
  competing-lexicon-candidates): propose from a real, zero-model-call
  co-occurrence signal, replay against held-out judgments, gate on a false-
  positive-rate ceiling, then compile to a versioned, inspectable lookup
  table. Nothing on the query hot path calls a model.

## 8. What does not exist yet (do not infer product-readiness from the above)

To keep this document from reading as more built than it is:

- No per-tenant bundle, tenant isolation, or resource accounting of any
  kind.
- No cost/cardinality-aware planner that picks between native and Solr, or
  between two native implementations, based on measured breakpoints — the
  facet-scan and sort breakpoints above are *known* but not yet *acted on*
  by any runtime decision.
- No real backend-swap contract (Solr vs. Havenask behind one interface).
- No scale-out, warmup, or cluster coordination of any kind — everything
  above runs single-node, single-process.
- No observability/explain/debug surface for why a query was admitted,
  rejected, or enriched — today that information only exists as
  eval-harness console output and CSV artifacts, not a queryable/servable
  explain path.
- No production polish — every binary in this repository is a benchmark or
  evaluation harness (`src/bin/pXeNN_*.rs`), not a deployable service.
