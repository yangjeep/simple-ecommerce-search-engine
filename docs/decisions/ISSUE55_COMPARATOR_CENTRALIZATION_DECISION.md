# Issue #55 A3 — centralize/reuse hardened comparator infrastructure

## Governing directive

The same repository-owner closure directive that required A1/A2 required,
as A3: "Audit evaluation binaries for duplicate/weak comparator clients.
Centralize/reuse hardened behavior so that transport failure != zero
results, parse failure != zero results, query failure != zero results
and comparator failure can never silently: become NDCG=0; disappear from
the denominator; produce a favorable native result; cause asymmetric
query/filter semantics. Comparator structural translation must be
auditable and symmetric. Do not create yet another private Solr client
in a diagnostic binary. Design the interface with Issue #57 in mind so
the same fairness/error contract can be reused for Solr, Elasticsearch,
and Havenask adapters."

## Audit

Two independent full-codebase audits (converging on the same findings)
inventoried every Solr-calling binary across `round1-eval`, `phase2-eval`,
`phase3-eval`, `phase5-eval`, `phase6a-eval`, `phase7-eval`, `phase9-eval`,
`issue35-eval`, `issue38-eval`, `issue55-eval`. Findings, in priority
order:

1. **`phase9-eval/p9_e02_wands_physical_advantage.rs`** — the single most
   severe defect found: `solr_result.map(|r| r.ids).unwrap_or_default()`
   fed straight into `ndcg_recall_mrr`, so a transport/parse failure
   scored a native-favoring `NDCG=0.0`, silently blended into both the
   per-class table and the headline traffic-weighted verdict this
   binary's own doc comment calls "the actual Issue #34 decision."
2. **`phase9-eval/p9_e07_ambiguous_routing_diagnostic.rs`** — the same
   `None -> 0.0` collapse, *plus* a local copy of `wands_solr_query_for`
   that had drifted out of sync with its own claimed-identical twin in
   (1): missing the `ProductTypeAny` match arm entirely, silently
   sending Solr no product-type filter for any query native resolved to
   `ProductTypeAny` — the exact asymmetric-structural-translation defect
   class the directive names, confirmed live rather than hypothetical.
3. **`issue55-eval/src/bin/i55_e15_automotive_hybrid_gap_probe.rs`** and
   **`phase9-eval/src/bin/p9_e04_isolated_ranking_and_execution.rs`** —
   both used a private, unhardened `ureq` client and, on failure,
   `continue`d past the query with **no counter, no log line, no trace
   anywhere** that it happened — quieter than a scored zero, but still a
   denominator-disappearance defect.
4. **`phase2-eval/src/bin/p1d_physical_advantage_eval.rs`** — the
   historical origin point `round1_eval::solr`'s own module doc-comment
   claims was "factored out of," but the extraction never removed the
   source: a fourth independent copy of the same client, and its
   `if let Some(sr) = solr_search(...) { record }` (no `else`) let
   `solr_correctness.n` silently diverge from `cn_correctness.n`/
   `ts_correctness.n` on any failure, with no visibility into how often.
5. **`issue35-eval/src/eval.rs`** — the one binary that already got this
   right end to end (a private `SolrLookup` 3-way outcome, failures
   excluded from the comparison and the run aborted before publishing
   any number) — but its hardening was crate-private, unexported, and
   its own `fq` translation covered only `Brand`/color, so it was not
   actually reusable by any other binary despite being the strongest
   precedent in the codebase.
6. **`round1-eval/src/solr.rs`** — the most widely reused client (9+
   downstream callers), but its `solr_search` returns a binary
   `Option<SolrResult>`: `.ok()?`/`.as_array()?` collapse connection
   failure, timeout, non-2xx, and JSON-parse failure into one
   indistinguishable `None`, with no `responseHeader.status` check at
   all. Its `fq` builder (`extract_brand_color`) covers only
   `Brand`/`BrandAny`/color — a gap independently quantified by
   `phase3-eval/p3e14_solr_baseline_gap_audit.rs`, a diagnostic binary
   that exhaustively matches every `ResolvedConstraint` variant and
   finds 9 of 12 shapes have no `fq` substitute in this builder.
7. Lower-severity duplication (not silent-failure risk, since each
   `panic!`s loudly on transport failure instead of laundering it):
   `phase5-eval`'s and `phase6a-eval`'s four near-identical private
   `solr_get`/`solr_num_found`/`solr_facet` clients (filter/facet
   correctness, not NDCG), and `phase7-eval`'s two private
   `solr_query_once` throughput probes (no `fq`/NDCG at all).

## What was built

`crates/comparator-eval`, a new workspace crate, generalizing
`issue35_eval::eval`'s crate-private hardening (the strongest existing
precedent) into a reusable, tested contract:

- **`outcome::EngineLookup`** — a 4-way outcome (`Success` /
  `TransportError` / `QueryError` / `ParseError`), splitting
  `SolrLookup`'s conflated `TransportError` (which mixed "the request
  never got a real answer" with "the server rejected the query") into
  two distinguishable failure kinds.
