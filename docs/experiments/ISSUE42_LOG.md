# Issue #42 Experiment Log — serving-contract decisions and LLM-compiled control plane

Governed throughout by Issue #42's own "do not trust the experiment
author" rule: every conclusion, fixture, query label, and piece of
benchmark code here — including this session's own future output — is
treated as untrusted until independently checked. See
`docs/experiments/ISSUE42_PROTOCOL.md` for the full preregistered
hypotheses, treatments, workloads, and GO-gate thresholds; nothing below
adjusts a threshold after seeing a result.

## Phase 0: frozen baseline

PR #39 (Issue #38 E1-E3, including a second independent pre-merge review
round) ran fully green on its final commit with no unresolved
correctness/methodology issue, and was merged into
`claude/issue-34-phase9-defect-fixes-wands` as merge commit
**`fe2e52e0fe872a0f4ab86c63ccc839e61de8f3e6`** — the immutable E1-E3
baseline every experiment below cites as `baseline_sha`.

## I42-R1: typed ambiguity and corroborated resolution

**Primary research question**: E3/issue #40 measured a real, disclosed
recall gap: `ir::query::compile`'s hard-coded `"size N"` numeric keyword
branch unconditionally resolves to a `Constraint::Numeric` hard filter,
*before* the phrase lexicon ever runs — so it can never discover that
the same literal value (e.g. `"34"`) is *also* a registered `Enum`
candidate for a different product family (apparel jeans sizing vs.
automotive wiper-blade sizing). Can a treatment recover the missed
family without introducing new wrong-family false positives, at
acceptable serving overhead?

**A real methodological correction, made before any treatment ran**:
this protocol's own row 1/2/3 workload text, as originally drafted,
assumed a query text/value that either could never be produced by the
real `apparel`/`automotive` generators or did not actually exercise
`compile()`'s numeric branch at all — caught by direct source reading
(`crates/commerce-core/src/ir/query.rs`), not by running anything
first. Full detail and the correction itself:
`docs/experiments/ISSUE42_PROTOCOL.md`'s "Correction" note under R1's
workload table. In short: apparel's real Jeans sizes
(`["30","32","34","36","38"]`) and automotive's real Wiper Blades size
range (`16..=28`) never numerically overlap, so no value drawn from the
real, frozen generators could ever produce the genuinely-ambiguous case
row 1 needs. Rather than modify the frozen `apparel`/`automotive`
generators backing the merged E1-E3 baseline, R1 uses a small,
purpose-built, fully hand-specified fixture
(`issue42-eval::r1_workload::build_typed_ambiguity_catalog`, 5
products, one per interpretation this experiment needs) — nothing is
sampled, so nothing needs an RNG to be deterministic.

### Hypotheses (from `ISSUE42_PROTOCOL.md`, restated for reference)

- **H1-A**: Treatment A produces at least one wrong-family false
  positive or one missed corroborated match.
- **H1-B**: Treatment B eliminates missed matches but introduces
  wrong-family false positives on at least one adversarial query.
- **H1-C**: Treatment C eliminates false positives but scores
  materially lower NDCG on corroborated queries than D.
- **H1-D**: Treatment D eliminates false positives, recovers
  corroborated queries at high NDCG, correctly falls back for
  genuinely ambiguous uncorroborated queries, at <=5% serving overhead.

### Treatments (implemented in `issue42-eval::r1_experimental`, behind an experimental boundary)

Neither `commerce_core::ir::query::compile` nor
`commerce_core::plan::execute_planned` is modified by any treatment;
each is called as-is.

- **A**: the real, unmodified `compile()`, reused verbatim.
- **B**: keeps *both* the numeric interpretation and any distinct
  lexicon-derived alternative for the same literal token as separate
  hard-constraint sub-queries — realized by calling the real
  `execute_planned` once per interpretation and union-ing the verified
  hit sets (an OR via two real production calls, not a new OR-aware
  executor).
- **C**: demotes every interpretation to a `Preference::Boost`,
  generalizing P9-E05's existing demotion rule (previously applied only
  to lexicon-derived attribute matches) to numeric-keyword-derived ones
  too — unconditionally, regardless of whether an entity constraint is
  present elsewhere in the query. This unconditional behavior is
  deliberate, not an oversight: H1-C's own falsifiable prediction is
  that C does worse than D specifically *because* it never uses
  available corroborating context to select one interpretation: a
  per-interpretation, registration-aware demotion rule would have made C
  behave identically to D, which would make the two treatments
  indistinguishable and untestable as competing hypotheses.
