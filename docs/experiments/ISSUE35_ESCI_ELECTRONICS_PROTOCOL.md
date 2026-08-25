# Issue #35 Preregistered Protocol — first-pass unseen-vertical slice: real ESCI electronics data

**Scope, disclosed up front**: Issue #35 is a large, multi-workstream epic
(frozen domain-neutral representation, methodology freeze before scoring,
blind retrospective replay, >=3 unseen verticals, merchant heterogeneity,
a cold-start artifact). This checkpoint is a deliberately small, single
falsifiable slice of Workstream B/D — **not** a claim of satisfying the
epic's own "Definition of done" checklist. It answers one question:
*does this project's existing, unmodified discovery/serving pipeline
(`CatalogProfile`, `compile_lexicon`, `CatalogIndex`, `execute_planned`)
behave safely and sanely on a real, genuinely different commerce
vertical, with zero `commerce-core` changes and zero hand-authored
vertical ontology?* Every dataset this project has used to date is
either home/furniture-heavy (WANDS) or a tiny apparel fixture (the
Magento configurable-product data) — neither tests a vertical where the
project's own code has never been exercised.

## 0. What this is testing

Dataset: a real, publicly available slice of the Amazon Shopping
Queries Dataset ("ESCI"), fetched from the `tasksource/esci` mirror on
Hugging Face (Apache-2.0-licensed original dataset; this project has
precedent using ESCI, per `docs/decisions/ROUND1_DECISION_TREE.md` /
`PHASE2_DECISION.md`). Filtered to real US-locale rows whose product
title/description/bullet points match a curated electronics/components
keyword list (e.g. "resistor", "capacitor", "HDMI", "ethernet cable",
"soldering iron", "multimeter", "oscilloscope", "circuit board",
"bluetooth speaker", "USB-C hub" — chosen to select a vertical that is
structurally and semantically distant from WANDS's furniture/home-goods
catalog, not to hand-pick a favorable sample).