- **`solr::{EngineComparator, SolrComparator, solr_search}`** — the
  hardened transport (POST form-encoded, checks
  `responseHeader.status`), exposed behind an `EngineComparator` trait
  so an Elasticsearch/Havenask adapter (Issue #57) can implement the
  same trait and reuse `translate`/`compare` unchanged. 15 tests,
  including every failure-fixture case `issue35_eval::eval`'s own test
  module had (connection refused, invalid JSON, missing `response.docs`
  shape, non-zero status) plus a new test proving `QueryError` and
  `TransportError` are actually distinguishable.
- **`translate::translate_constraint`/`translate_all`** — one
  exhaustive (no wildcard arm) match over every `StructuralConstraint`/
  `Constraint` shape (all 12), so a new variant fails to *compile* here
  instead of silently falling through a `_ => {}` — the exact mechanism
  by which the `ProductTypeAny` omission shipped twice. A
  `SolrFieldMap`/`StructuralNames` pair lets each dataset declare which
  Solr fields it actually has; a constraint for a field the dataset
  doesn't have becomes `Translation::NotApplicable` (safe, disclosed);
  a constraint whose id can't be resolved to a name becomes
  `Translation::Unresolvable` (a hard failure, never a partial/silently
  narrowed filter — proven directly by
  `brand_any_is_unresolvable_not_partial_when_one_id_is_unnamed`). 15
  tests, one per constraint shape plus the `ProductTypeAny` regression
  test named after the bug it prevents.
- **`compare::PairedComparison`** — an accumulator with no method that
  accepts a metric for a non-`Success` lookup, forcing every caller to
  route a failure through `record_lookup_failure`/
  `record_translation_failure` instead. `finish()` mirrors
  `issue35_eval`'s abort-before-publishing discipline; `finish_partial()`
  is a named, auditable escape hatch for a binary that deliberately
  wants disclosed partial reporting instead. **Disclosed honestly**: none
  of the five migrated binaries below actually use this type in
  production — each has its own multi-dimension aggregation (per query
  class, per routing outcome, per matched/control/broad population) that
  does not fit a flat native/other pair, so each implements the same
  fail-loud/disclosed-partial discipline directly with a local
  `Vec<String>` failure list instead. `PairedComparison` is exercised only
  by its own 4 unit tests today; it is real, tested, reusable
  infrastructure for a future binary whose structure is a flat paired
  comparison (e.g. the un-migrated `phase3-eval` NDCG sites named below),
  not yet a call site of its own.

30 unit tests total, `cargo clippy --all-targets --all-features -- -D
warnings` clean.

## What was migrated

| Binary | Defect fixed |
|---|---|
| `phase9-eval/p9_e02_wands_physical_advantage.rs` | Silent `NDCG=0.0` laundering (the confirmed live bug) → excluded + run aborts before printing any number on failure. |
| `phase9-eval/p9_e07_ambiguous_routing_diagnostic.rs` | Same laundering, plus the stale missing-`ProductTypeAny` `fq` arm (fixed automatically by delegating to the shared translator). |
| `phase9-eval/p9_e04_isolated_ranking_and_execution.rs` | Private client + silent trace-free drop → shared hardened transport + counted, reported failures, abort before printing. |
| `issue55-eval/i55_e15_automotive_hybrid_gap_probe.rs` | Private client + silent trace-free drop → same fix. |
| `phase2-eval/p1d_physical_advantage_eval.rs` | Duplicate private client (dedup'd `case_insensitive_field_regex` only; transport left as-is, see Scope below) + denominator-mismatch on failure → per-class and whole-run disclosure of excluded-query counts. |
| `round1-eval/src/solr.rs` | `case_insensitive_field_regex` is now a re-export of `comparator_eval::solr::case_insensitive_field_regex` (one duplicate implementation removed; public API unchanged for all 9+ downstream callers). |

### Live-data validation, not just compilation

Every migrated binary was run against a real, locally-controlled Solr
9.10.1 instance (`/home/user/solr_setup/solr-9.10.1`, the same install
prior checkpoints used) with the real, full WANDS (42,994 products, 480
queries) and ESCI-automotive corpora already in `dataset_cache/`, not
just compiled:

- `p9_e02_wands_physical_advantage`: ran clean, **zero comparator
  failures**, `structural_routed` gap = **-25.05%**, matching
  `ISSUE55_PAIRED_COMPARATOR_DECISION.md`'s own recorded pre-hyponym-
  expansion number exactly (current production has zero active
  `ProductTypeAny` promotions post-A1/A2, so no `ProductTypeAny`
  constraint is ever compiled right now — an expected, not a
  regression, result).
- `p9_e07_ambiguous_routing_diagnostic`: ran clean, reproduced
  `ISSUE55_AMBIGUOUS_ROUTING_DECISION.md`'s exact n=4, **-65.51%**
  matched-population gap byte-for-byte.
- `i55_e15_automotive_hybrid_gap_probe`: ran clean, reproduced
  `ISSUE55_HYBRID_ZERO_HIT_MECHANISM_DECISION.md`'s exact **14/32**
  zero-hit / **4** recoverable-miss automotive numbers byte-for-byte.

All three reproduce previously-published numbers exactly, confirming the
migration changed *failure handling and translation completeness*, not
the query semantics or numbers for the currently-reachable candidate
space. `p9_e04`/`p1d` were verified by compile + clippy + unit tests
only (no live rerun in this session, for time; nothing in their
migration touches query semantics, only transport/accounting).

## Scope: what was deliberately NOT migrated, and why

- **`issue55-eval/i55_e14_paired_comparator_freeze.rs`** — its own name
  and `ISSUE55_PAIRED_COMPARATOR_DECISION.md` describe it as a frozen
  reproducibility artifact for a specific historical checkpoint. Left
  untouched rather than risk altering a result already cited as
  independently-reproduced evidence.
- **`issue35-eval/src/eval.rs`** — already the strongest pre-existing
  precedent (hard abort-before-publish, real transport hardening) and
  the basis this session's `comparator-eval` design was generalized
  from. Not rewired to delegate to the new crate in this pass, to avoid
  touching decision-frozen ESCI-vertical checkpoint code
  (`ISSUE35_ESCI_*_DECISION.md`) without a live rerun budget to confirm
  byte-identical output. Named as a follow-up: swapping its private
  `solr_search`/`SolrLookup` for `comparator_eval::solr` is a pure
  transport-layer substitution (same POST shape, same status check) and
  should be a safe, mechanical follow-up once budget allows a
  confirming rerun of all three vertical checkpoints.
- **`phase2-eval/p1d_physical_advantage_eval.rs`'s transport** — the
  denominator-mismatch defect is fixed (see above), but its private
  `solr_search`/`solr_query_for` (returning `qtime_ms`/`num_found`,
  fields `comparator_eval::solr::solr_search` does not currently expose)
  was not replaced with the shared transport, to avoid extending the
  shared crate's return shape for one legacy Phase 2 binary whose
  terminal decision (`PHASE2_DECISION.md`: whole-engine-replacement
  thesis, STOP) is not expected to be rerun for new conclusions.
- **`phase3-eval`'s five NDCG call sites** (`p3e02_coverage_frontier.rs`,
  `p3e03_lexical_narrowing_eval.rs`, `p3e05_structural_anchored_lexical_eval.rs`,
  `p3e06_combined_admission_frontier.rs`,
  `p3e15_ambiguous_plus_lexical_corrected_eval.rs`) — confirmed by the
  audit to carry the identical `unwrap_or_default()`-into-`ndcg_recall_mrr`
  anti-pattern as `p9_e02`, all delegating through the same
  `round1_eval::solr::solr_search`. **Not fixed in this pass** — named
  explicitly here as remaining, real technical debt rather than silently
  left off this document. `round1_eval::solr::solr_search`'s own return
  type would need to change from `Option<SolrResult>` to an
  `EngineLookup`-shaped outcome for these to be fixed properly, which
  ripples into every one of its 9+ callers; deferred to a dedicated
  follow-up rather than rushed in this closure pass.
- **`phase5-eval`/`phase6a-eval`'s four facet/count-correctness clients**
  and **`phase7-eval`'s two throughput probes** — pure duplication, not
  silent-failure risk (`panic!` on transport failure already, loudly).
  Left as named follow-up; consolidating them was lower priority than
  the NDCG-laundering and asymmetric-`fq` defects this pass closes.
- **`issue42-eval`/`phase4-eval`** — consume a frozen CSV artifact, never
  call Solr live. No change needed; noted as a legitimate alternative
  pattern the shared crate does not need to discourage.

## Design for Issue #57

`solr::EngineComparator` is a one-method trait
(`search(&self, q, fq, rows) -> EngineLookup`) that `translate` and
`compare` depend on only through this shape, never on Solr specifically.
An Elasticsearch or Havenask adapter implements the same trait against
its own wire protocol and reuses `translate_constraint`/`translate_all`
(swapping in that engine's own `SolrFieldMap`-equivalent field
declarations) and `PairedComparison` unchanged — the fairness/error
contract Issue #57 requires ("comparator failure can never silently ...")
is enforced once, in the shared crate, not re-derived per engine.

## Verdict

**KEEP.** The shared crate is real, tested (30 unit tests, clippy
clean), and closes the most severe confirmed defect (`p9_e02`'s live
`NDCG=0.0` laundering feeding a headline, decision-relevant verdict) and
the exact asymmetric-`fq` bug class the directive names by construction
(an exhaustive match, not vigilance). Three of five migrated binaries
were confirmed against live Solr + real data to reproduce previously-
published numbers exactly. Full centralization of every Solr-touching
binary in the workspace was not completed in this pass; the remaining
gap is enumerated above rather than left implicit, consistent with this
project's "negative results / remaining gaps are first-class" discipline.
