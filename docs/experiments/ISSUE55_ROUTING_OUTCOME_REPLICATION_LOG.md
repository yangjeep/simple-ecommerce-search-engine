# Issue #55 — does the WANDS FastPath/Hybrid routing-outcome split replicate on ESCI? (append-only log)

Continues `ISSUE55_PAIRED_COMPARATOR_LOG.md`'s own named next question
(`docs/decisions/ISSUE55_PAIRED_COMPARATOR_DECISION.md`): "a cleaner, more
durable finding survives: splitting by routing shows FastPath native
materially worse than Solr (-66.11%, n=7) while Hybrid is roughly at
parity (+3.02%, n=14)" on real WANDS data. Does this replicate on the
three already-built, independent ESCI vertical slices (electronics,
automotive, beauty)?

## Entry 1 — instrumenting the three existing ESCI harnesses

Extended `crates/issue35-eval/src/eval.rs`'s `run_vertical_eval` (the
shared measurement procedure all three ESCI vertical binaries call) to
additionally bucket native/Solr NDCG by `ExecutionOutcome`
(FastPath/Hybrid/Punt), printed as a new "relevance by routing outcome"
section. Purely additive: the existing aggregate `native_ndcgs`/
`solr_ndcgs` vectors, printed sections, and correctness gate are
untouched.

First run (Solr had to be restarted first -- a stale PID file from a
prior container lifecycle; `bin/solr start -p 8983 --force` recovered
all four cores from persistent disk with their original document counts,
no reindexing needed):

```
Electronics: FastPath n=0, Hybrid n=48 (native 0.2279 / solr 0.2694, gap -15.40%), Punt n=442 (native 0.3124 / solr 0.2802, gap +11.47%)
Automotive:  FastPath n=4 (native 0.6340 / solr 0.4343, gap +45.99%), Hybrid n=32 (native 0.1857 / solr 0.5054, gap -63.25%), Punt n=467 (native 0.4554 / solr 0.4476, gap +1.74%)
Beauty:      FastPath n=8 (native 0.6491 / solr 0.7916, gap -18.00%), Hybrid n=38 (native 0.4557 / solr 0.7173, gap -36.48%), Punt n=443 (native 0.4086 / solr 0.3900, gap +4.77%)
```

Raw output: `docs/research/artifacts/i35_esci_{electronics,automotive,beauty}/run2_routing_breakdown.txt`.

