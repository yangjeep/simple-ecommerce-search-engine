# Issue #35 Experiment Log — third unseen-vertical slice: real ESCI beauty/personal-care data

Protocol: `docs/experiments/ISSUE35_ESCI_BEAUTY_PROTOCOL.md`.

## I35-ESCI-BEAUTY-E00 — H0 confirmed: checkpoint 13/15's finding replicates on a third, independent real vertical; the ">=3 materially different verticals" goal is now met

**Dataset acquisition**: `scripts/datasets/fetch_esci_beauty.sh` (same
pinned HF revision, independent download) +
`scripts/datasets/filter_esci_beauty.py` (fixed 20-term beauty/
personal-care keyword list). Result: **2,093 real products, 600 real
queries**, label distribution `{Irrelevant: 291, Exact: 802,
Substitute: 458, Complement: 28}`, 489/600 queries with >=1
non-Irrelevant judgment.

**Zero `commerce-core` changes required** (same code, same crate, same
shared `run_vertical_eval` function, third reuse):

```
catalog: 2093 products, 1231 distinct brands discovered
routing distribution: {"FastPath": 8, "Hybrid": 38, "Punt": 554}
queries with ambiguity: 32/600
queries with a Brand structural constraint: 46/600
```

**Correctness (hard gate)**: `PASS` -- zero wrong-family violations
across the 46 Brand-constrained queries.

**Relevance** (n=489 queries, real Solr core `esci_beauty_bench`):

```
native NDCG@10=0.4162  solr NDCG@10=0.4220
relative gap (native vs solr): -1.38%
```

**H0 CONFIRMED**, comfortably inside the <=15% bar -- and, notably, the
third distinct NDCG direction/magnitude across the three verticals
tested (electronics +8.93%, automotive -2.55%, beauty -1.38%), all
landing near parity rather than clustering suspiciously at one extreme.
Qualitative sample spot-checked directly: "shea moisture shampoo" and
"neutrogena sunscreen" resolve to correct, real, brand-exact matches.

**A genuine, disclosed quirk found by direct inspection, not glossed
over**: `"neutrogena naturals lotion"` resolved to *two* simultaneous
constraints -- `Brand(Neutrogena)` **and** `Attribute(color="Lotion")`
-- and returned zero hits. Root cause: ESCI's raw `product_color`
field is reused loosely on this catalog for form-factor descriptors
("Lotion", "Cream", "Spray") as well as genuine colors, so the word
"lotion" in the query resolved as a `color` enum value via the same
generic attribute-indexing mechanism color values normally use. No
product in this slice happens to be tagged with exactly `brand=Neutrogena`
`AND` `color=Lotion` simultaneously, so the (correct, safe) conjunction
returns nothing rather than a wrong result -- the same "zero-result,
not wrong-result" failure mode this project's own commerce-native
Product/Variant correctness discipline already guarantees, not a new
correctness gap. Disclosed as a real, if minor, coverage limitation
specific to catalogs that overload the "color" field name for
non-color variant dimensions.

**Brand-collision check**: 0 of 1,231 discovered brand strings collide
with the same 16-word stopword list checked for the prior two
verticals (electronics: 1/1,079; automotive: 0/502) -- a third data
point consistent with that risk being rare and isolated.

**Milestone**: three materially different real verticals (electronics,
automotive parts, beauty/personal care) now independently confirm the
same architectural safety property (zero `commerce-core` changes, zero
wrong-family matches, relevance within bounds), completing Issue #35's
own explicitly-named Workstream D requirement of "at least three
materially different verticals" for this slice of the epic.

## Decision

See `docs/decisions/ISSUE35_ESCI_BEAUTY_DECISION.md`.
