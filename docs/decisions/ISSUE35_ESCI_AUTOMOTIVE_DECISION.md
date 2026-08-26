# Issue #35 — second unseen-vertical slice: real ESCI automotive-parts data

Log: `docs/experiments/ISSUE35_ESCI_AUTOMOTIVE_LOG.md`. Protocol:
`docs/experiments/ISSUE35_ESCI_AUTOMOTIVE_PROTOCOL.md`.

## Verdict: H0 CONFIRMED — checkpoint 13's finding replicates on a second, independent real vertical

`docs/decisions/ISSUE35_ESCI_ELECTRONICS_DECISION.md` found this
project's discovery/serving pipeline generalizes safely to a real
electronics vertical with zero `commerce-core` changes. This checkpoint
tests a second, materially different one -- real Amazon automotive
parts/accessories (1,056 products, 600 queries, fitment/vehicle
semantics none of the prior three verticals carry at all) -- and the
same result holds:

1. **Zero `commerce-core` changes.** The identical measurement code
   (now shared via `issue35_eval::eval::run_vertical_eval`, extracted
   from the electronics checkpoint's own binary and verified to
   reproduce its numbers byte-for-byte before being trusted) ran
   against this new adapter's output unmodified.
2. **Zero unsafe/wrong-family matches** across 37 `Brand`-constrained
   queries.
3. **Relevance clears the bar in the other direction this time**:
   native NDCG@10 0.4396 vs. Solr's 0.4511 (-2.55% relative), still
   comfortably inside the preregistered <=15% bar, but this time
   slightly *worse* than Solr rather than better (electronics: +8.93%).
   Reporting both directions honestly is itself evidence this isn't a
   cherry-picked or systematically favorable comparison.
4. **Brand-collision risk checked again, found absent this time**: 0 of
   502 discovered brand strings collide with the same stopword list
   that caught 1/1,079 in the electronics slice -- consistent with that
   finding being rare and isolated, not a recurring pattern this
   vertical also happens to trigger.
5. **Routing is again heavily `Punt`-dominated** (563/600, 93.8%,
   slightly higher than electronics' 90.2%) for the same reason: no
   product-type/category signal exists in ESCI's data at all. `Brand`
   fires less often here (37/600 vs. 59/600) but with the same
   correctness guarantee.

## Why a second vertical matters more than a second confirmation of the same number

Two independent real verticals now agree on the qualitative claim (safe,
zero-code-change generalization) while *disagreeing* on the specific
relevance direction (native better in one, Solr better in the other,
both within bounds) and on whether the rare brand-collision risk
recurs (it does not, here). That combination -- same architectural
safety property, different quantitative details -- is exactly the
signature of a real, non-overfit finding rather than an artifact of one
particular dataset's quirks. A single vertical replicating its own
exact numbers twice would have been far weaker evidence than two
different verticals landing on the same qualitative conclusion via
different specific paths.

## What this does and does not establish

- **Two of Issue #35's named ">=3 materially different verticals"
  are now done** (electronics, automotive). A third remains for a
  future checkpoint if pursued; this document does not claim the
  Workstream D goal is complete.
- **Does not** run Workstream E (merchant-level heterogeneity within a
  vertical) or Workstream F (a cold-start merchant-profile artifact) --
  out of scope here, as in checkpoint 13.
- **New dataset provenance**: `scripts/datasets/fetch_esci_automotive.sh`
  (same pinned HF revision, independent download) +
  `scripts/datasets/filter_esci_automotive.py` (fixed keyword list) +
  reuses `scripts/datasets/esci_checksums.sha256` (same source parquet
  shard, so the same recorded checksum verifies it).
- **No `commerce-core` production code changed.**

## Adversarial review

- **Checked the refactor's correctness before trusting any new
  number**: reran `esci_electronics_eval` after extracting the shared
  `run_vertical_eval` function and confirmed byte-identical output to
  checkpoint 13's own recorded numbers (routing distribution,
  correctness gate, NDCG to 4 decimal places) -- the shared-code change
  itself introduced no behavior change before it was used to measure a
  second vertical.
- **Checked the automotive slice for the same off-topic keyword noise
  the electronics slice disclosed**: found and reported two concrete
  examples ("Muffler" matching a robe's neck feature, "Seat Cover"
  matching a furniture accessory) rather than assuming a clean slice.
- **Checked whether the brand-collision risk recurs here**: directly
  re-ran the same stopword check against this slice's own 502
  discovered brands -- zero collisions, reported as a real absence, not
  silently skipped because the prior checkpoint already covered the
  topic once.
- **Checked whether reporting native as *worse* than Solr here
  undermines the electronics finding**: no -- both checkpoints' own
  preregistered gate is "within <=15% either direction," not "native
  must win"; reporting the direction honestly in both cases is what
  makes the H0 confirmation credible rather than suspicious.

## Traceability

Source: `crates/issue35-eval/src/eval.rs` (new, shared measurement
procedure), `crates/issue35-eval/src/bin/esci_automotive_eval.rs` (new,
thin wrapper), `crates/issue35-eval/src/bin/esci_electronics_eval.rs`
(refactored to call the shared function, reproduction command
unchanged). Dataset scripts:
`scripts/datasets/{fetch_esci_automotive.sh,filter_esci_automotive.py,solr_index_esci_automotive.py}`.
Raw evidence: `docs/research/artifacts/i35_esci_automotive/run1.txt`.
