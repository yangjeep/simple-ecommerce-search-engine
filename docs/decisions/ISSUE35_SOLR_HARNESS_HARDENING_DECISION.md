# Issue #35 — Solr benchmark harness hardening: transport/parse failure must never score as relevance loss

Log: `docs/experiments/ISSUE35_SOLR_HARNESS_HARDENING_LOG.md`.

## Verdict: KEEP the fix. Confirmed CLEAN on all three existing unseen-vertical checkpoints — no historical conclusion changes, but the class of bug was real and would have silently fabricated a Solr loss had it fired

## The defect

`issue35-eval::eval::solr_search` (the function every unseen-vertical
checkpoint's native-vs-Solr NDCG comparison is built on) converted two
categorically different outcomes into the same `Vec::new()`:

1. a genuine transport failure (`ureq`'s HTTP call returning `Err` --
   connection refused, timeout, a non-2xx/3xx status);
2. a response body that failed to parse as JSON.

Both were indistinguishable from "Solr answered and legitimately found
zero matching documents," because the caller (`run_vertical_eval`)
always scored an empty ranked list as NDCG=0.0 via
`ndcg_at_k_graded(...).unwrap_or(0.0)`. A Solr outage, a firewall rule,
a stopped process, or a malformed edismax query from an unescaped
special character in real ESCI query text would all have silently
depressed `solr_mean`, inflating `native`'s *relative* advantage over
Solr -- i.e. an infrastructure failure would have read as a native
relevance *win*, in exactly the direction that makes a favorable
architecture claim easiest to over-trust. This is precisely the
research-hygiene failure mode Issue #55's adversarial-review discipline
exists to catch, found here by direct inspection before it produced a
wrong number, not after.

`responseHeader.status != 0` (a Solr-side query error returned with
HTTP 200) had the identical problem: the old code never inspected it,
so a malformed query silently became a zero-result query too.

## The fix

`crates/issue35-eval/src/eval.rs`:

- `solr_search` now returns a `SolrLookup` enum --
  `Success(Vec<String>)` / `TransportError(String)` / `ParseError(String)`
  -- instead of collapsing all three into `Vec<String>`.
  `responseHeader.status != 0` is now checked explicitly and treated as
  `TransportError` (Solr answered, but with a query-side error, not a
  relevance verdict). A response that parses as JSON but is missing the
  expected `response.docs` array shape is `ParseError`, not `Success(vec![])`.
- `run_vertical_eval` now builds `native_ndcgs`/`solr_ndcgs` as a
  **paired** set: a query enters the native-vs-Solr comparison only when
  Solr actually answered it (`Success`, including a legitimate
  `Success(vec![])`). A `TransportError`/`ParseError` query is excluded
  from the comparison and counted separately -- never scored as Solr
  NDCG=0.0.
- **Preregistered rule**: this Solr core is same-host and locally
  controlled (`/home/user/solr_setup/solr-9.10.1`, started via
  `bin/solr start`), not a flaky remote dependency, so any
  transport/parse failure during a run is treated as a real
  infrastructure defect, not expected variance. Any such failure makes
  the harness print every failing query plus the underlying error, then
  `std::process::exit(1)` -- the run does not print a relevance section
  at all rather than print a partial, uncertified comparison. This
  satisfies Issue #55's own instruction ("failed Solr requests must
  fail the experiment or be explicitly recorded/excluded under a
  preregistered rule") by doing both: excluded from the numeric
  comparison, and the run fails loudly.
- Six new unit tests in `crates/issue35-eval/src/eval.rs` (`mod tests`)
  spin up one-shot fake HTTP servers (`std::net::TcpListener`) and a
  closed port to directly exercise all four `SolrLookup` outcomes,
  including the two the bug conflated (`TransportError` on connection
  refusal and a Solr-side error status; `ParseError` on invalid JSON and
  a missing `response.docs` shape) and confirming a real empty-docs
  response is still scored `Success(vec![])`, not an error.

## Rerun of the three existing checkpoints

Ran all three existing unseen-vertical binaries
(`esci_electronics_eval`, `esci_automotive_eval`, `esci_beauty_eval`)
against the project's existing local Solr 9.10.1 install
(`/home/user/solr_setup/solr-9.10.1`, cores `esci_electronics_bench` /
`esci_automotive_bench` / `esci_beauty_bench` -- already indexed from a
prior session, doc counts verified against each checkpoint's own
recorded catalog size before rerunning: 2,075 / 1,056 / 2,093).

All three exited 0 (zero transport/parse errors) and reproduced their
originally recorded numbers exactly:

| Vertical | Native NDCG@10 | Solr NDCG@10 | Relative gap | Matches original decision doc |
|---|---|---|---|---|
| Electronics | 0.3041 | 0.2792 | +8.93% | `ISSUE35_ESCI_ELECTRONICS_DECISION.md` -- yes, byte-identical |
| Automotive | 0.4396 | 0.4511 | -2.55% | `ISSUE35_ESCI_AUTOMOTIVE_DECISION.md` -- yes, byte-identical |
| Beauty | 0.4162 | 0.4220 | -1.38% | `ISSUE35_ESCI_BEAUTY_DECISION.md` -- yes, byte-identical |

**No historical conclusion changes.** The bug existed in the code the
entire time these three checkpoints ran, but Solr was reachable and
healthy during all three original runs, so it never actually fired --
confirmed directly, not inferred, by rerunning against the same live
cores under the hardened harness and getting the same numbers. The
three `H0 CONFIRMED` verdicts stand.

## Adversarial check that the fix actually does something

Ran `esci_electronics_eval` a fourth time against a deliberately
unreachable URL (`http://localhost:9999/solr/nonexistent_core`). Before
this fix, this would have printed `native NDCG@10=0.3041  solr
NDCG@10=0.0000`, a fabricated ~+100000% relative gap, and an incorrect
`H0` verdict claiming native "carries real ranking quality" against a
Solr instance that never actually ran. After the fix: `exit code 1`, a
`SOLR HARNESS FAILURE` banner naming all 490 affected queries and their
connection-refused errors, and **no relevance numbers printed at all**.

## Scope note

`crates/phase5-eval/src/bin/p5e00_solr_vs_native_eval.rs` and
`crates/phase6a-eval/src/bin/p6a_e00_wands_vs_native_eval.rs` were
checked for the same pattern: their `solr_get` helper already calls
`.unwrap_or_else(|e| panic!(...))` on both the HTTP call and the JSON
parse, i.e. they already fail loudly rather than silently defaulting.
Only `issue35-eval`'s `solr_search` had the silent-empty-list defect;
this fix does not touch those other binaries.

## Decision

**KEEP.** Correctness/research-hygiene fix, zero experiment-semantics
change, all three affected historical checkpoints reconfirmed
unchanged, and the failure path is now directly demonstrated (not just
argued) to fail loudly instead of fabricating a relevance number.
