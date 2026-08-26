# Issue #35 Preregistered Protocol — third unseen-vertical slice: real ESCI beauty/personal-care data

## 0. What this is testing

Completes Issue #35's own Workstream D requirement of "at least three
materially different verticals" (`docs/decisions/ISSUE35_ESCI_ELECTRONICS_DECISION.md`,
`docs/decisions/ISSUE35_ESCI_AUTOMOTIVE_DECISION.md` cover the first
two). This third slice is beauty/personal-care products -- real Amazon
listings for skincare, haircare, and cosmetics -- structurally distinct
from furniture (WANDS), apparel (Magento), electronics, and automotive:
this vertical's real distinguishing attributes (skin type, ingredient
lists, shade/scent) are almost entirely free-text, with even less
structured signal available than the automotive slice's fitment data.

Same dataset source, pinned HF revision, and construction discipline as
the prior two slices. Keyword list (fixed before running): `shampoo`,
`conditioner`, `face moisturizer`, `lip balm`, `mascara`, `eyeliner`,
`foundation makeup`, `nail polish`, `hair dryer`, `flat iron`, `body
lotion`, `sunscreen`, `deodorant`, `perfume`, `essential oil`, `makeup
brush`, `hair straightener`, `facial cleanser`, `eye cream`, `beard
oil`.

## 1. Hypothesis

**H0**: the same three findings replicate on this third vertical: (a)
zero `commerce-core` changes/crashes; (b) zero wrong-family `Brand`
violations; (c) native NDCG@10 within the same preregistered <=15%
relative gap vs. a real Solr baseline.

**H1 (falsification)**: any of (a)/(b)/(c) fails.

## 2. Baseline / dataset / treatment

Identical methodology and shared measurement code
(`issue35_eval::eval::run_vertical_eval`) as the two prior slices,
against a new, independently-fetched dataset slice and a new Solr core
(`esci_beauty_bench`).

## 3. Metrics / gates

Identical to the two prior protocols: correctness hard gate (checked
first), <=15% relative NDCG@10 gap vs. Solr, routing distribution and
brand-collision check reported descriptively.