**Critical honesty constraint**: ESCI carries no `product_type` or
`category` field at all (unlike WANDS's `product_class`/`category_leaf`).
Rather than hand-author a keyword-based category classifier — which
would itself be exactly the "manually authored vertical ontology"
Issue #35 prohibits injecting into a first pass — every ingested
product's `product_type`/`category` are left as **unregistered
sentinel ids**, invisible to `CatalogProfile`'s lexicon (the same
`UNKNOWN_PRODUCT_TYPE`-style pattern `phase6a-eval`'s WANDS adapter
already uses for genuinely absent data, not a new mechanism). `Brand`
(from ESCI's real `product_brand` field) and a generic `color`
attribute (from `product_color`) are the only structural/attribute
signals populated, since both are pre-existing, vertical-agnostic
concepts already used identically for WANDS and the Magento fixture —
not new, vertical-specific code.

**Also disclosed up front**: ESCI has no real Product/Variant grouping
(flat products, one row per product) and no price data. Single-variant
ingestion (already an established, disclosed pattern for WANDS) is used
again here; price/inventory are placeholder constants, and no
price-range query is tested against this slice.

## 1. Hypothesis

**H0 (architecture generalizes)**: the unmodified pipeline handles this
vertical safely — zero wrong-family/unsafe structural promotions (any
`Brand`-constrained query's candidate set contains only that exact
brand's products), no panics/crashes, and native's NDCG@10 is not
materially worse than a real Solr baseline indexed over the identical
document set (threshold: **<=15% relative NDCG@10 gap**, i.e. native
NDCG@10 >= 0.85 x Solr NDCG@10 — a looser bar than this session's
usual >=10%/2x thresholds, disclosed as appropriate for a
smaller-scale, single-slice first pass rather than the 480-query WANDS
benchmark's own statistical weight).

**H1 (falsification)**: either (a) a correctness violation (a
wrong-family match) or crash occurs, meaning the architecture requires
vertical-specific code to behave safely on unseen data — serious
negative evidence per Issue #35's own falsification criteria; or (b)
native NDCG@10 is materially worse than Solr's (>15% relative gap),
meaning the delegate-fallback path alone (with almost no structural
specialization available, since this vertical offers little beyond
brand/color) is not carrying real ranking quality.

**Explicitly not treated as failure**: a routing distribution
dominated by `Punt` (little to no `FastPath`/`Hybrid` traffic). Per
Issue #35's own text, "the methodology is explicitly allowed to
conclude a vertical or merchant is not worth specializing" — if this
vertical's real data offers too little structural signal (no
product-type/category field at all) for `FastPath`/`Hybrid` to ever
fire, that is a legitimate, informative, disclosed finding about this
vertical's own specialization potential, not evidence against the
architecture's safety or the H0/H1 gates above.

## 2. Dataset construction (mechanical, disclosed, no peeking at outcomes before recording it)

1. Download one ESCI train parquet shard (`tasksource/esci`,
   `refs/convert/parquet/default/train/0000.parquet`, ~115MB) via the
   Hugging Face resolve URL.
2. Filter to `product_locale == "us"` and a case-insensitive keyword
   match against `product_title`/`product_description`/
   `product_bullet_point` using a fixed, disclosed keyword list (below),
   chosen *before* inspecting match counts or any downstream metric.
3. Deduplicate by `product_id`, keeping the first-seen row's product
   fields; collect every `(query, esci_label)` pair naming a kept
   `product_id` as that query's judgment set.
4. Drop queries with fewer than 1 judged product, and cap the final
   slice at a disclosed target scale (a few thousand products, a few
   hundred queries) — smaller than WANDS by design (first-pass slice,
   not a full replacement benchmark).

Keyword list (fixed before running): `resistor`, `capacitor`,
`transistor`, `circuit board`, `breadboard`, `soldering iron`,
`multimeter`, `oscilloscope`, `hdmi`, `ethernet cable`, `usb-c hub`,
`bluetooth speaker`, `power supply`, `arduino`, `raspberry pi`, `led
strip`, `wire stripper`, `heat shrink tubing`, `alligator clip`,
`voltage regulator`.

## 3. Metrics / gates

- **Correctness (hard gate, checked first)**: for every query whose
  compiled constraints include a `Brand` structural constraint, every
  hit returned must have that exact brand — checked directly against
  the built index/catalog, not assumed from `StructuralConstraint::Brand`'s
  known-safe `matches` implementation. Any violation is an immediate
  H1/STOP-level finding requiring investigation before anything else is
  trusted.
- **No production code changes**: `commerce-core` is used exactly as
  built for WANDS/Magento — if it panics, mishandles, or requires any
  edit to run against this new adapter's output, that is itself
  reported as a falsification signal per Issue #35's own criteria, not
  silently patched into an invisible non-event.
- **Routing distribution and lexicon discovery stats**: reported
  descriptively (FastPath/Hybrid/Punt counts, distinct brands
  discovered, ambiguity/residual rates from `compile()`) — no pass/fail
  threshold, since this is a first-pass exploratory slice, not a
  benchmark this project will track over time the way WANDS's 480
  queries are.
- **Relevance**: native NDCG@10 vs. a real Solr core indexed over the
  identical document set (same pattern as WANDS's `p9_e02`/`p9_e04`:
  Solr as an independent, mature baseline, not `BitmapTantivyDelegate`
  standing in for it). Graded relevance from `esci_label`: Exact=1.0,
  Substitute=0.1, Complement=0.01, Irrelevant=0.0 (the standard mapping
  used in ESCI-derived ranking literature, not invented for this
  checkpoint). Gate: **<=15% relative gap** (native vs. Solr) per H0/H1
  above.

Repetitions: NDCG is deterministic given fixed judgments and a fixed
lexicon; latency is not this checkpoint's subject (no latency claim is
made), so no repeated-trial discipline is required for the primary
gates.
