# Issue #55 — routing-outcome (FastPath/Hybrid) replication across ESCI verticals: decision

Full log: `docs/experiments/ISSUE55_ROUTING_OUTCOME_REPLICATION_LOG.md`.
Raw artifacts: `docs/research/artifacts/i35_esci_{electronics,automotive,beauty}/run{2_routing_breakdown,3_fair_solr_fq}.txt`.

## Governing question

`docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md` (Priority 1A)
named this as its own next question: does the WANDS finding --
`FastPath` native materially worse than Solr (-66.11%, n=7) while
`Hybrid` is roughly at parity (+3.02%, n=14) -- replicate on an
independent vertical, or was it WANDS-specific?

## What was actually found: two separable results

### 1. A real methodology defect in the test harness itself (fixed, KEEP)

Instrumenting the three existing ESCI vertical binaries
(`crates/issue35-eval`) to bucket NDCG by routing outcome surfaced a
second occurrence, in the opposite direction, of the exact defect class
Priority 1A had just fixed for WANDS: `issue35-eval`'s `solr_search` sent
Solr the raw query text only, never the `Brand`/`color` structural
filter native's own `execute_planned` enforces internally for the same
query. This let Solr answer an *easier*, unrestricted-pool question than
native did for every Brand/color-constrained query -- silently inflating
Solr's NDCG on exactly the routing outcomes (Hybrid, FastPath) where
structural constraints concentrate.

**Fixed**: `solr_search` gained an `fq` parameter; `run_vertical_eval`
derives it from each query's compiled `Brand`/`color` constraints using
`round1_eval::solr::case_insensitive_field_regex` (the same
already-reviewed construction reused elsewhere in this project for
exactly this purpose, not re-derived). Verified with two new tests that
capture the raw HTTP request and assert `fq=` reaches the wire only when
a constraint is present -- a real RED-before-fix-style check, not just a
compile check. Full workspace quality gate reran clean (135 test groups,
zero failures).

**Effect size was real**: two of three verticals' Hybrid-bucket relative
gaps moved by 25-27 percentage points (automotive -63.25% -> -38.75%;
beauty -36.48% -> -11.49%; electronics -15.40% -> -12.00%). Beauty's
FastPath bucket moved from -18.00% to -2.62%.

**No prior aggregate KEEP verdict is affected**: only 37-59 of 600
queries per vertical carry a Brand/color constraint at all, so the whole-
vertical aggregate NDCG numbers checkpoints 13/15/16 (`ISSUE35_ESCI_*_DECISION.md`)
based their H0-confirmed verdicts on moved by <0.5 percentage points
(electronics +8.93% -> +9.33%; automotive/beauty unchanged beyond
rounding). This is a correction to a secondary, exploratory
disaggregation this session added, not to a previously published
headline result.

**Verdict: KEEP the fix.** Same disposition as Priority 1A's own fix for
WANDS: a real, disclosed, fixed unfair-comparator bug, verified by a
regression test that would have failed against the pre-fix code.

### 2. The replication question itself: DOES NOT REPLICATE (negative result, preserved as first-class)

With the fair comparator, across all three independent ESCI verticals:

| Vertical | FastPath n, gap | Hybrid n, gap |
|---|---|---|
| WANDS (Priority 1A, for reference) | n=7, **-66.11%** | n=14, **+3.02%** |
| Electronics | n=0 (no evaluated queries routed here) | n=48, **-12.00%** |
| Automotive | n=4, **+45.99%** | n=32, **-38.75%** |
| Beauty | n=8, **-2.62%** | n=38, **-11.49%** |

Neither half of WANDS's own pattern reproduces:

- **Hybrid near-parity does not replicate.** All three ESCI verticals
  show Hybrid *worse* than Solr, not at parity. Two (electronics,
  beauty) land inside this project's usual informal `<=15%` band; one
  (automotive) does not.
- **FastPath-materially-worse does not replicate.** Automotive shows the
  opposite sign entirely (+45.99%, native *better*); beauty is close to
  parity (-2.62%); electronics has no FastPath-routed queries with a
  scoreable judgment at all in its 600-query sample.

**Verdict: REJECT the WANDS routing-outcome split as a general
architecture-level finding.** The corrected evidence base does not
support "FastPath is inherently weaker than Hybrid" (or the reverse) as
a property of the routing mechanism itself. `ISSUE55_PAIRED_COMPARATOR_DECISION.md`'s
own WANDS numbers stand as accurate *for WANDS* (not retracted -- they
were independently reproduced 3x there) but do not generalize to a
claim about the architecture broadly. The most defensible reading of
all four datasets together is that routing-outcome-level relevance is
dominated by per-dataset/per-query-mix idiosyncrasy (WANDS's low-n
FastPath cohort, ESCI's differing catalogs/label distributions), not by
a structural property of FastPath vs. Hybrid execution.

## Why this is not a disqualifying result for the architecture

This negative result concerns a *secondary, exploratory* disaggregation
(does relevance differ by routing outcome), not the *primary,
preregistered* metric each ESCI checkpoint's own protocol gates on
(whole-vertical aggregate NDCG within `<=15%` of Solr). All three
verticals' aggregate H0 verdicts stand, unaffected by either the bug or
its fix. Per CLAUDE.md's own discipline ("Negative results are first-
class outputs. Do not turn a failed gate into a feature roadmap."), this
is recorded as exactly what it is -- a specific secondary claim that
does not survive independent replication -- not folded into, or used to
retract, the primary finding it was never load-bearing for.

## Real caveats, disclosed rather than smoothed over

- **Small n throughout.** FastPath buckets range n=0-8; Hybrid buckets
  n=32-48. None of these support a strong statistical claim in either
  direction; "does not replicate" here means "the specific WANDS
  pattern is not visible in this data," not "definitively falsified at
  scale."
- **Automotive's Hybrid gap (-38.75%) remains large even after the
  fix.** Not investigated further here -- a legitimate next question
  (is this dataset-specific, e.g. automotive's shorter/more technical
  titles interacting with `score_text_relevance`, or a second, still-
  undiscovered comparator asymmetry) rather than assumed away.
- **This was not a preregistered study from the start.** The bug was
  found by re-reading code while investigating an unexpected result
  (exactly the sequence this project's research discipline calls for --
  catch a confound before publishing a conclusion drawn from it), not
  from a pre-committed protocol. The fix itself *was* verified with a
  dedicated regression test before the numbers above were trusted.

## What this does NOT establish

- Not evidence that Hybrid routing is architecturally worse than
  FastPath, or vice versa, in general.
- Not a claim that WANDS's own numbers were wrong -- they were
  independently reproduced 3x under a comparator later found fair for
  WANDS's own defect (`ProductTypeAny`); this checkpoint found and fixed
  a *different* defect in a *different* harness (`issue35-eval`, not
  `phase9-eval`'s `p9_e02`).
- Not a full audit of every possible remaining Solr-comparator asymmetry
  in `issue35-eval` (e.g. edismax field weighting differences, Solr
  analyzer/stemming differences from native's own tokenization) --
  scoped strictly to the specific Brand/color `fq` gap this checkpoint
  found and fixed.

## Next question (named, not implemented here)

Automotive's still-large post-fix Hybrid gap (-38.75%) is the one
concrete open thread this checkpoint leaves: worth a dedicated,
preregistered investigation (does it hold at larger n, is it explained
by a further disclosed asymmetry, or is it a genuine, real relevance
gap this project should treat as a finding about Hybrid on
technical/parts-heavy catalogs) rather than left as an unexplained
outlier.
