# Issue #55 checkpoint-14 follow-up — paired comparator freeze (Priority 1A)

Log: `docs/experiments/ISSUE55_PAIRED_COMPARATOR_LOG.md`.

## Verdict: checkpoint 14's cohort was genuinely identical, but its Solr comparator was not fair — the reported reversal (-25.05% → +5.37%) does not survive a fair comparator; corrected picture is -25.05% → -20.49%, still a real, substantial improvement but not a native win. The FastPath-worse/Hybrid-better split IS confirmed and is the more durable finding.

## Question and hypothesis, stated before implementation

`docs/decisions/ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md` (checkpoint 14)
reported `structural_routed` NDCG reversing from -25.05% to +5.37% when
leaf-only `ProductTypeAny` hyponym expansion was wired into production,
but in the same before/after pair **Solr's own NDCG also moved**
(0.3939 → 0.3455) — and the native treatment should never itself change
Solr's ranking. Two explanations were named as needing to be
distinguished: (1) the before/after cohorts were not actually the same
21 queries despite matching counts, or (2) something about the harness
lets the native treatment leak into what Solr is asked, or (3) genuine
Solr-side nondeterminism (this project's own documented
JVM-latency-variance precedent, `ISSUE43_DECISION.md`).

**Root-cause hypothesis stated before running anything**: `p9_e02`
(`crates/phase9-eval/src/bin/p9_e02_wands_physical_advantage.rs`) builds
each query's Solr `(q, fq)` from that same query's *compiled*
`residual_lexical`/`constraints` — downstream of the exact lexicon the
`ProductTypeAny` treatment changes. If the treatment changes what ends
up in `residual_lexical`/`constraints`, the text/filters sent to Solr
change too, so Solr is not actually a frozen, independent comparator
across the pair.

## What was built

1. **`compile_lexicon_with_product_type_hyponyms(profile, min_enum_frequency, enable_product_type_hyponyms: bool)`**
   (`crates/commerce-core/src/cold_start/profile.rs`) — a new public
   toggle, added because there was previously no way to reconstruct the
   pre-checkpoint-14 "hyponyms off" lexicon from current code without
   editing production source. `compile_non_brand_lexicon` now takes the
   bool and skips `product_type_hyponym_groups` entirely when `false`
   (plain per-id `ProductType` matching only). `compile_lexicon` and the
   two other existing lexicon-compilation entry points
   (`compile_lexicon_with_brand_canonicalizer`,
   `compile_lexicon_with_alias_enforcement`) call the new function with
   `true` unchanged — two new regression tests
   (`product_type_hyponym_toggle_true_matches_compile_lexicon`,
   `product_type_hyponym_toggle_false_never_produces_product_type_any`)
   prove `true` is byte-identical (via `Debug` formatting) to
   `compile_lexicon`'s own output and that `false` never emits
   `ProductTypeAny`. This is purely additive infrastructure for
   evaluation tooling, not a production behavior change.
2. **`crates/issue55-eval/src/bin/i55_e14_paired_comparator_freeze.rs`** —
   builds ONE catalog/native-index (shared, treatment-independent) and
   TWO lexicons (`baseline` = hyponyms off, `treatment` = hyponyms on,
   current production) from the exact same `CatalogProfile`. For every
   WANDS query with judgments: compiles and runs `execute_planned` under
   both lexicons, freezes each treatment's own routing-outcome set,
   reports the EXACT query-ID overlap (not just counts), then for every
   query structural-routed under either treatment fires Solr under BOTH
   the baseline-compiled and treatment-compiled `(q, fq)`, 5 times each,
   in the same run, against the same live Solr core. A Solr transport/
   parse failure is excluded from that call's mean, never scored as
   NDCG=0.0 (same rule as `ISSUE35_SOLR_HARNESS_HARDENING_DECISION.md`).

## Findings, in the order the experiment answers them

**1. The cohort is genuinely, exactly identical — not a coincidence.**
`baseline_structural` and `treatment_structural` are both the literal
same 21 query IDs (`{7, 14, 23, 79, 83, 126, 160, 166, 218, 224, 225,
240, 241, 252, 256, 295, 387, 437, 440, 461, 476}`); set difference in
both directions is empty. Every one of the 21 queries also keeps the
same FastPath-vs-Hybrid routing outcome under both treatments (the
`route_chg` column is `-` for all 21 rows). Checkpoint 14's n=21 match
was real, not an artifact of aggregating different populations to the
same count.

