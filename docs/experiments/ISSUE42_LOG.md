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
| B | **1.0000** | **1** (Bolt Kits, every run) | passes (returns both interpretations) | passes | 0.4%-3.5% (moot given the FP) |
| C | 0.3333 | 0 | passes (falls back to demotion) | passes | 47.4%-52.2% |
| D | **1.0000** | 0 | passes (falls back to demotion, no entity present) | passes | **13.6%-17.8%** |

Latency is the median of 7 independent `std::hint::black_box`-guarded
200-call batched trials per row per treatment (E1's own discipline). A
single-batch measurement, tried first, was found — before any verdict
was recorded from it — to occasionally report a *negative* overhead
for Treatment B relative to A despite B provably doing strictly more
work (two real `execute_planned` calls, not one): a clear sign the
single-batch measurement floor had been reached at this fixture's
few-microsecond absolute scale, not a real speed advantage. Taking the
median of 7 independent batches within one run eliminated that
*within-run* sign-flip, but **run-to-run (separate process invocation)
variance remains real at this catalog's few-microsecond absolute
scale**: an earlier batch of 5 runs, before this pass's diagnostic
section was added (below), measured B's overhead ranging from -6.9% to
14.1% (i.e. the sign-flip still appeared *across* runs, just not
*within* one run's own 7 trials) — disclosed here rather than silently
dropped now that a fresh batch happens to show a tighter, all-positive
range. Neither batch changes D's own conclusion: every individual
measurement of D's overhead across both batches (10 runs total) exceeds
5%, with a combined observed range of 10.2%-18.0%.

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
- **H1-C: CONFIRMED that Treatment C fails its own gate, but the
  reason is NOT what was originally claimed here** — see "Second
  correction round" below. Treatment C, as implemented, does score
  0.3333 (far below 0.95) and does fail the GO gate; that measurement
  is real and reproducible. But an adversarial review found, and a new
  diagnostic measurement confirmed, that this score is not actually
  evidence that "C never uses corroborating context to select an
  interpretation" as originally written — an isolated variant of C that
  differs *only* in not triggering a separate, already-known
  architectural issue (the residual-lexical strict veto, R2's own
  subject) scores a perfect 1.0000, identical to D, on the same
  corroborated rows. R1's fixture, as built, cannot actually distinguish
  "select one interpretation via corroboration" (D) from "demote both
  but don't also trigger the residual veto" as competing explanations
  for corroborated-row NDCG, because every product type in this fixture
  has exactly one candidate product, so the untouched `ProductType`
  entity constraint alone already uniquely identifies it once nothing
  vetoes the query.
- **H1-D: PARTIALLY CONFIRMED.** Every correctness claim in H1-D holds:
  zero wrong-family false positives, corroborated NDCG 1.0, correct
  fallback-to-demotion for the genuinely uncorroborated case. The
  `<=5%` serving-overhead claim does **not** hold: measured overhead is
  13.6%-17.8% in this pass's 5 runs (10.2%-18.0% combined with an
  earlier batch, see above), consistently and reproducibly above the
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
- `issue42-eval::r1_workload` (7 tests): the typed-ambiguity fixture's
  determinism, every claimed positive/negative independently verified
  against the oracle (not asserted in prose), and the Bolt Kits
  adversarial case's own reality (a genuinely distinct attribute, not a
  disguised duplicate of the size collision).
- `r1_typed_ambiguity_eval`'s own measurement logic (9 tests, after the
  second correction round below): the binary's own `ndcg_at_k`,
  `row1_does_not_silently_pick_one_family` (all 4 branches, see below),
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

### Second correction round: fresh adversarial review

Per Issue #42's own governance, a fresh subagent with no implementation
task in this session's history was asked to independently try to
falsify this experiment's protocol, ground truth, code, arithmetic, and
written claims — not to accept anything above on this session's own
say-so. Every finding below was independently reproduced by this
session before being accepted or fixed, per the same rule.

1. **The latency-overhead ranges originally published in this document
   and both manifests did not match the raw run artifacts they cited as
   their source.** The reviewer diffed the actual committed
   `main_run{1..5}.txt` files directly and found the true per-run
   overheads (B: 2.7%, 14.1%, 0.6%, **-6.9%**, **-1.9%**; C: 49.2%,
   47.7%, 47.5%, 35.0%, 46.7%; D: 18.0%, 15.5%, 13.9%, 10.2%, 14.5%)
   did not match this document's previously-published ranges (B:
   "2.6%-6.8%", C: "42.4%-50.8%", D: "11.6%-21.1%") at all — the true
   minimum for D (10.2%) was even below the previously-published floor
   (11.6%), and the previous write-up's B range entirely omitted that 2
   of 5 runs were negative. Independently reproduced by re-reading the
   same committed files directly. This also falsified the "median of
   several independent batches fixed the sign-flip" claim: 2 of the
   same 5 files it cited as evidence still showed a negative overhead
   for B, meaning the *within-run* sign-flip was fixed by the
   median-of-7 correction but *cross-run* (separate process invocation)
   variance was not, and the original write-up should have said so.
   Root cause: the published numbers were transcribed from an earlier,
   uncommitted set of interactive terminal runs, never reconciled
   against the raw artifact files actually committed alongside them — a
   real process gap in how this session moved from "watched the numbers
   scroll by" to "wrote them down as the record." Fixed by regenerating
   a fresh, complete 5-run batch (below) and citing only numbers read
   directly from the files being committed.
2. **A smaller internal inconsistency**: `benchmarks/manifests/i42_r1_typed_ambiguity_eval.yaml`
   stated D's overhead was measured "across 10 independent runs" while
   every other document said 5, and only 5 raw run files exist. Fixed
   to say 5.
3. **A test-count error**: this document claimed `issue42-eval::r1_workload`
   had 9 tests; `cargo test -p issue42-eval --release` and a direct read
   of the file's own `#[cfg(test)] mod tests` block both confirm exactly
   7. Fixed.
4. **The most significant finding**: Treatment C's causal explanation
   was materially incomplete. Tracing the reviewer's claim directly
   (`r1_experimental.rs`'s `resolve_c`, `commerce_core::plan::execute_planned`,
   `phase9_eval::bitmap_delegate::build_index`): `resolve_c` demotes the
   ambiguous constraints to preferences correctly, but *also*
   unconditionally pushes the demoted token into `residual_lexical` —
   which downgrades `plan()`'s routing from `FastPath` to `Hybrid`/`Punt`
   even though the untouched `ProductType` entity constraint alone
   already uniquely identifies the correct product in this fixture.
   Once routed to `Hybrid`/`Punt`, `execute_planned` builds its result
   set exclusively from the lexical delegate's own hits
   (`verify_and_truncate`); the delegate finds nothing for a bare
   numeric token never present in any title/attribute text, so the
   query returns zero results regardless of how good the surviving
   structural constraint is — precisely the strict-veto mechanism R2
   exists to address, confirmed directly in the raw runs (`main_run1.txt`:
   rows 2/3 under Treatment C print `outcomes=["punt"], hits=0`). This
   is not what H1-C's original causal story here claimed
   ("never uses corroborating context to select an interpretation").
   **Independently verified via a new diagnostic**, not merely
   accepted on the reviewer's telling: added
   `resolve_c_isolated_no_residual_push` (`r1_experimental.rs`),
   identical to `resolve_c` except it does not push the demoted token
   into `residual_lexical` — proven, via a new unit test, to differ
   from `resolve_c`'s own output *only* in that one field. Measuring
   this isolated variant's corroborated-row NDCG@10 directly: **1.0000
   — identical to Treatment D**, not 0.3333. This confirms the
   reviewer's finding precisely: essentially all of Treatment C's
   measured NDCG gap vs. D is attributable to the residual-lexical-veto
   confound, not to any actual difference between "select one
   interpretation via corroboration" (D) and "demote both, don't also
   veto via residual" on this fixture. A genuine, previously-undisclosed
   limitation follows from this: **R1's fixture, because every product
   type maps to exactly one candidate product, cannot by itself
   distinguish D's corroboration-based selection mechanism from an
   isolated C that merely avoids the residual veto** — a finer-grained
   fixture (multiple candidates per product type, where picking the
   wrong typed interpretation could retrieve a genuinely wrong
   *additional* candidate, not just zero-vs-one) would be needed to
   test whether D's selection step itself adds value beyond avoiding
   that veto. This limitation, and the diagnostic measurement, are now
   part of this document's own record rather than left as an unstated
   gap. Per-hypothesis verdicts above are updated accordingly; neither
   this finding nor its fix changes the REVISE verdict or which
   treatment is retained, since Treatment C's own preregistered
   measurement (0.3333, with the residual push intact, exactly as
   defined) still genuinely fails the 0.95 NDCG bar regardless of why.
