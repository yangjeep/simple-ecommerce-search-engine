# Issue #43 Preregistered Protocol — Phase 9 determinism re-audit

Committed before any rerun binary is executed, per this repository's
governance ("preregister hypothesis, baseline, protocol, treatments,
thresholds, splits and stop conditions before held-out results" —
CLAUDE.md item 3; Issue #55's "reproducibility requirements" §1–4). This
document is frozen at commit time; any amendment needed once execution
starts is added as a dated addendum below, before results are read, never
after.

## 0. What this is testing

GitHub Issue #43 reports: `phase9_eval::bitmap_delegate::build_index`
(`crates/phase9-eval/src/bitmap_delegate.rs`) used Tantivy's default
multi-threaded `index.writer(...)` (`min(num_cpus::get(), 8)` threads),
which Tantivy's own source documents as not giving a deterministic `DocId`
allocation. This was caught empirically while rerunning
`issue38-e2e3-eval`'s E2 binary 5 times: one template's mean NDCG differed
between two identical runs (0.5386 vs 0.5869), contradicting this
project's repeated "byte-identical across 5 runs" claim. The fix
(`index.writer_with_num_threads(1, 64_000_000)`) was committed in
`449c22fda2b180d7cb04196529ac26b8d97b5e48` ("Issue #42 pre-merge review
round 2: ... eliminate non-deterministic Tantivy indexing", 2026-08-23
01:09 UTC).

`bitmap_delegate.rs` lives in the `phase9-eval` crate itself and is the
**same module** Phase 9's own binaries import
(`p9_e01_bitmap_vs_termset_delegate.rs`, `p9_e02_wands_physical_advantage.rs`,
and transitively `p9_e04_isolated_ranking_and_execution.rs` via
`p9_e02`'s H1/H3 isolation in P9-E06). `ISSUE38_DECISION.md` states
plainly: "Phase 9's already-published results used the same shared module
before this fix and have not been re-audited (GitHub Issue #43, not
silently left unmentioned)." No decision document records that re-audit
as done. That is the gap this experiment closes.

**Confirmed by direct git ancestry** (`git merge-base --is-ancestor`), not
assumed from commit dates alone:

| Commit | Date (UTC) | What it produced | Contains the determinism fix? |
|---|---|---|---|
| `62b3af7` | 2026-08-22 04:50 | P9-E01 (11.6–12.1x headline) | **No** — predates `449c22f` |
| `bf0f19a` | 2026-08-22 05:11 | P9-E02 (WANDS physical advantage, REVISE) | **No** |
| `2f8d9cbd` | 2026-08-22 06:45 | P9-E05/E06 (compile() fix + re-verdict of H1/H3) | **No** |
| `449c22f` | 2026-08-23 01:09 | the determinism fix itself | — |

`git merge-base --is-ancestor 449c22f bf0f19a` returns false (fix does
**not** predate P9-E02); by transitivity it does not predate P9-E01 or
P9-E06 either, both of which are themselves ancestors of `bf0f19a`'s
lineage or earlier. **Every currently-published Phase 9 multiplier and
NDCG gap was generated with the non-deterministic indexer.** This is a
statement about provenance, not yet about correctness — that is what this
experiment measures.

## 1. Hypothesis

**H0 (numbers reproduce)**: the fix only changes score-tie-break ordering
among documents with *identical* scores within a segment. P9-E01's
decision criterion is a document-set identity check (already required and
verified) plus a latency ratio; tie-breaking does not change which
documents are returned or how long indexing/search takes. P9-E06's NDCG
and latency figures are aggregated over 480 real WANDS queries; if
score-tie collisions are rare enough among the surviving
`structural_routed` population (21 queries after the P9-E05 fix), the
aggregate should not move outside its own previously-reported run-to-run
variance band. Under H0, the published headline conclusions (P9-E01
CONFIRMED ~11.6–12x; P9-E06 H1 FALSIFIED +4.33%, H3 FALSIFIED 0.42x–0.60x)
stand.

**H1 (numbers move)**: this project already has a *demonstrated*
instance of the same bug changing a headline NDCG figure by a
non-trivial margin (0.5386 vs 0.5869 on a different template, Issue #38's
own E2 binary) — so H1 is not speculative. Score ties are plausible
specifically on short, generic-vocabulary WANDS queries where several
titles score identically under BM25-style text ranking, which is exactly
the query population P9-E06 isolates as `structural_routed`. Under H1,
one or more published multipliers/gaps move outside their reported
variance band, or the qualitative verdict (CONFIRMED/FALSIFIED,
sign of the relative gap) flips.

This is a two-sided reproducibility test, not a test of whether the
architecture is good — a KEEP verdict here means "the existing Phase 9
verdicts can now be trusted," not "the architecture won."

## 2. Baseline

Current branch HEAD (already contains `449c22f` unmodified — no code
changes are made by this experiment; it is a rerun for evidentiary
reproducibility, not a fix). `phase9-eval`'s binaries and
`commerce_core::ir::compile` are used exactly as committed.

## 3. Dataset

Real WANDS catalog + queries + judgments, canonical upstream
`github.com/wayfair/WANDS`, pinned commit
`3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5` (same pin `fetch_wands.sh`
already uses — no pin change). Refetched fresh this round (not reused
from a stale local copy — `dataset_cache/` currently holds no raw WANDS
files in this checkout) and hash-verified against
`scripts/datasets/wands_checksums.sha256`. Provenance recorded in full in
§8 once fetched.

## 4. Treatments

1. **T1 — P9-E01 rerun** (mechanism microbenchmark, synthetic, Solr not
   required): `p9_e01_bitmap_vs_termset_delegate`, 6 independent runs
   (matching the original manifest's "3 dev + 3 record" count), each 30
   measured reps/arm with 3-rep warmup (`bench-harness` convention,
   unchanged).
2. **T2 — P9-E02/E06 rerun** (real WANDS, needs a fresh same-run Solr
   9.10.1 baseline): `p9_e02_wands_physical_advantage`, 3 independent full
   480-query runs (matching P9-E06's own rep count for this binary), one
   full discarded warmup pass per engine before each measured run.
3. **T3 — P9-E04/E06 rerun** (isolated H1/H3, identical-candidate-set,
   needs Solr): `p9_e04_isolated_ranking_and_execution`, 6 independent
   runs (matching P9-E06's own rep count for this binary).

All three binaries run **unmodified** — this experiment changes zero
production or eval code. If T2/T3 cannot be run because a reproducible
Solr baseline cannot be stood up in this environment, that is recorded as
an infrastructure gap (§6 REFINE path), not silently skipped.

## 5. Metrics

- T1: mean speedup ratio (bitmap vs TermSetQuery) per run; top-10
  document-set identity per run (must remain exact, as originally
  required).
- T2: routing distribution (FastPath/Hybrid/Punt counts); structural-routed
  NDCG@10 (native vs Solr) and its relative-gap %; structural-routed
  latency ratio (Solr/native).
- T3: H1 relative NDCG@10 gap on the identical-candidate-set comparison;
  H3 latency ratio; candidate-set-size median for the surviving population.
- Cross-run determinism itself: are T1/T2/T3's *own* NDCG/routing figures
  now byte-identical across repeats (the exact claim the original bug
  violated)? Reported explicitly regardless of the KEEP/REJECT outcome
  below — this is itself the direct answer to what Issue #43 asked for.

## 6. Preregistered gates

- **KEEP** (Phase 9's published conclusions are confirmed reproducible):
  T1's speedup lands inside the published 11.56x–11.96x band (or a
  materially overlapping band, allowing for this environment's own
  hardware variance — judged the same way P9-E01's original 6-run spread
  was judged); T2/T3's structural-routed NDCG figures, relative gaps, and
  latency ratios land inside their published bands (T2: NDCG 0.1192–0.1194
  vs Solr 0.1505, ratio 2.25x–2.90x; T3/E06: H1 +4.33% ± a small
  tolerance, H3 0.42x–0.60x, routing split 21/480 structural); no
  qualitative verdict flips. Action: append a confirmation note to
  `PHASE9_DECISION.md` and `docs/decisions/README.md` closing the gap;
  close Issue #43 referencing this experiment; no numbers are rewritten.
- **REJECT/CORRECT** (numbers move materially): any headline figure falls
  outside its published band, the routing-split counts differ, or a
  qualitative verdict flips (CONFIRMED↔FALSIFIED, or the sign/bar-clearing
  of a relative gap changes). Action: preserve the original published
  numbers verbatim (never overwritten), add a dated correction addendum to
  `PHASE9_DECISION.md` presenting pre- and post-correction figures side by
  side, and explicitly enumerate which downstream conclusions (Issue #38,
  #42, #45 lineage; the root README's "~80x-class" and other headline
  claims) cite the now-corrected number and need their own caveat.
- **REFINE** (infrastructure cannot reproduce the original conditions):
  if a fresh, same-run Solr 9.10.1 baseline cannot be stood up
  reproducibly in this environment (download blocked, port conflict,
  etc.), T1 (which needs no Solr) is still run and reported in full; T2/T3
  are recorded as "not independently re-verified this round" with the
  specific blocker logged, and Issue #43 is left open with the narrowed
  remaining scope stated explicitly, not silently declared resolved.

No threshold above is adjusted after results are read. Any change is
recorded as a dated addendum with the reason, before the affected number
is looked at again, per this repository's rule 10 discipline.

## 7. Adversarial review checklist (applied after results, before KEEP is recorded)

- Is the Solr baseline genuinely "fresh, same-run" (freshly re-indexed
  immediately before each measured run), not a stale/warm index from a
  different code path?
- Does the rerun use the identical `PlannerPolicy`
  (`selectivity_threshold=0.05`, `delegate_oversample=20`) and `k=10` the
  original runs used?
- Could a change in `rustc`/`tantivy`/`roaring` crate versions since the
  original runs (not just the determinism fix) explain any observed
  drift? (Check `Cargo.lock` diff against the original run's recorded
  git SHA.)
- Is any observed "improvement" actually a second, different bug being
  silently fixed in the same rerun window, rather than isolating the
  determinism fix specifically?
- Are the reported document-set-identity and byte-identical-determinism
  claims verified by an actual diff of raw per-run output files, not by
  eyeballing summary statistics?

## 8. Dataset provenance (filled in once fetched, per Issue #55's
    requirements — source, owner, license, retrieval date, hashes, row
    counts, transformations, leakage risks, limitations)

- **Source**: `https://github.com/wayfair/WANDS`, pinned commit
  `3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5`.
- **Upstream owner**: Wayfair (WANDS = "Wayfair ANnotation Dataset").
- **License/usage constraints**: public research dataset, no
  authentication required for the raw CSVs; used here identically to
  every prior phase in this repository (Phase 6A onward) — non-commercial
  research/benchmarking use, no redistribution of the raw files
  themselves (only the fetch script + checksums are committed).
- **Retrieval date**: 2026-08-25 (this session).
- **Hashes** (sha256, verified against `scripts/datasets/wands_checksums.sha256`,
  itself unchanged since Phase 6A):
  - `product.csv`: `d993926254572e6eba96c8fd87cc549a17fb91ad3748308036eee4cf92b10ac6`
  - `query.csv`: `63b61660560fecc33ec490804c7e2b81402ee3e7c31a9cbb5e03736639f68e95`
  - `label.csv`: `c11fe81ad62f17f56f316b0ec9630ebe8fbe1393578cb0ca4f05c17253a180ef`
- **Row counts/schema**: 42,994 products, 480 queries, 233,448
  relevance judgments (Exact/Partial/Irrelevant), matching every prior
  Phase 6A/9 record — confirms this is byte-identical to the dataset
  every prior published Phase 9 number was computed against.
- **Transformations**: `scripts/datasets/prepare_wands.py` (unmodified)
  → `dataset_cache/wands/catalog.jsonl` (42,994 records written).
- **Sampling**: none — full catalog and full 480-query set used, as in
  every original Phase 9 run.
- **Train/test leakage risk**: none introduced — this is a rerun of
  existing eval binaries against the same public dataset already used for
  the numbers being re-audited; no new fitting/tuning occurs.
- **Known limitations**: unchanged from the original Phase 9 record —
  WANDS has no real price field (`Price::usd(0)` for every product).
