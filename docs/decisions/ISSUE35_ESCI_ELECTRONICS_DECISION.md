# Issue #35 — first-pass unseen-vertical slice: real ESCI electronics data

Log: `docs/experiments/ISSUE35_ESCI_ELECTRONICS_LOG.md`. Protocol:
`docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`.

## Verdict: H0 CONFIRMED for this slice — the pipeline generalizes safely to a genuinely new vertical with zero code changes; one rare, disclosed precision risk named

**Scope, restated**: this is a deliberately small, single falsifiable
slice of Issue #35's much larger epic (Workstream B/D's core question,
on one real vertical, without the epic's own methodology-freeze/scoring-
rubric/3-vertical/merchant-heterogeneity machinery). It is the first
time in this project's history that `CatalogProfile`, `compile_lexicon`,
`CatalogIndex`, and `execute_planned` have been run against data from a
vertical genuinely different from WANDS (home/furniture) and the
Magento fixture (apparel) — real Amazon electronics/components listings
via a filtered ESCI slice (2,075 products, 600 queries).

**Result: the unmodified pipeline handled it safely.**

1. **Zero `commerce-core` changes required.** The exact same
   `CatalogProfile::build`/`compile_lexicon`/`CatalogIndex::build`/
   `compile()`/`execute_planned` functions already used for WANDS and
   Magento ran against this new adapter's output with no edit, no
   panic, no crash. This directly refutes the most severe Issue #35
   falsification criterion for this slice: "representation requires
   vertical-specific serving code" / "unseen verticals require bespoke
   architecture" did not occur here.
2. **Zero unsafe/wrong-family matches.** Every one of the 59
   `Brand`-constrained queries' hits carried the exact required brand —
   checked directly against the built catalog, not assumed.
3. **Native relevance is competitive, in fact ahead**: NDCG@10 0.3041
   vs. a real Solr baseline's 0.2792 (+8.93% relative), comfortably
   inside the preregistered <=15% bar. Spot-checked directly: Solr's
   own top hits for a brand-specific query ("dell monitors") were not
   even Dell products, confirming native's structural `Brand` narrowing
   is doing real, measurable work on this vertical, not that the
   comparison was accidentally handicapping Solr.
4. **The specialization-boundary finding Issue #35 explicitly allows**:
   routing is 90.2% `Punt` (541/600), because this vertical's real data
   carries no product-type/category signal at all — there is very
   little for `FastPath`/`Hybrid` to specialize on beyond `Brand`. This
   is disclosed as a legitimate finding about *this vertical's own
   specialization potential*, not a defect: Issue #35's own text states
   "the methodology is explicitly allowed to conclude a vertical or
   merchant is not worth specializing." Brand-based structural
   narrowing, the one axis this vertical's data does support, both
   fires (59/600 queries) and measurably helps (point 3).