5. **A minor test-coverage gap**: the reviewer noted
   `row1_does_not_silently_pick_one_family`'s `(enum-hard,
   numeric-preference)` branch had no dedicated unit test (only its
   mirror image did) — by inspection the branch appeared correct and no
   treatment in this run exercises it, but per this project's own
   standard that is not a reason to leave it untested. Added
   `row1_check_passes_when_enum_is_hard_and_numeric_survives_as_a_preference`.

All five findings were independently reproduced (by direct file
diffing, `cargo test`, or line-by-line source tracing) before being
accepted; none required overturning the REVISE verdict itself. The
result tables, hypothesis verdicts, and manifests above reflect the
corrected figures; this section keeps the original, now-superseded
figures visible (rather than silently editing them out of existence)
per Issue #42's own "no silent replacement of invalidated numbers"
rule.

Reproduction: `cargo build --release -p issue42-eval &&
./target/release/r1_typed_ambiguity_eval [output_summary_json_path]`.
Raw artifacts: `docs/research/artifacts/i42_r1_run1/`. Manifest:
`benchmarks/manifests/i42_r1_typed_ambiguity_eval.yaml`,
`artifacts/manifests/i42_r1_typed_ambiguity_eval.json`.

## I42-R2: residual lexical semantics

### Hypotheses (from `ISSUE42_PROTOCOL.md`, restated for reference)

- **H2-A**: Treatment A (current strict veto) fails to recover benign
  zero-result cases.
- **H2-B**: Treatment B (unconditional structural fallback) recovers
  benign cases but also over-recovers adversarial ones.
- **H2-C**: Treatment C (residual always advisory/ranking-only) has the
  same over-recovery failure mode as B.
- **H2-D**: Treatment D (compiled residual policy, ingestion-time,
  catalog-statistics-only) recovers benign cases at high rate and
  rejects adversarial cases at low false-recovery rate, with zero
  query-time model calls.

### Treatments (implemented in `issue42-eval::r2_experimental`, behind an experimental boundary)

All four operate on `commerce_core::plan::plan`'s already-computed
routing decision and call only real, public `commerce_core::plan`/
`commerce_core::index` primitives — `commerce_core::plan::verify_and_truncate`
is `pub(crate)` and unreachable from this crate, so wherever a
treatment's correct behavior is identical to A's, it simply calls the
real `execute_planned` again rather than reimplementing verification:

- **A**: the real, unmodified `execute_planned`, reused verbatim
  (`execute_a`).
- **B**: if the delegate's own raw hits (replicated via
  `raw_delegate_hits`, which mirrors `execute_planned`'s exact
  `Hybrid`/`Punt` delegate-call shape) are empty for a `Hybrid`/`Punt`
  outcome with a non-empty residual, fall back unconditionally to the
  structural candidate set alone (`execute_b`).
- **C**: residual lexical text is always advisory — delegate hits, if
  any, only re-order the structural candidate set; if the delegate
  finds nothing, behaves exactly like B (`execute_c`).
- **D**: `ResidualPolicy`, compiled once from the whole `Catalog` at
  "ingestion time" (a lowercased-token → `BTreeSet<ProductTypeId>`
  occurrence map built from title words plus registered Enum/MultiEnum
  values and true-Boolean attribute names), classifies every residual
  token as `Required` (never observed anywhere, the safest default) or
  `Preferred` (observed under the query's own product type, or under
  `CROSS_TYPE_BREADTH_THRESHOLD = 2` or more *other* product types — a
  broadly-used, generically descriptive term). At query time, a
  `Required` token with zero raw delegate hits keeps the query at zero
  results (A's behavior); a `Preferred` one triggers B's structural
  fallback (`execute_d`). Only the two classes the preregistered R2
  workload actually exercises are implemented — `Contextual`/`Unknown`
  are named in the protocol but no row needs entity-dependent unit-word
  handling or an unclassifiable-frequency case, so they are a disclosed
  scope decision, not a silent omission.

### Fixture design: two problems found and fixed before any row was run

R2's fixture (`issue42_eval::r2_workload`, 56 hand-specified products —
Sofas ×2, Boots ×2, Coffee Tables ×1, Bookshelves ×1, plus 50 plain
filler products) is purpose-built, not a modification of the frozen
`issue38_e2e3_eval` generators, for two reasons found by direct source
reading *before* writing any experiment code, both consistent with this
project's "verify against the real mechanism before trusting a fixture"
discipline (first established during R1's own corrections):

1. **The lexicon-auto-resolution trap.** `commerce_core::cold_start::compile_lexicon`
   registers *every* Enum/MultiEnum attribute value and true-Boolean
   attribute name in the catalog as a hard-constraint lexicon
   candidate — unconditionally, with no per-product-type scoping. A
   residual-lexical test word that happened to also be a registered
   attribute value (the original plan: Enum `"purple"`, Boolean
   `"waterproof"`, MultiEnum `"bestseller"`/`"clearance"`) would
   therefore *never* reach `residual_lexical` at all — it auto-resolves
   to a hard (or P9-E05-demoted) constraint before the Hybrid/Punt
   residual-veto mechanism R2 exists to test is ever reached. Every
   test residual word in the final fixture is therefore a plain title
   word only, never a registered attribute value or name — enforced by
   the fixture's own `no_test_residual_word_is_also_a_registered_enum_value`
   test.
2. **The Punt-vs-Hybrid routing trap.** `commerce_core::plan::plan`
   routes a query with a non-empty structural constraint to `Hybrid`
   (delegate search restricted to the structural candidate set via
   `restrict_to`, confirmed by direct read of
   `phase9_eval::bitmap_delegate`'s `BitmapRestrictQuery` to filter
   *inside* the delegate's own scorer) only when
   `structural_candidates / catalog_size <= policy.selectivity_threshold`
   (0.05 throughout this protocol); otherwise to `Punt`, where the
   delegate searches the *whole* corpus with no restriction at all and
   a structural constraint is applied only as a post-hoc filter on
   whatever it returns. With only the original 6 real products, Sofas/
   Boots' own 2-product share (33%) is far above 0.05, so every row
   would have routed to `Punt` — and since the four benign words are
   deliberately *also* present elsewhere in the catalog (the same
   cross-type "broadly used" signal `ResidualPolicy` itself needs to
   observe), a whole-corpus `Punt` search for e.g. "furniture" would
   have found the Coffee Tables/Bookshelves products even when
   restricted-to-Sofas is what the row is meant to test — silently
   replacing R2's intended "delegate found nothing" scenario with an
   unrelated one ("delegate found something, but it belongs to the
   wrong product type"), and never exercising `raw_delegate_hits`'
   `raw.is_empty()` branch at all. Found by tracing `plan`/`execute_planned`'s
   source directly, before running anything — not discovered by a
   surprising experimental result. Fixed by padding the fixture with 50
   plain filler products (distinct titles unrelated to every test word,
   so no catalog-content invariant is disturbed) until Sofas/Boots'
   selectivity clears the threshold with real margin (2/56 ≈ 3.6% <=
   5%), forcing the intended `Hybrid` routing — confirmed directly in
   this run's own "compiled query diagnostics" output (every
   structurally-anchored row shows `outcome=Hybrid`, selectivity ≈
   0.0357).

### Workload

| # | query | class | expectation |
|---|---|---|---|
| 1 | `furniture sofas` | benign | recover the Sofas structural set |
| 2 | `banana sofas` | adversarial | must NOT recover ("banana" observed nowhere) |
| 3 | `waterproof boots` | benign | recover the Boots structural set |
| 4 | `leathr sofas` (misspelling of the real Enum value "leather") | measured only, out of scope for the GO gate | documents current behavior |
| 5 | `bestseller sofas` | benign | recover the Sofas structural set |
| 6 | `velvet boots` | adversarial | must NOT recover ("velvet" observed only under Sofas) |
| 7 | `clearance boots` | benign | recover the Boots structural set |
| 8 | `banana` (no entity at all) | regression guard | every treatment byte-identical to A; `query.constraints.is_empty()` path untouched |
| 9 | `velvet blue sofas` (compound constraint) | adversarial | must NOT recover — added during the second correction round below; see that section |

Deviation from the protocol's illustrative row 5 text ("a real
collection/marketing term absent from every title"): `bestseller` *is*
present in this fixture's Coffee Tables/Bookshelves titles (by
necessary construction — `ResidualPolicy`'s cross-type-breadth signal
requires a real, catalog-observable occurrence somewhere to classify a
token `Preferred` rather than `Required`'s "never observed" default).
"Absent from every title" is read here as "absent from the *target*
entity's own titles" (Sofas), which is what the row's own mechanism
actually needs and is true by the fixture's own tests. Disclosed here
per this protocol's own correction discipline, not silently changed.

Entity-level positive claims (rows 1/2/5/8's Sofas backing set; rows
3/6/7's Boots backing set) are independently re-verified against
`issue42_eval::oracle` at binary startup (`assert_positive`), not
merely asserted in prose. The lexical-content claims (which word
appears in which title, rows 2/4/6/8) are outside `oracle::classify`'s
attribute-based `QueryIntent` scheme by construction — R2 is
fundamentally about free-text residual words, not modeled
`AttributeValue`s — and are instead independently checked by
`r2_workload`'s own direct catalog-content invariant tests
(`banana_appears_nowhere_at_all`, `velvet_is_exclusively_a_sofas_title_word`,
`no_test_residual_word_is_also_a_registered_enum_value`), run by
`cargo test -p issue42-eval`.

### Results (5 independent runs; correctness numbers byte-identical across all 5, confirmed by direct diff)

**These are the final, post-second-correction-round numbers** (9 rows,
3 adversarial), after the fix described in "Second correction round"
below. The originally-published 8-row/2-adversarial-row numbers are
retained, not deleted, in that section per Issue #42 rule 9.

| Treatment | benign recovery (of 4) | adversarial false recovery (of 3) | mean benign NDCG@10 | latency overhead vs A (range across 5 runs) |
|---|---|---|---|---|
| A | 0/4 | 0/3 | 0.0000 | — (baseline) |
| B | 4/4 | **3/3** | 1.0000 | 3.3%–6.1% |
| C | 4/4 | **3/3** | 1.0000 | 4.4%–8.0% |
| D | 4/4 | **0/3** | 1.0000 | 46.8%–51.9% |

Every correctness/recovery/false-recovery number above is byte-for-byte
identical across all 5 independent runs (confirmed by direct diff
excluding the `latency_ms_per_call` JSON block) — this fixture, like
R1's, has zero RNG, so this is expected, not a claimed new finding.

### Per-hypothesis verdicts

- **H2-A: CONFIRMED.** Treatment A recovers 0 of the 4 benign rows.
  Direct diagnostic: each benign row compiles to a `Hybrid` outcome
  with a single `ProductType` structural constraint plus a one-word
  residual; the delegate's own restricted search for that word finds
  nothing (by fixture construction), so `verify_and_truncate` has
  nothing to verify and the query returns zero hits even though the
  entity alone (Sofas or Boots) has real, matching products.
- **H2-B: CONFIRMED.** Treatment B recovers all 4 benign rows (mean
  NDCG@10 = 1.0000) but also recovers *all three* adversarial rows (3/3
  false recovery) — row 2 ("banana sofas", a word observed nowhere),
  row 6 ("velvet boots", a word observed only under a different product
  type), and row 9 ("velvet blue sofas", a compound-constraint case
  added during the second correction round below) are all
  indiscriminately recovered via the same unconditional structural
  fallback that fixes the benign rows.
- **H2-C: CONFIRMED.** On this workload, Treatment C's behavior is
  *identical* to Treatment B's on every row: since the delegate's raw
  hits are empty for every structurally-anchored row here (both benign
  and adversarial), C's `if raw.is_empty()` branch fires every time.
  Its distinguishing mechanism (re-ordering rather than filtering when
  the delegate *does* find something) is real and does produce a
  different result — proven directly by a dedicated unit test added
  during the second correction round
  (`execute_c_reorders_the_full_structural_set_instead_of_filtering_to_only_the_delegates_hits`)
  — but is still never exercised by *this 9-row workload itself*, only
  by that standalone test.
- **H2-D: CONFIRMED, after a real defect found and fixed (see "Second
  correction round" below).** Treatment D recovers all 4 benign rows
  (mean NDCG@10 = 1.0000) and correctly rejects all three adversarial
  rows (0/3 false recovery): "banana" was never observed anywhere in
  the catalog (`Required`, the safest default); "velvet" was observed
  under exactly one product type (Sofas) in this fixture — below
  `CROSS_TYPE_BREADTH_THRESHOLD = 2` — so it classifies `Required`
  regardless of which product type a query names (row 6's Boots query
  *and* row 9's compound Sofas-plus-color query alike, after the fix).
  Zero query-time model/LLM calls: a structural fact (no import of any
  `control_plane::provider::ModelProvider` surface anywhere in
  `r2_experimental.rs`), not merely a measured absence.

### Root cause of D's measured overhead (disclosed, not hand-waved)

Unlike R1, **R2's own preregistered GO gate has no latency threshold at
all** — `ISSUE42_PROTOCOL.md`'s R2 GO-gate section requires only
benign-recovery rate, adversarial-false-recovery rate, and zero
query-time model calls; latency is listed only under "Metrics" (report,
don't gate). This is a real, checked difference from R1's explicit
"<=5%" bar, not an inconsistency this log is glossing over.

D's overhead is nonetheless real and substantially larger than B/C's
(46.8%–51.9% vs. B's 3.3%–6.1% and C's 4.4%–8.0%), and has an
identifiable, disclosed cause in this specific experimental
implementation: `execute_d` calls `raw_delegate_hits` once itself (to
inspect whether the delegate found anything), and then — whenever a
residual token classifies `Required` (rows 2, 4, 6, 9: 4 of this
workload's 9 rows, one more than the originally-published 3 of 8 now
that row 9 also classifies `Required`) — calls the real
`execute_planned` a *second* time as its "stay at zero, like A"
fallback, rather than directly returning the already-known-empty
result. `execute_planned` internally re-runs `plan()` and re-executes
the identical `Hybrid`-restricted delegate search a second time, so
these 4 rows pay roughly double the delegate cost. (Rows 1/3/5/7 do not
hit this path — they classify `Preferred` and call the cheap
`structural_only_hits`/`execute_ranked` instead, the same single extra
call B/C also make; row 8 is deferred to `execute_a` directly, no
duplication.) This is a real, disclosed, plausibly-fixable
*implementation-cost* finding specific to this experimental harness's
"reuse the real `execute_planned` as a lazy correctness-preserving
fallback" choice (made because `verify_and_truncate` is `pub(crate)`
and not reachable from this crate) — not an inherent property of "a
compiled residual policy" as an architectural mechanism. A real
production implementation, with access to `verify_and_truncate`
internally, would know the delegate result is already empty and return
directly, paying the delegate cost once, matching A's/B's/C's own
single-call cost. The overhead range increased from the originally-
published 36.6%–41.3% specifically because row 9 adds one more
`Required`-classified row to this same double-call pattern — a
coherent, verifiable consequence of the fix, not an unexplained
regression.

### GO gate verdict: GO for Treatment D

Per `ISSUE42_PROTOCOL.md`'s R2 GO gate: **GO** requires >=90% benign
recovery (of the 4 preregistered rows — with this small a denominator,
90% is numerically equivalent to "all 4", disclosed rather than hidden
behind the percentage framing), <=1% adversarial false recovery (of the
3 preregistered adversarial rows, after the second-correction-round
addition of row 9 below — equivalent to "zero of 3"), and zero
query-time model calls.

- Treatment A: FAILS (0/4 benign recovery).
- Treatment B: FAILS (3/3 adversarial false recovery).
- Treatment C: FAILS (3/3 adversarial false recovery, identical to B on
  this workload).
- **Treatment D: PASSES every preregistered R2 GO-gate criterion** —
  4/4 benign recovery, 0/3 adversarial false recovery, zero query-time
  model calls (structural). Its latency overhead, while real and
  substantially higher than B/C's, is not a gating criterion for R2 (see
  above) and is disclosed, not hidden. This PASS holds only *after* the
  second-correction-round fix below; before it, D would have failed row
  9 (see that section for the confirmed defect and its fix).

Unlike R1 (REVISE — no treatment cleared every gate), **R2 reaches a
clean GO**: Treatment D's `ResidualPolicy` mechanism is the winning
design. Per Issue #42's own rule ("ship a production behavior change
ONLY when its treatment wins the declared gate"), this is a real
candidate for a RED-before-GREEN production change to
`commerce_core::ir`/`commerce_core::plan`. That change is deliberately
**not** made in this same pass: per this project's own governance, a
fresh, no-implementation-task adversarial reviewer must first attempt
to falsify this protocol/fixture/code/arithmetic/claim (exactly as
R1's second correction round did before R1's REVISE verdict was
finalized), and every confirmed finding independently reproduced and
fixed, before a production change is made on the strength of this
result. The production-change step itself is tracked separately (task
#63, after R3 is also complete) so that R1/R2/R3's serving-contract
decisions are reviewed together rather than merged piecemeal mid-epic.

### Regression coverage added this pass

- `issue42_eval::regression::assert_positive` for the Sofas-alone and
  Boots-alone entity claims (rows 1/2/5/8 and 3/6/7's respective
  backing sets), checked against the actually materialized 56-product
  catalog, not asserted in prose.
- `r2_workload`'s own 5 catalog-content invariant tests
  (`build_is_deterministic`,
  `benign_words_never_appear_in_sofas_or_boots_titles_but_do_appear_elsewhere`,
  `velvet_is_exclusively_a_sofas_title_word`,
  `banana_appears_nowhere_at_all`,
  `no_test_residual_word_is_also_a_registered_enum_value`) — the R2
  analogue of R1's oracle-based regression checks, adapted to check
  lexical catalog content rather than typed attributes, since that is
  what R2's own claims are about.
- `r2_experimental`'s own 6 unit tests (after the second correction
  round below added 2), including
  `treatment_a_is_exactly_the_real_execute_planned_output` (proving
  `execute_a` is not a reimplementation that could silently diverge
  from production behavior), `ResidualPolicy::classify` tests directly
  exercising the `Preferred`-via-cross-type-breadth and
  `Required`-via-never-observed/via-single-type branches,
  `treatment_d_does_not_recover_a_compound_constraint_query_whose_wrong_variant_the_residual_word_would_have_excluded`
  (the confirmed-defect regression test), and
  `execute_c_reorders_the_full_structural_set_instead_of_filtering_to_only_the_delegates_hits`
  (closing a real, previously-total test-coverage gap on Treatment C's
  only distinguishing code path).
- A hard runtime assertion (not merely a printed metric) that row 8 (no
  structural anchor at all) is byte-identical across all four
  treatments — a violation would indicate a treatment's implementation
  touches a code path the protocol requires it not to, which this run
  treats as a bug in the experiment's own code, not a graded result.

### Second correction round: fresh adversarial review

Before any production change was made on the strength of R2's GO
verdict — per Issue #42's own governance, exactly mirroring R1's
second correction round — a fresh reviewer with no implementation task
read the protocol, the write-up, every source file, and the raw
artifacts, and tried to independently recompute or falsify every
claim. It confirmed the fixture ground truth, all recovery/false-
recovery/NDCG arithmetic, the determinism claim, the Punt-vs-Hybrid
selectivity math, `raw_delegate_hits`'s fidelity to `execute_planned`,
`BitmapRestrictQuery`'s scorer-level filtering, the GO-gate-asymmetry
claim (R2 genuinely has no latency threshold, unlike R1), and D's
overhead root-cause explanation — all by direct recomputation or
re-running the binary from source, not by re-reading the write-up. It
found two real issues:

1. **A confirmed defect in `ResidualPolicy::classify`, with a working
   false-positive reproduction.** `classify` accepted only the bare
   `ProductTypeId` a query's `ProductType` constraint names, never the
   query's actual (possibly narrower) structural candidate set. R2's
   own 8-row workload only ever compiled a single `ProductType`
   constraint, so it never exercised a *compound* constraint (e.g.
   `ProductType(Sofas) AND Enum(color=Blue)`) whose real candidate set
   can be a strict subset of every product of that type. The reviewer
   pointed out this leaves a plausible false-recovery mode completely
   untested by Issue #42 rule 4's own "every treatment needs a case
   capable of disproving it" requirement. Independently reproduced by
   writing a direct test (`compile("velvet blue sofas", &lexicon)`
   compiles exactly the compound constraint described, with a real
   candidate set of `{P1}` only — P2, the only product "velvet"
   actually describes, is Purple, not Blue) and running Treatment D
   against it: it returned `[ProductId(1)]` (the Blue Leather Sofa) —
   a genuine false positive, confirmed RED before any fix. Root cause:
   `classify`'s old logic treated "observed anywhere under this query's
   own product type" as sufficient evidence a token is safe to ignore,
   which is true for a broadly-used generic word but not for a token
   that is real and specific to a *different* product within the same
   type. **Fix**: removed that condition entirely, leaving only the
   cross-type-breadth signal (a token observed under
   `>= CROSS_TYPE_BREADTH_THRESHOLD` *other*, distinct product types is
   a safe generic-word signal regardless of how narrow the current
   query's own candidate set is). This did not require the "own-type"
   condition in the first place: re-checking every one of R2's original
   4 benign rows shows all of them already passed via cross-type
   breadth alone (each benign word is observed under 2 non-Sofas/
   non-Boots product types), so the fix changes nothing about the
   originally-published benign-recovery numbers. Added a permanent
   regression test
   (`treatment_d_does_not_recover_a_compound_constraint_query_whose_wrong_variant_the_residual_word_would_have_excluded`,
   RED before the fix, GREEN after) and a new preregistered-style
   adversarial workload row (row 9, `velvet blue sofas`) run through the
   full GO-gate computation above, not merely covered by a unit test
   buried in `r2_experimental.rs`. Also had to rewrite an existing test
   that encoded the old, buggy asymmetric behavior as if it were
   correct (`residual_policy_classifies_velvet_as_required_for_boots_but_preferred_for_sofas`,
   replaced by
   `residual_policy_classifies_velvet_as_required_for_both_boots_and_sofas`)
   — a test that asserts a confirmed defect's behavior is itself a
   defect, not left in place for the sake of "not breaking a passing
   test."
2. **A real, previously-total test-coverage gap on Treatment C.** The
   original write-up disclosed that C behaves identically to B "on this
   workload," which is true, but the reviewer found this actually
   understated the gap: `execute_c`'s only code path that distinguishes
   it from `execute_b` at all (reordering rather than filtering when
   the delegate's raw hits are non-empty) had never been exercised by
   *any* test anywhere in this crate, not just this workload — a
   difference of degree the original disclosure didn't fully convey.
   Fixed by adding a dedicated unit test
   (`execute_c_reorders_the_full_structural_set_instead_of_filtering_to_only_the_delegates_hits`)
   using a small fixed-output stub `LexicalDelegate` (`FixedDelegate`)
   to force a non-empty raw hit deterministically without depending on
   real Tantivy content, proving `execute_c` genuinely returns a
   different (and larger) result than `execute_b`/`execute_a` in that
   case — the full structural candidate set, reordered, rather than
   only the delegate-verified subset.

Both findings were independently reproduced (by direct test execution,
not by trusting the reviewer's description) before being accepted and
fixed. Neither reverses the GO verdict: after the fix, Treatment D
still clears every preregistered R2 GO-gate criterion, now against the
harder 9-row/3-adversarial-row workload the first finding's own fix
demanded. The originally-published 8-row/2-adversarial-row numbers (B
2/2, C 2/2, D 0/2 adversarial false recovery; B 2.8%–5.2%, C
1.7%–4.8%, D 36.6%–41.3% latency overhead) are superseded by, not
silently replaced by, the 9-row/3-adversarial-row numbers in the
Results table above, per Issue #42 rule 9's "no silent replacement of
invalidated numbers."

Reproduction: `cargo build --release -p issue42-eval &&
./target/release/r2_residual_lexical_eval [output_summary_json_path]`.
Raw artifacts: `docs/research/artifacts/i42_r2_run1/`. Manifest:
`benchmarks/manifests/i42_r2_residual_lexical_eval.yaml`,
`artifacts/manifests/i42_r2_residual_lexical_eval.json`.

## I42-R3: identifier serving primitive

### Hypotheses (from `ISSUE42_PROTOCOL.md`, restated for reference)

- **H3-A**: Treatment A (current: product-level lexical delegate only)
  cannot find variant-level identifiers at all.
- **H3-B**: Treatment B (index variant-level Text) recovers exact
  identifier lookup but remains vulnerable to partial/prefix false
  matches.
- **H3-C**: Treatment C (a dedicated identifier dictionary, selected
  only for fields whose measured statistics look identifier-like)
  achieves higher Recall@1 and lower false-match rate than B, with a
  lower incremental update cost, but only for accepted fields.

### Two preregistration corrections, found before any treatment code (both documented, dated, in `ISSUE42_PROTOCOL.md`)

1. **`automotive::generate_catalog` has no seed parameter.** The
   protocol's literal "`generate_catalog(1500)` with
   `SEED.wrapping_add(1000)`" is unachievable — confirmed by direct
   source read, the function always reseeds from the crate's own
   `SEED` constant. Worse, calling it twice at two different `n` would
   make the smaller call a byte-identical *prefix* of the larger one
   (same reseeded RNG, same index-driven generation), silently
   violating calibration/held-out disjointness. Fixed: one
   `automotive::generate_catalog(4500)` call, split by index range into
   disjoint calibration `[0, 1500)` and held-out `[1500, 4500)` slices.
2. **`commerce_core::plan::LexicalHit` cannot express a resolved
   `VariantId` at all**, and `verify_and_truncate`'s per-variant
   resolution is vacuously true whenever `query.constraints` is empty —
   exactly the case for an identifier-only query, since `compile()` has
   no dedicated "part number" keyword branch. Today's pipeline always
   returns a product's *first* variant for such a query, regardless of
   which variant's text a delegate matched. Fixed: every R3 treatment
   is its own self-contained index-and-lookup path returning
   `issue42_eval::r3_experimental::IdentifierHit` (which does carry a
   real `VariantId`) directly, never routed through
   `execute_planned`/`LexicalDelegate` at all.

Full detail for both: `ISSUE42_PROTOCOL.md`'s R3 section.

### Treatments (implemented in `issue42-eval::r3_experimental`)

- **A**: the real, unmodified `phase9_eval::bitmap_delegate::BitmapTantivyDelegate`/`build_index`
  (product-level Text/title only), reused verbatim.
- **B** (`VariantTextIndex`): an experimental per-variant Tantivy index
  — one document per `(product, variant)` pair, with real
  `product_ordinal`/`variant_ordinal` fields stored directly, so a
  match resolves to the exact variant. A general-purpose text index
  with no notion of "this token is a complete identifier."
- **C** (`IdentifierClassifier` + `IdentifierDictionary`): a calibrated
  classifier gating a dedicated exact/normalized-key dictionary. The
  classifier's only input is measured field statistics (uniqueness
  ratio, mean per-value Shannon entropy, whether the field is ever
  variant-scoped) over stringified attribute values *regardless of
  `AttributeValue` variant* — never the field's name, and not even a
  type-based shortcut (an `Enum` field is measured on the same footing
  as a `Text` one, so a low-cardinality Enum is rejected on its own
  statistics, not because it isn't `Text`).

### Calibration (chosen from the calibration set alone, before any held-out number existed)

`compute_field_stats` on the 1500-product calibration catalog measured
19 fields; the three purpose-built ones were `part_number`
(uniqueness_ratio=0.998 — 1497 distinct values across 1500
occurrences; the 3 natural collisions are automotive's own real
`rng.gen_range(1000..9999)` draw occasionally repeating, confirmed by
direct computation, not assumed away), `sku_code` (0.002, mean
entropy=0.0 — a single-character Enum value has no internal character
variation), and `product_fingerprint` (0.00067, mean entropy≈3.84 —
*higher* than `part_number`'s own 2.91, proving entropy alone cannot
separate these two fields). `MIN_UNIQUENESS_RATIO = 0.95` sits with
wide margin between the misleading fields and the real identifier and
is used verbatim on the held-out set, never re-tuned.

### Results (5 independent runs; every correctness/classification/recall/false-match/violation number byte-identical across all 5, confirmed by direct diff)

On the held-out set (3000 real automotive products + 1 hand-built
7-variant stress product), the classifier `ACCEPT`s only `part_number`
and `ABSTAIN`s on all 17 other fields present there — including
`cross_reference_code` (the legitimate-cross-reference case), which has
only 2 occurrences in this fixture, both deliberately sharing one
value: statistically indistinguishable from a low-cardinality field at
that sample size, so the classifier correctly abstains and those
queries fall back to Treatment B, exactly the protocol's own required
abstention behavior, not a failure of the gate.

A self-caught methodology bug, found and fixed before any adversarial
review: the initial Recall@1/false-match sample (2994 un-mutated held-
out identifiers) counted every naturally-colliding identifier pair
(automotive's own generator produces these by chance, same phenomenon
the calibration set's own 0.998 ratio already showed) as a "false
match" whenever the sample happened to iterate to one member of the
pair and the top hit resolved to its real collision-partner instead —
conflating a genuine, correctly-surfaced collision with an actual wrong
resolution. Excluded 22 of 2994 candidates as members of a natural
collision group (disclosed, not silently dropped; collision handling
itself is measured by the dedicated `collision_pair` case, which
passes). On the resulting 2972 genuinely-unique queries:

| Treatment | Recall@1 | false-match rate |
|---|---|---|
| A | 0/2972 (0.00%) | 0/2972 (0.00%) — a miss, not a false match |
| B | 2972/2972 (100.00%) | 0/2972 (0.00%) |
| C | 2972/2972 (100.00%) | 0/2972 (0.00%) |

Corner-case workload, all PASS: the deliberate collision surfaces both
variants under B and C; the absent-identifier variant is never a false
match target; the legitimate cross-reference correctly falls back to B
(classifier abstains) and both products are found; the adversarial
near-miss (single-character edit) is rejected by C; a bare prefix query
is rejected by C but *does* match under B — confirming H3-B's own
predicted weakness directly, not a bug in this experiment; all 7
variants of the many-variant stress product are individually
resolvable via C.

Build/update cost (ranges over 5 independent runs, post-second-
correction-round fix below): Treatment B build ≈12.6–18.4ms, incremental
(one new variant) bimodal — 2 of 5 runs ≈6.5–6.7ms, 3 of 5 runs
≈104.8–107.4ms (not a smooth range: a real, disclosed bimodal split,
plausibly Tantivy's segment-merge policy occasionally triggering
synchronously on `commit()` for a single-document delta — not
independently confirmed against Tantivy's own internals, a genuine
unresolved question, not asserted as settled; see the "variants-per-
product scaling curve" finding below, which shows this same bimodal
split recurring at every tested variants-per-product level, not just
this one held-out-catalog scale). Treatment C build ≈2.3–2.6ms,
incremental ≈0.0008–0.0023ms (a single `HashMap` insert) — consistently
and substantially lower than B's in every one of the 5 runs, satisfying
the GO gate's own "lower build/update cost than B" criterion regardless
of B's own variance. Index size (Treatment B): 129,315 bytes,
deterministic across all 5 runs — **superseding** the originally-
published 129,713 bytes, not silently replacing it: the second
correction round below found the original console print and JSON
summary read `index_size_bytes()` at two different points in the run
(before vs. after the incremental-update section), silently reporting
two different byte counts under the same label; both now read one
consistent post-build snapshot. RSS deltas (B ≈5.8–6.2MB, C ≈0–4KB) are
reported, per the protocol's own text, not gated on.

Lookup latency (P50/P95/P99, median of 7 batched trials, one
representative run): Treatment A ≈11.2–11.8us, Treatment B ≈7.1–7.6us,
Treatment C ≈0.075–0.079us — Treatment C's dictionary lookup is roughly
two orders of magnitude faster than either text-index path, unsurprising
for an O(1) hash lookup vs. a real query-parse-plus-search call.

### Per-hypothesis verdicts

- **H3-A: CONFIRMED.** Treatment A finds 0 of 2972 held-out identifiers
  — `phase9_eval::bitmap_delegate::build_index` never indexes
  variant-level `part_number` at all (product-level `title`/`Text`
  only), confirmed directly by a dedicated unit test
  (`treatment_a_never_finds_a_variant_level_identifier_at_all`).
- **H3-B: CONFIRMED.** Treatment B achieves 100% Recall@1 with 0%
  false-match on the genuinely-unique sample and correctly surfaces
  both variants of the deliberate collision, but the dedicated prefix
  row shows it matching a bare prefix query it should not — a real
  general-purpose-text-index limitation, exactly as hypothesized.
- **H3-C: CONFIRMED.** Treatment C matches B's Recall@1/false-match
  numbers exactly (both are 100%/0% on this held-out set — the
  hypothesis's own "higher Recall@1... than B" is not distinguished on
  this fixture, since B does not have a *lower* Recall@1 here to
  improve on; the differentiator that materializes is C's correct
  *rejection* of the prefix/near-miss adversarial cases B fails, and
  its substantially lower build/incremental-update cost), only for the
  one field (`part_number`) the classifier actually accepts; every
  other field correctly falls back to B via abstention.

### GO gate verdict: GO for Treatment C

Per `ISSUE42_PROTOCOL.md`'s R3 GO gate: Recall@1 >= 0.99 with
false-match rate == 0 on accepted fields (0.9963 → after the
methodology fix, 1.0000; 0.0000), build/incremental-update cost lower
than B's for the same field (confirmed in every one of 5 runs,
regardless of B's own timing variance), no measurable general-lexical
regression, and abstention (not silent misclassification) on every
field the classifier does not accept (17 of 18 non-`part_number` fields
present in the held-out catalog, all correctly abstained).

The general-lexical-regression criterion is now backed by a real
executed check, not merely a structural argument (see the second
correction round below for why the original prose-only version was a
fair target for review): `commerce_core::index::CatalogIndex::execute_ranked`
is run twice, in this same process, against 3 real free-text queries
("Sofas"/"Jeans"/"Brake Pads") over `mixed_merchant`'s 3,000-product
mixed catalog, and the two runs are asserted byte-identical (10/10/10
hits, confirmed identical across all 5 runs). The reason no interaction
with Treatments B/C was ever structurally possible still holds and is
worth keeping precise: `held_out_mixed`'s `Catalog` is a completely
separate object from `held_out.catalog` (the one B/C's own indices are
built over), so this check's real value is confirming the production
ranking pipeline behaves normally and deterministically in this same
run — a genuine, executed confirmation — not proving an interaction was
avoided that had no code path to occur through in the first place.

**Treatment C passes every preregistered R3 GO-gate criterion.**
Mirroring R2's own outcome (and unlike R1's REVISE), this is a real
candidate for a RED-before-GREEN production change — deliberately
**not** made in this pass. A fresh, no-implementation-task adversarial
reviewer must first attempt to falsify this protocol/fixture/code/
arithmetic/claim, exactly as R1 and R2's own second correction rounds
did, and every confirmed finding independently reproduced and fixed,
before any production change is made on the strength of this result.
The production-change step itself remains tracked separately (task
#63), so R1/R2/R3's serving-contract decisions are reviewed together.

### Self-caught issue this pass (found and fixed before any adversarial review)

The natural-collision/false-match conflation described above under
"Results" — caught by directly inspecting the first run's printed
false-match rate (0.37%, non-zero) against the GO gate's own
requirement, tracing every one of the specific mismatches by hand, and
recognizing they were all real, naturally-occurring collision groups
rather than genuine wrong resolutions, before writing up any verdict.
Fixed by excluding catalog-wide-non-unique identifiers from the
Recall@1/false-match sample and disclosing the excluded count directly
in the binary's own printed output, rather than silently narrowing the
sample. A second, unrelated determinism bug was also caught the same
way: `shannon_entropy_bits` summed floating-point terms in `HashMap`
(randomized-per-process) iteration order, producing a last-ULP-level
difference in one field's `mean_entropy_bits` between two runs of an
otherwise-identical binary — caught by diffing 5 runs' summary JSON
before trusting the "byte-identical" claim, fixed by switching to
`BTreeMap`'s deterministic iteration order.

### Second correction round: fresh adversarial review

Before any production change was made on the strength of R3's GO
verdict — per Issue #42's own governance, exactly mirroring R1 and R2's
own second correction rounds — a fresh reviewer with no implementation
task read the protocol, the write-up, every source file, and the raw
artifacts, and tried to independently recompute or falsify every claim.
It found four substantive issues and one minor one:

1. **`IdentifierClassifier::accepts` silently narrowed the protocol's
   own preregistered multi-signal classifier design (uniqueness ratio,
   entropy, format-consistency, collision rate, Product-vs-Variant
   scope, exact-query rate) down to uniqueness ratio alone, with no
   dated deviation note recording the narrowing.** This was a real,
   confirmed defect independent of any numeric consequence — the
   protocol described a multi-signal classifier and the shipped code
   was single-signal, undisclosed.
2. **A concrete numeric consequence of finding 1**: on the calibration
   set, `lumens` (a genuine, non-identifier Numeric attribute —
   Headlight Bulbs' brightness) measured `uniqueness_ratio=0.94`, only
   0.01 below `MIN_UNIQUENESS_RATIO`. The reviewer traced this to sample
   size, not semantic identifier-ness: `lumens`'s ratio drops further,
   to 0.89, on the 2×-larger held-out set. A single-signal classifier
   has no second check to catch a similarly-sampled non-identifier
   field that happens to clear 0.95 by chance — a real, if
   not-yet-triggered, margin risk.
3. **The build-time/incremental-update-cost measurement was only taken
   at a single variants-per-product level** (the one 7-variant stress
   product embedded in the held-out catalog), not "at every tested
   variants-per-product level" as the protocol's own text requires.
4. **The general-lexical-retrieval regression check was argued, not
   measured.** The previous version of this section built
   `held_out_mixed`'s catalog and then discarded it
   (`let _held_out_mixed = ...`) without ever running a real query
   against it — a fair criticism that an argued-not-measured claim reads
   as more rigorous than it is.
5. **(Minor) An index-size measurement-point inconsistency**: the
   console print (evaluated before the incremental-update section ran)
   and the JSON summary (evaluated after) both called
   `index_b.index_size_bytes()`, silently reporting two different byte
   counts under the identical `"index_size_bytes_b"` label.

Every finding was independently reproduced (by direct grep/inspection of
the actual code and run artifacts, not by trusting the reviewer's
description) before being accepted and fixed:

- **Findings 1+2 (classifier narrowing + the `lumens` near-miss).** The
  first candidate fix tried — adding `FieldStats::format_consistency`
  (fraction of a field's values sharing the single most common
  character-class "shape": alphabetic→`A`, digit→`9`, else unchanged)
  as a second, required gate signal — was tested with real numbers
  *before* being committed to, not assumed to work from the protocol's
  description, and was found to **empirically fail**: it scores
  `part_number` (the true positive) at only ~0.51, *lower* than several
  genuine non-identifiers (`product_fingerprint`/`sku_code` both score
  a trivial 1.0), because automotive's own brand and product-type names
  vary in word count (`"TrueDrive"` → a one-letter code;
  `"Ironclad Auto"` → a two-letter code), so `part_number`'s own
  brand/type-code segments genuinely vary in length across the catalog,
  spreading its occurrences across multiple signatures with no single
  majority. An equivalent fixed-length-ratio variant was also tried and
  also failed (`part_number`≈0.56). Gating on either would have
  **incorrectly rejected the real identifier field — a regression, not
  a fix.** Per `CLAUDE.md`'s "record failed experiments," this negative
  result is kept, not erased: both statistics remain computed and
  reported (`FieldStats::format_consistency`) for transparency, but
  neither gates. The fix that *does* work, found next: `variant_scoped`
  (already computed by `compute_field_stats`, but — per the same review
  — never actually read anywhere before this fix) discriminates
  correctly for a structural reason, not a numeric coincidence:
  automotive's generator sets `part_number`/`warranty_months`/
  `compatible_fitment` directly on each `Variant`, and every other
  attribute (including `lumens`) only on the parent `Product`.
  `IdentifierClassifier::accepts` now requires
  `uniqueness_ratio >= MIN_UNIQUENESS_RATIO && variant_scoped`.
  Verified (via a temporary diagnostic, deleted after use) that this
  produces **identical classification results** to the original design
  on both the calibration and held-out sets — `part_number` still
  `ACCEPT`s, every other field still correctly `REJECT`s/`ABSTAIN`s,
  including `lumens` (now rejected structurally rather than by a
  numeric margin) — because `warranty_months` (also variant-scoped) is
  already independently rejected on uniqueness ratio alone
  (0.0027/0.0013), so this addition introduces no new false accept. New
  regression test:
  `identifier_classifier_rejects_lumens_a_genuine_near_miss_on_uniqueness_ratio_alone`.
- **Finding 3 (variants-per-product scaling curve)**: added
  `r3_workload::build_scaling_catalog(n_products, variants_per_product)`
  — a small, self-contained synthetic catalog builder, deliberately
  disjoint (a third id range, `SCALING_PRODUCT_ID_BASE`/
  `SCALING_VARIANT_ID_BASE`) from every other id range this fixture
  hands out — and a new scaling-curve section in the eval binary that
  builds a fresh, dedicated 200-product catalog at each of 4 levels
  (`variants_per_product ∈ {1, 3, 7, 15}`; `7` deliberately included so
  this curve's own middle point is directly comparable to the single
  stress-product measurement already taken on the main held-out
  catalog), measuring Treatment B/C build-time and incremental-update
  cost at each level independently (never sharing a catalog/index
  across levels, so no level's timing is confounded by another level's
  data still being present). Result: Treatment C's build time scales
  roughly linearly with total variant count as expected of a per-entry
  `HashMap` insert (≈0.09–0.22ms at 200 variants up to ≈0.8–2.0ms at
  3,000 variants, across 5 runs); Treatment C's incremental-update cost
  stays consistently tiny (≈0.0007–0.004ms) at every level, with no
  bimodal behavior. Treatment B's build time is noisy at these small
  sizes (dominated by Tantivy's own per-commit overhead, not a clean
  function of variant count) and — genuinely informative — **the same
  bimodal incremental-update split already disclosed at the main
  held-out-catalog scale recurs at every one of the 4 tested
  variants-per-product levels**, not correlated with the level itself:
  across the 20 (level × run) cells measured, roughly half land
  ≈3–8ms and half ≈104–108ms, with no level showing only one mode.
  This strengthens (without proving) the existing "Tantivy's own
  segment-merge policy occasionally triggering synchronously on a
  single-document commit" hypothesis, since it rules out
  variants-per-product itself as the trigger. New regression tests:
  `scaling_catalog_has_the_requested_shape_and_distinct_identifiers`,
  `scaling_catalog_ids_never_collide_with_the_other_two_extension_ranges`.
  The GO gate itself continues to gate on the original single
  held-out-catalog build/incremental numbers, matching the
  preregistered gate text — this curve is measured and reported for
  protocol completeness, not a second, parallel gate.
- **Finding 4 (general-lexical-regression argued-not-measured)**:
  extended `HeldOutMixed` to expose the real `product_types`/`brands`/
  `categories` `ingest::build_catalog` already returns (previously only
  `catalog` was kept), then replaced the discarded-catalog prose with a
  real, executed check: `commerce_core::cold_start::CatalogProfile::build`
  + `compile_lexicon`, then `commerce_core::ir::compile` +
  `CatalogIndex::execute_ranked` run twice against 3 real free-text
  queries, asserting byte-identical hit counts (10/10/10, confirmed
  across all 5 runs) — see the GO gate verdict section above for the
  precise, re-written disclosure of why this check's real value is
  confirming normal production behavior in-process, not proving an
  interaction was avoided that had no code path to occur through in the
  first place.
- **Finding 5 (index-size measurement-point inconsistency)**: fixed by
  capturing `index_size_bytes_b_post_build` once, immediately after
  Treatment B's build and before the incremental-update section runs,
  and reading that one snapshot in both the console print and the JSON
  summary. The corrected, consistent value (129,315 bytes) supersedes
  the originally-published 129,713 bytes (see "Results" above) — not a
  silent replacement, since both are recorded and the reason for the
  discrepancy (two different measurement points, not measurement
  noise) is disclosed.

All fixes were re-verified via `cargo test -p issue42-eval --release r3`
(13 tests, all passing — 10 pre-existing plus 3 added this round: the
new `lumens`-rejection test and the 2 new scaling-catalog tests) and by
re-running the full binary. **The GO verdict
for Treatment C survives**: Findings 1+2's fix only makes the classifier
*more* conservative (adds a second required condition) in a way already
verified to preserve every existing classification decision, so it
cannot itself flip any accept/abstain outcome; Finding 3 adds evidence
without changing any existing number; Finding 4 replaces an
argued-but-unmeasured claim with a measured-and-passing one; Finding 5
is a reporting-consistency fix, not a change in underlying behavior. All
5 regenerated runs remain byte-identical on every correctness/
classification/recall/false-match/violation field and on every
non-timing field of the new scaling curve (`dictionary_entry_count_c`,
`variants_per_product`/`products`/`total_variants` per level), confirmed
by direct diff; only timing/RSS/index-size-adjacent fields vary
run-to-run, as before.

Reproduction: `cargo build --release -p issue42-eval &&
./target/release/r3_identifier_primitive_eval [output_summary_json_path]`.
Raw artifacts: `docs/research/artifacts/i42_r3_run1/`. Manifest:
`benchmarks/manifests/i42_r3_identifier_primitive_eval.yaml`,
`artifacts/manifests/i42_r3_identifier_primitive_eval.json`.

## I42-Merge: R1/R2/R3 serving-contract decisions reviewed together, evidence-supported changes merged into `commerce_core`

Per Issue #42's own sequencing rule, no experiment's GO verdict was acted
on in production code until all three (R1/R2/R3) could be reviewed
together, here:

- **R1 (typed ambiguity and corroborated resolution): REVISE.** No
  treatment cleared every preregistered gate (Treatment D fails only the
  latency bar). Per Issue #42's own rule ("ship a production behavior
  change ONLY when its treatment wins the declared gate — otherwise
  record REVISE/INCONCLUSIVE"), **no production change was made for
  R1.** Current production behavior (`commerce_core::ir::query::compile`'s
  unmodified ambiguity handling) is retained unchanged.
- **R2 (residual lexical semantics): GO for Treatment D.** Merged.
- **R3 (identifier serving primitive): GO for Treatment C.** Merged.

### What was merged

**R2 (`docs/adr/0012-residual-lexical-policy.md`)**: a new
`commerce_core::plan::residual` submodule (`ResidualPolicy`/
`ResidualClass`, ported from `issue42-eval::r2_experimental` with the
eval prototype's own proven-dead `_product_type` classify parameter
dropped from the production signature). `execute_planned` gained one
new, additive, trailing parameter, `residual_policy:
Option<&ResidualPolicy>` — `None` (every pre-existing call site) is
byte-identical to prior behavior; `Some` lets a `Hybrid`/`Punt` outcome
with zero raw delegate hits fall back to the structural candidate set
instead of collapsing to empty, but only when a corroborating
`ProductType` constraint is present **and** every residual token
classifies `Preferred` (a real defect this exact interaction between
"corroborating constraint" and "classify's own signal" was found and
fixed twice — once during R2's own second correction round, and once
more during this production merge's own review, below).

**R3 (`docs/adr/0013-identifier-serving-primitive.md`)**: a new
`commerce_core::index::identifier` submodule (`FieldStats`,
`compute_field_stats`, `IdentifierClassifier`, `IdentifierDictionary`,
ported from `issue42-eval::r3_experimental`). `CatalogIndex::build` now
computes per-field statistics and builds a dictionary for every field
the classifier accepts (`uniqueness_ratio >= 0.95 && variant_scoped`,
R3's own fully-corrected condition), **plus one new safeguard added
during this production integration and disclosed as such, not part of
R3's own experimental evidence**: `MIN_IDENTIFIER_SAMPLE_SIZE = 100`,
since R3's own calibration/held-out catalogs (1,500+ products) never
exercised the real risk of a tiny hand-authored test catalog spuriously
accepting a field on small-sample noise alone. `plan::LexicalHit`
gained one new, additive field, `variant: Option<VariantId>` (every
existing delegate constructs `None`); `verify_and_truncate` now prefers
a delegate-named variant when present, but only if it also satisfies
every constraint, never falling back to a different variant of the
same product on failure. `execute_planned`'s `Hybrid`/`Punt` arms try
an exact identifier-dictionary lookup across `query.residual_lexical`
*before* calling the delegate at all, re-verifying every candidate
against `query.matches_variant`/`restrict_to` exactly like any other
hit — never a correctness bypass.

`commerce_core::admission.rs`'s separate, structurally unrelated
native-vs-Solr strict-veto functions (`admit`/`admit_lexically_narrowed`/
`admit_structurally_anchored_lexical`) were explicitly out of scope for
both merges and were left untouched — verified directly (`git diff`
shows zero changes to that file or its tests).

### How the merge was executed and reviewed

Both merges were implemented against the real, exact current
`commerce_core::plan`/`commerce_core::index` source (not assumed from
memory), each as its own focused change with new regression tests
before being considered complete, followed by a fresh, no-implementation-task
adversarial review of the combined result — the same "do not trust the
author" governance already applied three times to R1/R2/R3 themselves,
now applied a fourth time to the act of merging their conclusions into
production. The reviewer independently re-ran the full quality gate
from scratch (not trusting a prior "all green" claim), traced every one
of the 34 `execute_planned` call-site migrations and 15 `LexicalHit`
construction-site migrations by file:line, and specifically hunted for
undisclosed behavior changes, scope creep into `admission.rs`, and any
path by which an identifier-dictionary or named-variant hit could
bypass `query.matches_variant`. It found two confirmed defects, both
independently reproduced and fixed before this checkpoint:

1. **ADR 0012 misreported its own call-site migration count** ("13
   existing call sites across 11 files," when the ADR's own itemized
   list, plus `r2_experimental.rs`'s 9 sites disclosed separately, sums
   to 34 — the "11 files" claim was correct, only the site count was
   wrong, by more than 2x). Purely a documentation/disclosure defect —
   every one of the 34 real sites was independently re-verified (by
   direct grep, both before and after the fix) to actually pass `None`
   correctly; no behavior was ever affected. Fixed by correcting the
   ADR's text to the verified count (34) with the arithmetic shown
   explicitly, and disclosing the original error rather than silently
   replacing the number (Issue #42 rule 9).
2. **`commerce-core`'s own `residual_policy_catalog` fixture
   (`crates/commerce-core/src/fixtures.rs`) could never compile a
   *compound* structural constraint at all** — every product was built
   with `attributes([])`, so no test in `commerce-core`'s own suite
   could reproduce the exact shape (`ProductType(Sofas) AND
   Enum(color=...)`) R2's own second correction round needed to catch a
   real false positive. Production's `ResidualPolicy::classify` no
   longer even accepts a `ProductTypeId` parameter, so the *specific*
   old bug cannot be reintroduced through `classify` itself — but the
   reviewer correctly noted this left the *observable* behavior
   (`residual_fallback_hits`'s structural recovery respecting a
   compound constraint's full, narrowed candidate set — not just the
   bare `ProductType`) completely unguarded by `commerce-core`'s own
   test suite, dependent entirely on `issue42-eval`'s separate, frozen
   historical test to ever catch a future regression in this area.
   Fixed by giving `residual_policy_catalog`'s Sofas products real
   `color`/`material` Enum attributes (matching
   `issue42-eval::r2_workload::build()`'s own shape) and adding a new
   regression test,
   `plan::tests::residual_fallback_respects_a_compound_constraints_full_narrowed_candidate_set`
   — an all-`Preferred` residual word ("furniture") plus a compound
   `ProductType(Sofas) AND Enum(color=Blue)` constraint must recover
   *only* the Blue Leather Sofa, never the Purple Velvet Sofa, which
   satisfies the bare `ProductType` constraint alone but not the
   compound one. (The test's first draft used "velvet" as the residual
   word, matching R2's own original adversarial case — that draft
   failed for an instructive reason, not a defect: "velvet" is
   observed under only one product type in this fixture, so it
   classifies `Required` regardless of any product type, meaning it
   never reaches the compound-constraint-sensitive code path at all;
   corrected to use "furniture," the fixture's genuinely `Preferred`
   cross-type word, which does.)

Both findings were independently reproduced (by direct grep/computation
and by writing/running the corrected test, not by trusting either the
implementing agents' or the reviewer's own description) before being
accepted and fixed. Every other item the reviewer checked — call-site
behavior preservation, cross-variant correctness (no unverified
identifier or named-variant hit can bypass `matches_variant`), the
`MIN_IDENTIFIER_SAMPLE_SIZE` safeguard's own RED-before-GREEN test,
`admission.rs`'s isolation, ADR 0013's accuracy, and R1/R2's own
existing tests being functionally untouched (only mechanical `,
None`/`variant: None` additions, confirmed directly by `git diff`) —
checked out clean.

### Final verification

After both fixes: `cargo fmt --all -- --check` clean; `cargo clippy
--workspace --all-targets --all-features -- -D warnings` clean;
`cargo test --workspace --all-features` — every test binary green, 0
failures, including `commerce-core`'s lib suite (52 passed — 46
pre-existing plus the 4 R2 regression tests, 3 R3 regression tests, and
1 new compound-constraint test, minus overlaps already counted by the
merge phases) and `tests/plan.rs` (10 passed); `cargo build --workspace
--release` clean. `cargo test -p issue42-eval --release` confirms every
one of R1/R2/R3's own historical tests — the actual evidence artifacts
this whole merge is based on — still passes completely unchanged.

Full detail: `docs/adr/0012-residual-lexical-policy.md`,
`docs/adr/0013-identifier-serving-primitive.md`. This section is itself
the decision record Issue #42 rule 9 requires for a production change
made on the strength of a GO verdict — R2's and R3's own sections above
remain the primary experimental evidence and are not restated here.