**2. Solr itself is fully deterministic in this environment.** Across 5
repeated calls with an IDENTICAL query text (`repeat_runs=5`), mean
per-query NDCG standard deviation is `0.000000` for both the
baseline-query-text and treatment-query-text variants. This directly
rules out "historical Solr ranking drift" (JVM warmup/cache variance,
this project's own `ISSUE43_DECISION.md` precedent) as the explanation
for checkpoint 14's observed Solr NDCG movement — whatever moved Solr's
number, it was not Solr answering the same question differently at
different times.

**3. The Solr comparator query itself changes for 71.4% of the cohort
(15/21 queries).** Confirms the stated hypothesis directly: enabling
`ProductTypeAny` changes `compiled.residual_lexical`/`compiled.constraints`
for most of this cohort, which flows straight into `wands_solr_query_for`'s
`(q, fq)` construction. Solr was never a frozen, independent comparator
across checkpoint 14's before/after pair.

**4. The actual root cause of the Solr-side NDCG movement: a missing
match arm, not a Solr behavior change.** `p9_e02_wands_physical_advantage.rs`'s
`wands_solr_query_for` had match arms for `Category`, `ProductType`, and
`Attribute::Enum`, falling through a `_ => {}` catch-all for everything
else — including `StructuralConstraint::ProductTypeAny`, which did not
exist as a reachable constraint kind against WANDS data when that
catch-all's comment was written, and was never revisited when
checkpoint 14 started producing it in production. **Confirmed
mechanistically, not inferred**: every query where native resolved to
`ProductTypeAny` sent Solr a query with NO product-type filter at all,
silently weakening Solr's own query for exactly the treatment's own
cohort. This is a benchmark-harness defect, symmetric in spirit to
`ISSUE35_SOLR_HARNESS_HARDENING_DECISION.md`'s finding, but a different
failure mode: not infrastructure failure scored as relevance loss, but
an incomplete comparator translation silently under-specifying one
side of a paired measurement.

**Fixed and reran the actual `p9_e02` tool** (not just this checkpoint's
standalone experiment binary), adding the missing `ProductTypeAny` arm
(OR-of-regex alternation across every id in the group, the same
translation the single-id `ProductType` arm already uses). Rerunning
`p9_e02` against the same live Solr core reproduces this experiment's
own treatment-side numbers exactly: `native NDCG@10=0.3641, solr
NDCG@10=0.4579, relative gap=-20.49%` — two independently written
binaries agreeing is a real cross-check, not circular.

**5. Corrected paired picture**: `baseline (hyponyms OFF): n=21, native
NDCG@10=0.2953, solr NDCG@10=0.3939, relative gap=-25.05%` (byte-identical
reproduction of checkpoint 14's own recorded baseline — the harness is
measuring the same thing) → `treatment (hyponyms ON) with the fair
comparator: n=21, native NDCG@10=0.3641, solr NDCG@10=0.4579, relative
gap=-20.49%`. **Checkpoint 14's headline "structural_routed NDCG turns
positive for the first time" does not survive a fair comparator.** The
corrected reading: the `ProductTypeAny` mechanism narrows the relative
gap materially (-25.05% → -20.49%, a genuine ~4.6pp improvement) but
does **not** reverse it into a native win. `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`
is corrected via a dated addendum (see below), not rewritten.

**6. Per-query evidence explains WHY, and it cuts both ways.** The
qualitative sample of the 5 largest `|native NDCG delta|` queries shows
the mechanism is not purely "native gets better while Solr holds
still": query 14 ("beds that have leds") has BOTH native (-0.2495) and
the fairly-filtered Solr (-0.1416) move down together for the same
underlying broadening; query 256 ("high weight capacity bunk beds") has
both move up together (native +1.0000, solr +0.6422). This is expected
and not itself a problem — `ProductTypeAny` is designed to broaden the
structural interpretation, and once Solr is given a comparably broadened
filter it reasonably finds more of the same broader-category products
too. The problem checkpoint 14 had was specifically that Solr was NOT
given that broadening at all (finding 4), not that broadening a
structural interpretation is illegitimate.

**7. The FastPath-vs-Hybrid split named in the governing task IS
confirmed, and is the more durable, better-supported finding** than the
aggregate `structural_routed` number either checkpoint 14 or this
correction reports. Using the treatment's own routing (production
truth) and the fair comparator:

```
FastPath: n=7,  native NDCG@10=0.1583, solr NDCG@10=0.4670, relative gap=-66.11%
Hybrid:   n=14, native NDCG@10=0.4670, solr NDCG@10=0.4533, relative gap=+3.02%
```

FastPath (fully structural execution, no delegate/lexical residual
search) is materially worse than Solr; Hybrid (structural anchor +
bitmap-narrowed lexical delegate) is roughly at parity, slightly ahead.
This is exactly the split the governing task named as "a likely
high-value hypothesis": **structural anchors + lexical residual are
useful; forcing complete structural execution may not be.** `n=7` for
FastPath is small and this is not itself a full falsification test of
that hypothesis (see Next question below) — reported as directionally
confirmed evidence, not proof at this sample size.

