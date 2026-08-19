# Issue #9: brand-vocabulary adjudication rubric

## Purpose

Ground truth for evaluating canonicalization strategies (frequency-only,
deterministic heuristic, model-assisted) against real long-tail brand
vocabulary excluded or near-frontier under P2-E05's `min_enum_frequency`
canonicalization (`docs/experiments/PHASE2_LOG.md`).

## Corpus

`scripts/phase2/build_brand_adjudication_corpus.py` (committed, deterministic,
seed=7) samples 209 real candidates from `dataset_cache/export/catalog.jsonl`
(gitignored, real 1,215,854-product ESCI catalog):

- 50 singleton (occurs on exactly 1 real product) — 101,786 eligible
- 50 low (2-5 occurrences) — 68,807 eligible
- 50 mid (6-25 occurrences) — 29,307 eligible
- 50 near_threshold (20-30 occurrences, straddling P2-E05's measured real
  recall-peak frontier at `min_enum_frequency=25`) — 3,780 eligible
- 9 calibration_high_frequency (fixed, well-known real brands already
  trusted at every threshold P2-E05 tested — sanity-check cases only, not
  part of the frontier under test)

Each candidate carries: the normalized brand string, its real occurrence
count, up to 5 representative real products (ASIN, title, bullets snippet,
color), and same-bucket brand strings sharing a token (a crude,
deterministic alias/near-duplicate signal, not a judgment).

**Known evidence gap, not papered over**: this dataset has no real
`product_type`/`category`/seller field (`round1_eval::catalog`'s own
documented ESCI export limitation — every real product gets sentinel
`ProductTypeId(0)`/`CategoryId(0)`). Issue #9 asked for those as
adjudication evidence where available; here they are not available.

## The five classes

Adjudicate each candidate into exactly one class, using **only** the
catalog evidence provided (representative products, occurrence count,
token-overlap candidates) — not general world knowledge about what brands
exist, since a real production canonicalizer would only have catalog
evidence available either.

1. **canonical known entity / alias** — the value clearly names a
   real-world brand/manufacturer entity, or is an unambiguous spelling/
   formatting/casing variant of one already identifiable from the evidence
   (e.g. a possessive/plural variant, an abbreviation of a name that
   appears in full elsewhere in the token-overlap candidates).
2. **legitimate new entity deserving a canonical ID** — the value reads as
   a plausible, distinct brand/manufacturer name (proper-noun-shaped, used
   consistently as a brand label across its representative products) even
   if small, regional, or unfamiliar — not a slogan, not a descriptive
   phrase, not obviously a mis-filed value.
3. **lexical-only value that should not become a structural primitive** —
   the value is real text (often a tagline, a product-title fragment, a
   generic descriptive phrase, or a seller/storefront name used
   title-fragment-style) that is not nonsensical but is not a brand
   *entity* — it should remain free text, not become a trusted hard-filter
   lexicon entry.
4. **ambiguous / insufficient evidence** — the provided catalog evidence
   genuinely does not support a confident decision between two or more of
   the above (state which ones, and what additional evidence — e.g. a real
   product_type field — would resolve it).
5. **junk / malformed / wrong-field value** — the value carries no
   brand-identifying signal at all: gibberish, clearly the wrong field's
   content, near-empty, or purely numeric/coded with no semantic content.

## Labeling protocol

Three independent adjudication passes (separate agent runs, no shared
context between them) label the full 209-candidate corpus against this
rubric. Per-candidate agreement across all three passes is the confidence
signal:

- **3/3 agree**: high-confidence ground truth (that label).
- **2/3 agree**: majority ground truth, flagged as lower-confidence.
- **no majority (3 different labels, or 2-way split with real ambiguity
  language in the rationale)**: ground truth label is itself
  `ambiguous / insufficient evidence` — genuine adjudication difficulty is
  evidence, not noise to be resolved by fiat.

**Explicit methodological limitation, stated rather than hidden**: all
three labeling passes and the "model-assisted" canonicalizer arm being
evaluated against this ground truth are, in this environment, produced by
the same underlying model family (no independent human annotator panel or
distinct-vendor model was available here). This is a real threat to
validity — correlated errors between "ground truth" and "system under
test" could inflate the model-assisted arm's apparent precision/recall
relative to what independent human adjudication would show. This
limitation is carried into every result derived from this corpus and must
not be smoothed over when interpreting them. A production deployment
decision should not rest on this evidence alone without independent
(human, or different-vendor-model) validation.
