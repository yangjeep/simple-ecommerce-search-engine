# Issue #35 Experiment Log — Solr benchmark harness hardening

Decision: `docs/decisions/ISSUE35_SOLR_HARNESS_HARDENING_DECISION.md`.

## I35-HARNESS-E00 — transport/parse failure was scorable as Solr relevance loss; fixed, reran, confirmed clean

**Trigger**: Issue #55's P0.2 instruction -- audit whether the Issue #35
evaluator can convert Solr transport/JSON failures into an empty result
list that silently scores as `Solr NDCG=0`.

**Found**: yes. `crates/issue35-eval/src/eval.rs`'s `solr_search`
returned `Vec::new()` on both a `ureq` transport error (`Err(...)`) and
a JSON-parse failure, indistinguishable from a real "Solr found zero
matching documents" response. `run_vertical_eval` then scored an empty
list as NDCG=0.0 unconditionally
(`ndcg_at_k_graded(...).unwrap_or(0.0)`). A `responseHeader.status != 0`
Solr-side query error (HTTP 200, but Solr itself reporting failure) had
the same problem -- never inspected at all.

**Fix**: `solr_search` returns a `SolrLookup { Success(Vec<String>),
TransportError(String), ParseError(String) }` enum.
`responseHeader.status != 0` is now checked and treated as
`TransportError`. A response missing the expected `response.docs` array
shape is `ParseError`, not a silent empty `Success`. `run_vertical_eval`
builds the native-vs-Solr comparison as a paired set: a query enters
`native_ndcgs`/`solr_ndcgs` only when Solr returned `Success`
(including a legitimate `Success(vec![])`, still correctly scored
0.0). A `TransportError`/`ParseError` query is excluded from the
comparison, counted, and printed with its underlying error. Per the
preregistered rule (this Solr core is same-host/locally-controlled, not
a flaky remote dependency), **any** transport/parse failure makes the
run print the failure banner and `std::process::exit(1)` before
printing any relevance numbers -- no partial/uncertified comparison is
ever reported as a result.

**Unit tests** (`crates/issue35-eval/src/eval.rs::tests`, 6 new,
`cargo test -p issue35-eval`): connection-refused ->
`TransportError`; invalid-JSON body -> `ParseError`; JSON missing the
`response.docs` shape -> `ParseError`; `responseHeader.status=400` ->
`TransportError`; valid empty `docs: []` -> `Success(vec![])` (not an
error); valid non-empty `docs` -> ids round-trip correctly. All pass.

**Rerun of the three existing checkpoints**, against the project's
existing local Solr 9.10.1 install (`/home/user/solr_setup/solr-9.10.1`,
started via `bin/solr start -p 8983 --force`; cores
`esci_electronics_bench`/`esci_automotive_bench`/`esci_beauty_bench`
already indexed from a prior session -- doc counts verified via
`admin/cores?action=STATUS` before rerunning: 2,075 / 1,056 / 2,093,
matching each checkpoint's own recorded catalog size):

```
electronics: native NDCG@10=0.3041  solr NDCG@10=0.2792  relative gap: +8.93%  (exit 0, 0 solr errors)
automotive:  native NDCG@10=0.4396  solr NDCG@10=0.4511  relative gap: -2.55%  (exit 0, 0 solr errors)
beauty:      native NDCG@10=0.4162  solr NDCG@10=0.4220  relative gap: -1.38%  (exit 0, 0 solr errors)
```

Raw console output preserved at
`docs/research/artifacts/i35_solr_harness_hardening/{electronics,automotive,beauty}_rerun_hardened.log`.
Every number is byte-identical to the originally recorded checkpoint 13
(`ISSUE35_ESCI_ELECTRONICS_DECISION.md`), checkpoint 15
(`ISSUE35_ESCI_AUTOMOTIVE_DECISION.md`), and checkpoint 16
(`ISSUE35_ESCI_BEAUTY_DECISION.md`) numbers. **No conclusion changes.**
The defect existed in the code throughout all three original runs but
never fired, because Solr was reachable and healthy each time --
confirmed directly by reproducing the same numbers against the same
live cores under the hardened harness, not merely inferred from the old
runs' own exit codes (the old harness had no way to report a failure at
all, which is exactly the point being fixed).

**Adversarial check that the fix is not a no-op**: reran
`esci_electronics_eval` a fourth time against
`http://localhost:9999/solr/nonexistent_core` (deliberately
unreachable). Result: `exit code 1`, a `SOLR HARNESS FAILURE` banner
listing all 490 affected queries' `Connection refused (os error 111)`
errors, and no relevance section printed. Raw output preserved at
`docs/research/artifacts/i35_solr_harness_hardening/electronics_adversarial_broken_solr_url.log`.
Before this fix, the same run would have silently printed `native
NDCG@10=0.3041  solr NDCG@10=0.0000` and a fabricated `H0` verdict.

**Scope check**: `phase5-eval`'s and `phase6a-eval`'s own `solr_get`
helpers already `panic!` on both transport and parse failure -- not
affected by this bug, not modified by this fix.

**Decision**: KEEP. See decision doc for full reasoning.