**8. Reproducibility**: ran the paired-freeze binary three independent
times; all three produce byte-identical output (`diff` clean). Combined
with finding 2's direct zero-variance measurement, there is no
ordering/index/state artifact in this result.

## Adversarial review

- **Checked whether the `ProductTypeAny` fq translation itself is
  unfairly generous to Solr** (i.e., whether the fix's higher Solr score,
  0.4579 vs. the original 0.3455, could be an artifact of an
  over-broad filter rather than a fair one): the translation is the
  natural symmetric extension of the existing single-id `ProductType`
  arm (an OR of the same per-id case-insensitive regex, one term per id
  in the group) — the same construction a contributor would write if
  asked to extend the existing arm to a multi-id constraint, not a
  novel, hand-tuned filter chosen to produce a particular number.
- **Checked whether the baseline (hyponyms-off) reproduction is
  trustworthy before trusting anything built on top of it**: baseline
  native/solr/gap are byte-identical to checkpoint 14's own recorded
  numbers (0.2953/0.3939/-25.05%), confirming this new harness measures
  the same underlying quantity before its treatment-side correction is
  trusted.
- **Checked whether the fix is itself correct by triangulating two
  independently written binaries**: the standalone paired-freeze
  experiment and the actual `p9_e02` tool (patched with the same fq
  arm, written from scratch rather than copy-pasted verification)
  produce the identical treatment-side number (0.3641/0.4579/-20.49%).
- **Checked whether "Solr is deterministic here" generalizes beyond this
  environment**: it does not claim to — this is a single-node, local,
  freshly-warmed Solr instance; the zero-variance finding is scoped to
  this environment and stated as such, not as a universal claim about
  Solr.
- **Did not claim this closes `structural_routed`'s standing latency
  question**: the fixed `p9_e02` run's own latency ratio (17.98x
  Solr/native, up from checkpoint 14's 1.19x) is a real, disclosed
  secondary effect of the OR-of-regex fq being more expensive for Solr
  to evaluate than a single-term regex — noted, not investigated
  further here, since Priority 1A's scope is the paired relevance
  comparison, not latency.

## Correction to `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`

That decision's own verdict on the `ProductTypeAny` mechanism itself
(KEEP, corrected leaf-only version, false positives gone, recall
retained) is **unaffected** by this finding — this checkpoint did not
re-test that mechanism. What is corrected is specifically the
"`structural_routed` NDCG turns positive for the first time" claim and
its `-25.05% → +5.37%` numbers, which rested on an unfair Solr
comparator. See the dated addendum appended to that decision document.

## Decision

**KEEP** the `compile_lexicon_with_product_type_hyponyms` toggle and the
`ProductTypeAny` Solr-fq fix (both correctness/research-infrastructure
additions, zero production serving-behavior change from the toggle
itself). **REVISE** checkpoint 14's own `structural_routed` reversal
claim per the correction above — the underlying `ProductTypeAny`
mechanism's KEEP verdict stands, but its downstream effect on
`structural_routed`'s aggregate Solr comparison was overstated by an
uncorrected comparator bug.

## Next highest-information question

The FastPath-worse/Hybrid-better split (finding 7) is the most
promising lead this checkpoint surfaces, but at `n=7`/`n=14` on one
vertical (WANDS) it is not yet a confident architectural conclusion.
The natural next falsification test: does this split replicate on a
larger sample (e.g. the full WANDS query set under a policy that routes
more traffic through FastPath/Hybrid, or a synthetic stress workload
designed to populate both buckets at scale) and/or on a second
structurally-rich vertical? If it replicates, it directly supports
narrowing `PlannerPolicy` to prefer Hybrid over forcing FastPath — a
concrete, testable architecture delta, not just a descriptive finding.

## Traceability

Source: `crates/commerce-core/src/cold_start/profile.rs`
(`compile_lexicon_with_product_type_hyponyms`, `compile_non_brand_lexicon`),
`crates/issue55-eval/src/bin/i55_e14_paired_comparator_freeze.rs`,
`crates/phase9-eval/src/bin/p9_e02_wands_physical_advantage.rs`
(`wands_solr_query_for`'s new `ProductTypeAny` arm). Raw evidence:
`docs/research/artifacts/i55_paired_comparator_freeze/`
(`run1.txt`, `run2.txt`, `run3.txt` -- byte-identical,
`p9_e02_after_productypeany_fq_fix.txt`).