- **D**: like C, but when a corroborating `ProductType` entity
  constraint is present, uses the real materialized catalog (never a
  generator's own bookkeeping) to check which interpretation's
  attribute kind is actually registered on that product type, and
  selects exactly that one as the hard constraint. Falls back to C's
  demotion when no corroborating entity is present, or when
  corroboration doesn't disambiguate cleanly (neither or both
  registered) — a treatment must not fabricate a choice it cannot
  justify from the catalog.

### Workload (9 of the protocol's 10 rows; row 8 covered separately)

Row 8 ("same-token-four-ways") is covered by
`issue42-eval::r1_workload::build()`'s own fixture and unit tests
(already committed, verifying every one of the four interpretations is
genuinely present) rather than by this binary's GO-gate evaluation,
since it is a stress case, not a pass/fail row per the protocol's own
framing.

**Adversarial case added during implementation** (Issue #42 rule 4:
every treatment needs a case *capable of disproving it*): the original
9-row workload had no case that could actually falsify Treatment B — an
uncorroborated ambiguous query only ever found the two *intended*
interpretations in the original fixture, which is the *correct*
behavior for that case, not a false positive. Added a "Bolt Kits"
product carrying an unrelated attribute (`thread_count`) whose value
coincidentally shares the ambiguous value's literal text ("22").
`lexicon_alternatives` returns every Attribute-typed lexicon candidate
registered for a raw token, not only ones literally named `size` — so
Treatment B's "keep every alternative as a hard constraint" mechanism
is expected to incorrectly admit this unrelated product for the
uncorroborated query, a genuine wrong-family/wrong-interpretation false
positive.

Every claimed positive (rows 1/2/3/6) is checked via
`issue42_eval::regression::assert_positive` at binary startup, against
the independent oracle, before any treatment runs — not merely asserted
in this document.

### Results (5 independent runs; correctness numbers byte-identical across all 5, confirmed by direct diff)

| Treatment | corroborated mean NDCG@10 (rows 2/3/6) | wrong-family FPs | row 1 (uncorroborated) | negative rows 9/10 | latency overhead vs A |
|---|---|---|---|---|---|
| A | 0.6667 | 0 | **FAILS** (silently keeps only Numeric as sole hard constraint) | passes | 0.0% (baseline) |
| B | **1.0000** | **1** (Bolt Kits, every run) | passes (returns both interpretations) | passes | 2.6%-6.8% (borderline; moot given the FP) |
| C | 0.3333 | 0 | passes (falls back to demotion) | passes | 42.4%-50.8% |
| D | **1.0000** | 0 | passes (falls back to demotion, no entity present) | passes | **11.6%-21.1%** |

