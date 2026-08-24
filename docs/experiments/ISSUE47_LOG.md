# Issue #47 Experiment Log — E2d: adaptive semantic consensus and
# proposal-model capability/cost frontier

## I47-E2d Phase A: adaptive-consensus controller

**Hypothesis** (`docs/experiments/ISSUE47_PROTOCOL.md`): a deterministic
adaptive controller wrapped around the already-frozen E2c canonicalizer
(R1-R11, unchanged) can match fixed-5-ensemble (A1) quality/safety while
requesting materially fewer independent proposals per semantic problem,
by certifying a result robust once no possible composition of the
remaining undrawn proposals (up to the pool's own size) could change the
promoted `(role, primitive, scope)` triple.

**Method**: reused `e2c_canonicalizer::canonicalize` (Treatments C and D)
completely unmodified. Implemented a pure deterministic controller
(`crates/issue42-eval/src/e2d_controller.rs`) that, at each depth `n`,
checks whether a hypothetical unanimous block of the remaining
`K_MAX - n` proposals (for any of the 7 possible alternate roles — the
maximum-leverage adversarial perturbation for any vote mix at that
budget) could ever change the current outcome; if not, it stops early
and delivers the current result, never reading any proposal's own
self-reported `confidence` (grep-verified by an automated test). A0
(single proposal), A1 (fixed-5, reusing E2c's own leave-one-out design
unchanged), A2 (adaptive/Treatment C), and A3 (conservative
adaptive/Treatment D) are all computed from the same 5-draw pool per
real key; A2/A3's own repeated-run stability is measured via 5 cyclic
rotations of that pool (`ISSUE47_PROTOCOL.md` §8), matching A1's own
`C(5,2)=10` pairwise-comparison sample size.

Reproduction:
`cargo build --release -p issue42-eval && ./target/release/e2d_adaptive_consensus_eval [calibration|heldout] [out.json]`

### Calibration-lane sanity check (before any held-out draw exists)

Recorded in full in `ISSUE47_PROTOCOL.md`'s own Addendum 1 (not
duplicated here) — run against the already-frozen, already-analyzed
`automotive` E2b artifacts (zero new live calls), confirming the
controller behaves correctly end to end: 100% match with A1 on
oracle-disagreement count (1 of 17), 100% full-descriptor/primitive
stability for both A2 and A3, zero abstention, mean depth 3.18/5
(36.47% reduction vs fixed-5). Named explicitly: the controller's own
"majority lock" mathematics make `n=3` the earliest possible certified
depth for `K_MAX=5` — no key can ever certify at n=1 or n=2 regardless
of real-vote unanimity, since a hypothetical unanimous 3- or 4-vote
remaining block always outvotes 1 or 2 real votes under plain plurality.
No controller code or threshold was changed after this run.

## Real Product/Variant/relationship dataset attempt (external validity)

Per Issue #47's "Datasets / external validity" section: attempted to
acquire at least one license-compatible real structured feed/catalog
with genuine Product/Variant identity or relationship complexity (beyond
WANDS, which R6's own audit already established has **no** real per-row
Variant grouping — every WANDS record is exactly one Product with
exactly one Variant, per `e2b_ingest::build_catalog`'s own doc comment,
carried forward unchanged from E2b/E2c).

This session's network access is restricted to a pre-configured allowlist
(`curl -sS "$HTTPS_PROXY/__agentproxy/status"` shows the policy). A
general web request to `kaggle.com` — host of several well-known real
Product/Variant catalogs (e.g. Home Depot search relevance, Olist
Brazilian e-commerce) — is denied by gateway policy (403,
`connect_rejected`). `github.com` (both HTTPS and `git` protocol) **is**
reachable, confirmed directly (`git ls-remote
https://github.com/magento/magento2-sample-data.git HEAD` succeeded).

A search identified `magento/magento2-sample-data` (official Magento 2
sample catalog, OSL-3.0/AFL-3.0 licensed, GitHub-hosted) as a reachable
candidate with genuine configurable-product (parent) / simple-product
(child SKU, e.g. size/color combinations) structure — real in the sense
of reflecting an actual commercial platform's production configurable-
product schema (used by real Magento merchants), though itself a
vendor-authored demo catalog rather than data scraped from a live store,
a materially weaker "real" claim than WANDS's own genuine Wayfair scrape.

**Decision**: full integration (clone, parse Magento's CSV/product-
attribute-set format, build a new ingestion module analogous to
`e2b_ingest`, author an independent oracle for its configurable/simple
product split, wire it through the E2d pipeline) is a materially sized
new-dataset-ingestion engineering task in its own right, not a short
extension, and Issue #47's own priority ordering places Phase A and
Phase B — not dataset acquisition — as this checkpoint's primary
deliverables. Issue #47 explicitly authorizes this exact fallback: "If
no qualifying dataset is acquired, Phase A/B may still produce useful
results but external validity remains NOT ESTABLISHED." Per that
clause: **external validity beyond WANDS/automotive is NOT ESTABLISHED**
for this checkpoint. `magento/magento2-sample-data` is recorded here as
a confirmed-reachable, license-compatible candidate for a future
checkpoint, not silently dropped.

### Held-out measurement

*Pending — 10 fresh, independent `claude-sonnet-5` proposal draws (5x
`wands_baseline`, 5x `automotive`, per `ISSUE47_PROTOCOL.md` §7) are
being generated via a Workflow run at the time this entry was started.
Results appended below once the draws are frozen and
`e2d_adaptive_consensus_eval heldout` is run — never before.*