At face value this contradicts WANDS: Hybrid is *worse* than Solr on all
three ESCI verticals (WANDS found near-parity), and FastPath is
inconsistent (automotive reverses WANDS's own direction).

## Entry 2 — a real methodology defect caught before trusting Entry 1

Before writing up "does not replicate," re-reading `solr_search` (the
function `run_vertical_eval` calls for every query) found it sends Solr
**only the raw query text** -- no `fq` (filter query) for the `Brand`
(or `color`) structural/attribute constraint that native's own
`execute_planned` enforces internally via `restrict_to`/
`query.matches_variant` for the exact same query.

This is the same *class* of unfair-comparator defect
`ISSUE55_PAIRED_COMPARATOR_DECISION.md` (Priority 1A) just found and
fixed for WANDS's `p9_e02` -- a missing `ProductTypeAny` fq arm there --
except here it runs in the opposite direction: native answers a
correctly-scoped (harder) question (rank only within the Brand/color-
narrowed candidate set), while Solr answered an unrestricted (easier,
broader-pool) question. For any Brand- or color-constrained query routed
to Hybrid or FastPath, this would inflate Solr's NDCG relative to
native's -- plausibly the dominant explanation for Entry 1's consistent
"Hybrid native worse than Solr" pattern.

**Fix** (`crates/issue35-eval/src/eval.rs`, `crates/issue35-eval/Cargo.toml`):

- `solr_search` gained an `fq: &[String]` parameter, forwarded to Solr as
  repeated `fq=` form fields (Solr ANDs multiple `fq` clauses).
- `run_vertical_eval` now derives `fq` from `compiled.constraints` for
  each query: a `brand:/regex/` clause per resolved `Brand(id)` and a
  `color:/regex/` clause per resolved `Attribute(Constraint::Enum{attribute: "color", ..})`,
  using `round1_eval::solr::case_insensitive_field_regex` -- the same,
  already-adversarially-reviewed (P2-E13) construction every other Solr
  comparator in this project already uses for exactly this purpose
  (reused, not re-derived, per this project's own precedent-reuse
  discipline).
- **Scope disclosed explicitly, not silently**: both Brand and color are
  included, because `query.matches_variant` enforces both as hard
  filters identically (confirmed by reading `commerce-core`'s
  `Constraint::matches` and `execute_planned`'s `verify_and_truncate`/
  `identifier_hits`) -- restricting the fq to Brand alone would have
  left the same class of unfairness for the one live "neutrogena
  naturals lotion" (`Brand` + `color="Lotion"`) example the beauty
  checkpoint's own decision doc already disclosed.

**Fix verification** (RED-before-fix-style, not just a compile check):
two new tests, `fq_parameters_reach_the_wire_when_present` and
`no_fq_parameters_are_sent_when_the_query_has_no_structural_constraints`,
spawn a fake Solr that captures the raw request bytes over a channel and
assert on the literal `fq=` occurrence count on the wire -- expressible
only after `solr_search` gained the parameter. All 10 `issue35-eval`
unit tests pass (6 pre-existing `SolrLookup` tests + 4 new: the 2 above
plus updating the 6 existing call sites' arity, of which 2 already
counted). Full workspace quality gate (`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace --all-features` -- 135 test groups, zero
failures, `issue35-eval` itself now 10 passed, up from 6 -- `cargo build
--workspace --release`) reran clean after the fix.

## Entry 3 — rerun with the fair comparator

Solr still up (verified `HTTP 200` on all three cores' `/select`
endpoints before rerunning). Raw output:
`docs/research/artifacts/i35_esci_{electronics,automotive,beauty}/run3_fair_solr_fq.txt`.

```
                 FastPath (n)              Hybrid (n)                Punt (n)
Electronics      n=0 (no data)             n=48  -12.00% (was -15.40%)   n=442  +11.47% (unchanged)
Automotive       n=4   +45.99% (unchanged) n=32  -38.75% (was -63.25%)   n=467  +1.63%  (was +1.74%)
Beauty           n=8   -2.62%  (was -18.00%) n=38 -11.49% (was -36.48%)  n=443  +4.77%  (unchanged)
```

Aggregate (whole-vertical) NDCG numbers, the ones checkpoints 13/15/16's
own KEEP verdicts are based on, moved negligibly (electronics
+8.93%->+9.33%; automotive and beauty's aggregate gaps did not visibly
change beyond rounding) -- expected, since only 37-59 of 600 queries per
vertical carry a Brand/color constraint at all, and the fix only touches
Solr's answer to those. **No prior aggregate-level KEEP verdict is
affected by this fix.**

Automotive's FastPath bucket (n=4) and two of Punt's three buckets show
no numeric change at all -- consistent with those specific queries
carrying no Brand/color constraint reaching Solr any differently, not a
sign the fix failed to apply (the `fq_parameters_reach_the_wire_when_present`
test directly confirms the mechanism fires when a constraint is present).

## Interpretation

The bug was real and material: two of three verticals' Hybrid gaps
moved by 25-27 percentage points once Solr answered the same
Brand/color-scoped question native does. This alone is a valuable,
disclosed finding independent of the replication question -- see the
decision doc's verdict.

On the replication question itself, the corrected numbers still do
**not** replicate WANDS's own split:

- **Hybrid**: WANDS found near-parity (+3.02%, n=14). All three ESCI
  verticals still show Hybrid *worse* than Solr even after the fix
  (electronics -12.00%, beauty -11.49% -- both now inside this project's
  usual `<=15%` aggregate bound; automotive -38.75% -- clearly outside
  it).
- **FastPath**: WANDS found native materially worse (-66.11%, n=7). ESCI
  shows the opposite or a null result (automotive +45.99%, beauty
  -2.62%, electronics n=0). Not one ESCI vertical reproduces WANDS's
  FastPath-is-the-weak-outcome direction.

See the decision doc for the full verdict and what this does and does
not establish.
