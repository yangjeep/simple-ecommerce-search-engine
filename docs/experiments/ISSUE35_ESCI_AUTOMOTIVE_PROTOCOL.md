# Issue #35 Preregistered Protocol — second unseen-vertical slice: real ESCI automotive-parts data

## 0. What this is testing

`docs/decisions/ISSUE35_ESCI_ELECTRONICS_DECISION.md` (checkpoint 13)
found this project's existing discovery/serving pipeline generalizes
safely to a real electronics-vertical slice with zero `commerce-core`
changes. Issue #35's own Workstream D asks for "at least three
materially different verticals" before any generalization claim beyond
a single slice is warranted. This checkpoint tests a second: real
Amazon automotive parts/accessories listings, structurally and
semantically distant from both WANDS (home/furniture), the Magento
fixture (apparel), and the electronics slice (consumer
electronics/components) -- automotive parts carry fitment
(vehicle-make/model/year) semantics none of the prior three verticals
have at all.

Same dataset source and construction discipline as the electronics
slice: `tasksource/esci` (Apache-2.0), same pinned HF revision, a fixed
keyword list chosen before inspecting any downstream metric, same
2,075-scale/600-query target caps, same ingestion rules (no
product_type/category fabrication -- ESCI has none; `Brand`/`color`
populated from real data).

Keyword list (fixed before running): `brake pad`, `brake rotor`,
`spark plug`, `oil filter`, `air filter`, `wiper blade`, `car battery`,
`floor mat`, `seat cover`, `windshield wiper`, `motor oil`, `timing
belt`, `alternator`, `radiator hose`, `muffler`, `exhaust pipe`, `tire
pressure gauge`, `jumper cables`, `car wax`, `steering wheel cover`.

## 1. Hypothesis

**H0**: the same three findings checkpoint 13 established replicate on
this second, independent vertical: (a) the unmodified pipeline runs
with zero `commerce-core` changes/crashes; (b) zero wrong-family
`Brand` violations; (c) native NDCG@10 is within the same preregistered
<=15% relative gap vs. a real Solr baseline. Routing distribution is
reported descriptively, not gated (per Issue #35's own allowance that a
vertical may have little to specialize on).

**H1 (falsification)**: any of (a)/(b)/(c) fails on this second
vertical, meaning checkpoint 13's finding does not generalize even to
a second real vertical and the "zero vertical-specific code" claim was
narrower than it appeared.

## 2. Baseline / dataset / treatment

Identical methodology to `docs/experiments/ISSUE35_ESCI_ELECTRONICS_PROTOCOL.md`,
reusing the shared measurement procedure
(`issue35_eval::eval::run_vertical_eval`, extracted from the
electronics checkpoint's own binary with no logic change -- confirmed
byte-identical reproduction of checkpoint 13's own numbers before this
extraction was trusted) against a new, independently-fetched dataset
slice and a new Solr core (`esci_automotive_bench`).

## 3. Metrics / gates

Identical to the electronics protocol: correctness hard gate (zero
wrong-family `Brand` violations, checked first), <=15% relative NDCG@10
gap vs. Solr, routing distribution reported descriptively.