Latency is the median of 7 independent `std::hint::black_box`-guarded
200-call batched trials per row per treatment (E1's own discipline). A
single-batch measurement, tried first, was found — before any verdict
was recorded from it — to occasionally report a *negative* overhead
for Treatment B relative to A despite B provably doing strictly more
work (two real `execute_planned` calls, not one): a clear sign the
single-batch measurement floor had been reached at this fixture's
few-microsecond absolute scale, not a real speed advantage. Taking the
median of several independent batches fixed the sign-flip and produced
a stable, reproducible reading (Issue #42 rule 9 does not apply
retroactively here, since no verdict had been finalized from the
uncorrected numbers).

### Per-hypothesis verdicts

- **H1-A: CONFIRMED.** Treatment A fails on both named grounds: it
  silently resolves the uncorroborated ambiguous query to a single
  hard-filtered family, and it produces zero hits for the corroborated
  apparel case despite "jeans" being present as an entity constraint —
  because the unconditional `Constraint::Numeric` ANDs against
  `ProductType(Jeans)`, and the Jeans product's own `size` attribute is
  `Enum`-typed, so `Constraint::matches`'s type-checked catch-all
  correctly (and silently) rejects it.
- **H1-B: CONFIRMED.** Treatment B recovers every corroborated case
  (NDCG 1.0) but introduces exactly the predicted wrong-family false
  positive on the dedicated adversarial case, every one of 5 runs.
- **H1-C: CONFIRMED.** Treatment C eliminates the false positive (0
  measured) but scores far below the 0.95 bar on corroborated queries
  (0.3333), exactly as predicted — it never uses corroborating context
  to select an interpretation.
- **H1-D: PARTIALLY CONFIRMED.** Every correctness claim in H1-D holds:
  zero wrong-family false positives, corroborated NDCG 1.0, correct
  fallback-to-demotion for the genuinely uncorroborated case. The
  `<=5%` serving-overhead claim does **not** hold: measured overhead is
  11.6%-21.1% across 5 runs, consistently and reproducibly above the
  bar.

### Root cause of D's measured overhead (disclosed, not hand-waved)

`constraint_kind_registered_on_product_type` (`r1_experimental.rs`)
scans every product of the corroborating entity's product type on
*every query*, checking whether the candidate interpretation's
attribute kind is actually present — an `O(products of that type)`
cost paid per query. On this experiment's tiny 5-product fixture this
is a handful of `BTreeMap` lookups, yet still measures as a consistent
double-digit percentage overhead relative to Treatment A's near-instant
`FastPath` call, because A's own absolute cost is itself only tens of
microseconds here. This is almost certainly implementation-cost, not a
property of "corroboration" as a concept: a production implementation
would plausibly precompute a per-product-type attribute-kind registry
once at ingestion time (the same kind of work `CatalogProfile` already
does for its other lexicon data), turning D's per-query cost into a
`O(1)` lookup — but that is not what this experimental implementation
does, and the measured number is reported as measured, not adjusted
for a hypothetical optimization that was not actually built and tested.
Per the protocol's own "known confounders" list, a latency comparison
at this tiny catalog scale is not a reliable stand-in for
production-scale overhead in either direction; this experiment reports
the number it actually measured rather than assuming a larger catalog
would either confirm or refute it.

### GO gate verdict: REVISE

No treatment clears every preregistered threshold. Per the protocol's
own instruction ("If none clear it, record REVISE... naming the
closest treatment and what specifically fails — do not retroactively
loosen a threshold to manufacture a GO"):

**Treatment D is the closest** — the only treatment passing every
correctness, wrong-family, and fallback criterion — and fails solely on
the serving-overhead bar, for a reason (an unoptimized per-query
catalog scan) that is plausibly fixable without changing D's underlying
corroboration mechanism. **Current production behavior (Treatment A,
i.e. `compile()` unmodified) is retained.** D is not adopted in this
pass. A follow-up implementing D's corroboration check as a
precomputed, ingestion-time index (rather than a per-query scan) and
re-measuring is the natural next step if this mechanism is revisited,
but is out of this pass's scope per Issue #42's own "smallest
experiment that can answer the question" discipline — building and
validating that optimization is new implementation work, not a
correction to what was already measured.

### Regression coverage added this pass

- `issue42-eval::oracle` (8 tests, previously committed) and
  `issue42-eval::regression` (4 tests, previously committed) — the
  independent ground-truth layer R1 is built on.
- `issue42-eval::r1_experimental` (7 tests): each treatment's real
  output shape, including that A/B degrade to exactly A's output when
  no size-numeric token is present.
- `issue42-eval::r1_workload` (9 tests): the typed-ambiguity fixture's
  determinism, every claimed positive/negative independently verified
  against the oracle (not asserted in prose), and the Bolt Kits
  adversarial case's own reality (a genuinely distinct attribute, not a
  disguised duplicate of the size collision).
- `r1_typed_ambiguity_eval`'s own measurement logic (8 tests): the
  binary's own `ndcg_at_k`, `row1_does_not_silently_pick_one_family`,
  `negative_row_has_zero_size_hard_constraints`, and `median` helpers
  are independently unit-tested, not merely trusted because they are
  simple — per this project's own "do not trust the experiment author"
  extended to its own measurement code, not just the treatments under
  test.

### A bug this pass caught in its own measurement code before trusting any result

The wrong-family false-positive counter was originally a single global
`usize` shared across all four treatments' evaluation loops — meaning
Treatment B's one real violation would have made every *other*
treatment's GO-gate check report a false failure on that dimension too
(masked in practice this run because Treatment D separately failed on
latency regardless, but a real bug that would have produced a
misleading report under different numbers). Caught by inspecting the
per-treatment printed breakdown against hand-computed expectations,
fixed by tracking false positives in a `BTreeMap<Treatment, usize>`
keyed per treatment before any verdict was finalized from the buggy
version.

A second, smaller correction: the binary's negative-row check
originally applied row 9's strict "zero hard constraints" requirement
to row 10 as well, when the protocol's own row 10 wording only requires
"must not match anything, must not error" — a materially weaker bar (a
harmless `Numeric(size=999999)` constraint that simply matches nothing
is fine). Caught by comparing a "violation" the binary reported against
the protocol's own literal text; fixed by splitting `RowClass::Negative`
into `NegativeZeroSizeConstraint` (row 9) and `NegativeZeroHits` (row
10).

Reproduction: `cargo build --release -p issue42-eval &&
./target/release/r1_typed_ambiguity_eval [output_summary_json_path]`.
Raw artifacts: `docs/research/artifacts/i42_r1_run1/`. Manifest:
`benchmarks/manifests/i42_r1_typed_ambiguity_eval.yaml`,
`artifacts/manifests/i42_r1_typed_ambiguity_eval.json`.
