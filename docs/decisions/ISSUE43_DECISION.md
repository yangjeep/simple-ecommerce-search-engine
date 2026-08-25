# Issue #43 Decision — Phase 9 determinism re-audit

Full protocol: `docs/experiments/ISSUE43_PROTOCOL.md` (3 dated addenda).
Full log/raw numbers: `docs/experiments/ISSUE43_LOG.md`. Raw artifacts:
`docs/research/artifacts/i43_e00_phase9_determinism_reaudit{,_t3_isolated}/`.

## What was tested

Every currently-published Phase 9 headline number (P9-E01, P9-E02,
P9-E05/E06) was generated **before** commit `449c22f` fixed
`phase9_eval::bitmap_delegate::build_index`'s non-deterministic
multi-threaded Tantivy indexing — confirmed directly via `git merge-base
--is-ancestor`, not assumed from commit dates. `ISSUE38_DECISION.md` had
already flagged this gap explicitly and left it open as Issue #43. This
experiment reran the three affected binaries, unmodified, against current
HEAD (which already contains the fix) using a freshly fetched,
hash-verified real WANDS dataset and a freshly built Solr 9.10.1
baseline.

## Verdict: **KEEP**, with two disclosed corrections and one new open thread

**Phase 9's published relevance/correctness conclusions are confirmed
reproducible against the determinism fix.** Every NDCG-based figure the
fix could plausibly have touched reproduced **byte-identical** across
every independent rerun (3 runs for P9-E02, 6 for P9-E04, each rebuilding
the Tantivy index from scratch):

- P9-E02 structural-routed: native NDCG@10 0.2953 vs Solr 0.3939,
  relative gap -25.05% — identical every run, identical to the
  already-published post-fix figure.
- P9-E04/H1 (isolated ranking quality, identical candidate set): native
  NDCG@10 0.4586 vs Solr-restricted 0.4396, relative gap +4.33% —
  identical across all 12 reruns (both Solr-JVM conditions tested),
  identical to the already-published figure.
- P9-E01's mechanism-level speedup (bitmap vs TermSetQuery delegate
  restriction) reran at 11.49x–12.25x across 6 runs, closely overlapping
  the published 11.56x–11.96x band — ordinary cross-environment wall-clock
  variance, not a qualitative change; still clears the project's >=2x bar
  by a wide margin every run.

No qualitative verdict in Phase 9's decision record (CONFIRMED/FALSIFIED/
REVISE, or the sign of any relative gap) changes. **Issue #43's core
concern — that these numbers were never re-verified against the fix — is
now closed with direct evidence, not just a plausibility argument.**

### Correction 1 — this experiment's own protocol had a citation error

The preregistered T2 KEEP-band text cited stale, pre-P9-E05-fix NDCG
figures instead of the already-published, correct post-fix figures.
Disclosed and corrected in `ISSUE43_PROTOCOL.md` addendum 1, found by an
adversarial review, not the protocol's original author. The actual rerun
result was unaffected (it correctly reproduced the post-fix population);
only the gate's reference text was wrong.

### Correction 2 — P9-E01's determinism claim was mis-scoped

P9-E01's "same document set every run" check is a within-run comparison
(bitmap arm vs termset arm on one index build), not proof of cross-run
indexing determinism. Cross-run determinism is instead established
directly by P9-E02/P9-E04's byte-identical NDCG across independent index
rebuilds. `ISSUE43_PROTOCOL.md` addendum 2.

### New open thread (not an Issue #43 finding) — P9-E04's H3 latency ratio has an undisclosed Solr-JVM-warmup confound

Rerunning P9-E04 immediately after P9-E02 on the same, unrestarted Solr
JVM (0.63x–1.11x) versus on a freshly restarted, cold JVM (1.08x–1.88x)
versus the originally published number (0.42x–0.60x, JVM history
unknown) produced three materially different latency-ratio bands for the
**identical** code and dataset — a >3x swing driven by Solr's own JIT
warm-state, not by anything Issue #43 touches (`p9_e04_isolated_ranking_and_execution.rs`
does not import `bitmap_delegate` or `tantivy` at all). The qualitative
verdict — native fails the project's >=2x speed bar — holds in all 18
measurements across all three conditions, so no downstream conclusion
changes. But P9-E04's specific latency multiplier was never a controlled,
portable measurement, and the original decision record presented one
band as if it were. This is filed as a new methodological gap for any
future P9-E04-derived work (control/record Solr JVM warm-state
explicitly before citing an H3 multiplier again) — not folded into this
issue's verdict, per this repository's rule against combining unrelated
findings into one conclusion.

## Action taken

- `docs/decisions/PHASE9_DECISION.md` gets a confirmation addendum (see
  that file) closing the reproducibility gap it could not close at write
  time.
- `docs/decisions/README.md`'s chronology gains an entry for this
  checkpoint.
- GitHub Issue #43 is closed, referencing this decision and the raw
  evidence.
- The H3/Solr-warmup gap is recorded as an open thread, not a new issue —
  it does not block or change any current architecture conclusion and is
  small enough to fold into any future P9-E04-adjacent work rather than
  tracked separately.

## Architecture delta

None. This experiment does not touch the "can the architecture be
faster/flexible/accurate/stable" thesis directly — it is a reproducibility
audit of evidence already on the books. Its value is restoring confidence
in citations that downstream work (Issue #38, #42, #45, and this
project's own README) already treats as settled.