**One genuine, disclosed, rare precision risk, found by directly
investigating the qualitative sample rather than accepting the
aggregate NDCG number as sufficient**: a single off-topic query
spuriously matched a `Brand` constraint because a real product in this
slice carries the literal brand string `"IS"`, colliding with the
common English word "is." Quantified: 1/1,079 discovered brands (0.09%)
is an exact stopword collision. This does not violate the correctness
hard gate (it produces zero results, not a wrong one), but it is real
evidence that this project's brand-discovery mechanism — safe on
WANDS's single-curated-retailer data, where brand names bypass
`min_enum_frequency` filtering by design (`crates/commerce-core/tests/cold_start.rs`'s
own "brand/product-type resolution must be unaffected by
min_enum_frequency" test) — carries a small but real false-positive
risk on noisier, open multi-seller marketplace metadata, where any
seller-entered string can become a `product_brand` value. This is
exactly the kind of "generalization failure taxonomy" entry Issue #35
asks to be preserved, not smoothed over.

## Why this counts as real evidence, not a foregone conclusion

Nothing about `commerce-core`'s architecture guaranteed this outcome in
advance. A plausible failure mode existed and was checked for directly:
a brand-discovery mechanism trusted unconditionally (no frequency
filter, unlike attribute enum values) could plausibly have produced
*many* false-positive structural constraints on messier marketplace
data, not just the one found here — that would have been a genuine,
serious falsification of "no vertical-specific code needed." It did
not: only 1 of 1,079 real discovered brand strings collided with common
English, and the collision produced a safe (zero-result), not unsafe
(wrong-result), outcome. The finding is disclosed at the actual measured
rate, not asserted as either "never happens" or hidden.

## What this does and does not establish

- **Does not** satisfy Issue #35's own "Definition of done" checklist
  (frozen domain-neutral representation, methodology freeze before
  scoring, >=3 unseen verticals, merchant-level heterogeneity, a
  cold-start artifact, etc.) — this is one slice of one workstream on
  one vertical, explicitly scoped as a partial contribution.
- **Does** provide the first real, disclosed evidence for this
  project's "no vertical-specific serving code" architectural claim
  beyond WANDS and a tiny apparel fixture, on a vertical whose data
  shape (no category field, noisy open-marketplace brand strings) is
  genuinely different from both.
- **Names, without implementing**, the concrete follow-up the brand
  collision reveals: extending `min_enum_frequency`-style trust
  filtering (or a minimum-length/stopword check) to brand-name
  discovery specifically when ingesting uncurated marketplace data —
  not needed for WANDS (where it would change nothing, since Wayfair's
  own `brand` field has no such collisions) but a real, disclosed gap
  for any future merchant/vertical whose data resembles ESCI's rather
  than WANDS's.
- **New dataset provenance**: `scripts/datasets/fetch_esci_electronics.sh`
  (pinned to HF revision `45c948250c2116f1e535bac67b92501c695307a4` of
  `tasksource/esci`, Apache-2.0) +
  `scripts/datasets/filter_esci_electronics.py` (fixed keyword list,
  fixed caps, disclosed before any metric was inspected) +
  `scripts/datasets/esci_checksums.sha256` — fully reproducible, matching
  this project's own `fetch_wands.sh`/`wands_checksums.sha256` pattern.
- **No `commerce-core` production code changed.** All new code is in
  `crates/issue35-eval` (a new eval crate) plus dataset-acquisition
  scripts.

## Adversarial review

- **Checked whether the Solr comparison was fair, not accidentally
  handicapped**: directly inspected Solr's own raw top-3 hits for a
  brand-specific query ("dell monitors") — non-Dell products, confirming
  plain edismax lacks brand-exactness bias and native's win is a real
  structural-narrowing effect, not a Solr misconfiguration artifact.
- **Checked whether the routing distribution's `Punt` dominance was
  itself evidence of a problem (e.g., the lexicon failing to discover
  real structure)**: no — cross-checked against the data itself (ESCI
  genuinely carries no product-type/category field for any row), so a
  ~90% `Punt` rate is the correct, expected outcome given the data, not
  a discovery-pipeline defect. `Brand`, the one structural signal this
  data supports, does fire and does measurably help.
- **Checked whether the "IS" brand collision was a one-off oddity or a
  more widespread problem**: quantified directly against the full
  discovered-brand vocabulary (1,079 strings) — exactly one exact
  stopword collision, 40 short-but-legitimate brand acronyms. Not
  smoothed into "no problem" or exaggerated into "this vertical isn't
  safe."
- **Checked whether "zero commerce-core changes" could be an illusion
  from choosing an easy vertical**: electronics products carry genuinely
  different structure from WANDS's furniture/home taxonomy (no
  category hierarchy at all, heavy free-text technical specifications,
  a much larger and noisier brand vocabulary discovered per-catalog-size
  than WANDS's curated one) — this was not a vertical hand-picked to
  resemble WANDS; the keyword list was fixed before inspecting any
  match, and the resulting data shape (flat, category-less, marketplace
  brand strings) is precisely the kind of shape most likely to expose a
  hidden vertical-specific assumption, had one existed.

### Correction (2026-08-26) — the named brand-trust-filtering follow-up is already answered; withdrawn, not pursued

This document's own "What this does and does not establish" section
named a follow-up: "extending `min_enum_frequency`-style trust
filtering... to brand-name discovery specifically." That follow-up is
withdrawn, not pursued, after checking this project's own prior
evidence rather than treating the idea as novel.

Two corrections, found by reading `crates/commerce-core/src/cold_start/profile.rs`'s
own doc comments and `docs/experiments/PHASE2_LOG.md` before writing
any new code:

1. **Factual correction**: brand names do **not** categorically bypass
   `min_enum_frequency` filtering the way this document implied.
   `compile_lexicon` (`crates/commerce-core/src/cold_start/profile.rs`)
   already gates every brand through `brand_occurrence_count` at the
   same `min_enum_frequency` threshold as every other value (added by
   P2-E05, `docs/experiments/PHASE2_LOG.md`, after a real ESCI
   integration run found the *opposite* of this document's claim was
   the actual historical bug). This checkpoint's own binary used
   `MIN_ENUM_FREQUENCY = 1` -- the same value every dataset in this
   project uses by default -- at which that filter is a numerical
   no-op for any value that occurs at all, not evidence that brands are
   structurally exempt from frequency gating.
2. **Substantive correction, the more important one**: this project
   already ran an extensive, rigorous investigation into exactly this
   class of problem -- distinguishing genuine brand strings from noisy
   marketplace junk on the *real, full* ESCI catalog (206,227 distinct
   raw brand strings, 49.4% singleton) -- across four independent
   mechanisms (P2-E05's raw frequency threshold, P2-E08's
   `HeuristicCanonicalizer` deterministic heuristic, P2-E09/P2-E10's
   model-assisted classifier). **All three arrive at the same
   conclusion**: trusting *more* strings as hard structural filters,
   however intelligently chosen, costs more real Exact-relevance recall
   than it gains in false-inclusion-rate improvement. `PHASE2_LOG.md`'s
   own recorded verdict: **"CANONICALIZATION FRONTIER IS FUNDAMENTAL"**
   -- the tension is inherent to enforcing brand matches as a *hard*
   exact-match structural constraint at all, not to which classifier
   decides which strings to trust. A bespoke stopword/short-string
   blocklist targeting the specific `"IS"` collision this checkpoint
   found would be exactly the kind of "smarter classifier" approach
   this prior evidence already falsified as the wrong lever -- pursuing
   it here would silently re-litigate an already-closed question rather
   than build on it.

The evidence-backed direction this prior work actually points to (and
already partially shipped) is **softer enforcement, not smarter
classification**: `PHASE2_LOG.md`'s own "Next" step named testing
whether a *scored* or *alias-normalized* match beats an unconditional
hard filter -- which is exactly what `StructuralConstraint::BrandAny`
(Issue #6 P1-B, this session's own repeated precedent for `ProductTypeAny`)
already is: a deterministic alias-group mechanism, not a wider blocklist.
The `"IS"` collision this checkpoint found is a genuine, small, novel
*data point* in that same well-established tension (a hard exact-match
`Brand` constraint firing on a stopword collision is one more instance
of "hard exact-match structural filters are fragile to real-world
string noise"), but it does not call for new work -- it is additional
confirmation of a conclusion this project already reached and partly
acted on.

## Traceability

Source: `crates/issue35-eval/` (new crate: `src/lib.rs` ingestion,
`src/bin/esci_electronics_eval.rs` measurement). Dataset scripts:
`scripts/datasets/{fetch_esci_electronics.sh,filter_esci_electronics.py,solr_index_esci_electronics.py,esci_checksums.sha256}`.
Raw evidence: `docs/research/artifacts/i35_esci_electronics/run1.txt`.
