# Issue #35 Experiment Log — first-pass unseen-vertical slice: real ESCI electronics data

Protocol: `docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`.

## I35-ESCI-E00 — H0 confirmed: the unmodified pipeline generalizes safely, with one disclosed, rare precision risk

**Dataset acquisition** (mechanical, disclosed before any metric was
inspected): `scripts/datasets/fetch_esci_electronics.sh` fetches one
real ESCI train parquet shard (`tasksource/esci` HF mirror, pinned
revision `45c948250c2116f1e535bac67b92501c695307a4`);
`scripts/datasets/filter_esci_electronics.py` filters to US-locale rows
matching a fixed 20-term electronics/components keyword list, capped at
4,000 products / 600 queries. Result: **2,075 real products, 600 real
queries**, judgment label distribution `{Irrelevant: 281, Substitute:
422, Exact: 1406, Complement: 33}` (counted per query-product pair, not
per query), **490/600 queries have >=1 non-Irrelevant judgment** (used
for NDCG; queries with zero positive-gain judgments contribute an
undefined IDCG and are excluded from the mean, following this
project's own established convention).

**Ingestion** (`crates/issue35-eval`, new crate, zero `commerce-core`
changes): every product gets `product_type`/`category` left as
unregistered sentinel ids (ESCI has no such field; fabricating one via
keyword classification would itself be exactly the "manually authored
vertical ontology" this checkpoint's own methodology prohibits
injecting). `Brand` (from real `product_brand`) and a generic `color`
Enum attribute (from real `product_color`) are populated —
pre-existing, vertical-agnostic concepts already used identically for
WANDS/Magento. Single-variant-per-product, placeholder price/inventory
(ESCI has neither), matching WANDS's own already-disclosed Product/
Variant limitation.

**Zero `commerce-core` changes required to run**: `CatalogProfile::build`,
`compile_lexicon`, `CatalogIndex::build`, `compile()`, and
`execute_planned` all ran unmodified against this new adapter's output.
No panic, no crash, no edit needed anywhere in `commerce-core` or
`phase9-eval`'s `BitmapTantivyDelegate`/`build_index` (reused verbatim).

**Discovery/routing** (descriptive, no pass/fail threshold per the
protocol):

```
catalog: 2075 products, 1079 distinct brands discovered
routing distribution: {"FastPath": 1, "Hybrid": 58, "Punt": 541}
queries with ambiguity: 6/600
queries with residual lexical text: 599/600
queries with a Brand structural constraint: 59/600
```

Routing is heavily `Punt`-dominated (541/600, 90.2%), as expected and
explicitly *not* treated as a failure: this vertical's real data offers
almost no structural signal beyond brand (no product-type/category
field exists at all), so there is little for `FastPath`/`Hybrid` to
specialize on. Per Issue #35's own text, "the methodology is explicitly
allowed to conclude a vertical or merchant is not worth specializing" —
this is exactly that finding, for the *product-type/category* axis
specifically (Brand-based structural narrowing, by contrast, does fire
and does help — see the relevance result below).

**Correctness (hard gate)**: `PASS` — zero wrong-family violations
across the 59 Brand-constrained queries (every hit under a `Brand`
structural constraint carried that exact brand, checked directly
against the built catalog, not assumed from `StructuralConstraint::Brand`'s
known-safe implementation).

**Relevance** (n=490 queries, real Solr 9.10.1 core `esci_electronics_bench`
indexed over the identical 2,075-document set, same
`text_general`/edismax discipline this project already uses for WANDS):

```
native NDCG@10=0.3041  solr NDCG@10=0.2792
relative gap (native vs solr): +8.93%
```

**H0 CONFIRMED** (native comfortably clears the preregistered <=15%
relative-gap bar — and is in fact *better* than Solr here, not merely
not-materially-worse). Spot-checked directly (not just accepted at face
value): Solr's own top-3 hits for "dell monitors" are an AC power cord
and a USB-C cable, not Dell-brand products at all — plain edismax BM25
has no brand-exactness bias, so native's structural `Brand` narrowing
(fired for exactly this kind of query) is carrying a real, measurable
advantage on the minority of queries where a structural signal exists,
not an artifact of a misconfigured or handicapped Solr comparison.

**A genuine, disclosed, rare precision risk found via direct
investigation** (not accepted as a black-box "H0 confirmed" without
checking the qualitative sample): the query `"a cord of three strands
is not easily broken wedding"` (an off-topic, non-electronics query
that entered this slice only because one of its originally-judged
products happens to also be in this electronics-filtered set) resolved
to a spurious `Brand` structural constraint (`BrandId(1023)`, zero
hits). Root cause, confirmed directly: one real product in this slice
has the literal raw `product_brand` value `"IS"` — a genuine short
brand acronym that happens to collide exactly with the common English
word "is" in the query text. Quantified: **1 of 1,079 discovered
brands (0.09%)** is an exact stopword collision (`{is, a, an, the, of,
to, in, on, at, for, and, or, not, no, it, be}` checked; only `"IS"`
matches); 40/1,079 (3.7%) are otherwise short (<=3 characters, e.g.
"GE", "LG", "HP" — legitimate real brands, not collisions). This does
**not** violate the correctness hard gate (it produces zero results,
not a wrong result), but it is real, disclosed evidence that this
project's existing brand-discovery mechanism (`CatalogProfile`'s brand
vocabulary, treated as always-trusted the same way `min_enum_frequency`
explicitly does *not* apply to brand names) is safe on WANDS's
single-curated-retailer data but carries a real, if rare, false-positive
risk on noisier, open multi-seller marketplace metadata where any
seller-entered string can become a `product_brand` value.

## Decision

See `docs/decisions/ISSUE35_ESCI_ELECTRONICS_DECISION.md`.
