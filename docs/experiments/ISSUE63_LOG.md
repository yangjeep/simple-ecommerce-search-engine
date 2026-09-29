# Issue #63 (Infra E3, amended) — Primitive CPU-efficiency and bytes-touched: log

Append-only. The preregistration is GitHub issue #63: the original body, **amendment 1 (2026-09-29)** appended to it, and **clarification C1** (issue comment, the same day). The verdict is in `docs/decisions/ISSUE63_PRIMITIVE_EFFICIENCY_DECISION.md`. Raw evidence is under `artifacts/issue63/results/`.

## 2026-09-29 — preconditions (before any #63 code or measurement)

- **Stale PR queue closed.**
  - #53 closed; #71 and #80 merged.
  - PR #59's unique Issue #57 content was salvaged onto fresh `main` in PR #81: 13 cherry-picked commits, mechanical conflict resolution, and a dated salvage note. A fresh focused review found three wording inaccuracies in that note, and they were fixed before merge. #81 was merged as `31a82e9`, and #59 was closed as superseded.
  - The only open PR is an unrelated dependabot bump (#58).
- **Final `main` gate** at `31a82e9`, run locally from a clean detached checkout:
  - `cargo fmt --all -- --check`: clean.
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
  - `cargo test --workspace --all-features`: 786 passed, 0 failed.
  - `cargo build --workspace --release`: ok.
  - GitHub CI on `31a82e9`: `quality-gate` and every CodeQL job succeeded.
- **Reproducible binaries.** The release `e3b_native_plp_server` / `e3b_native_measure` / `e3b_correctness_gate` built from `31a82e9` have the same sha256 as the copies built earlier from the byte-identical salvage-branch tree: `ab24b45b…`, `2aa90330…`, `ead705bc…`.
- **Amendment 1 and clarification C1** were posted to #63 before any #63 code existed.
- **Host:** `athos-dev`, Xeon D-1518, 4 logical CPUs, 30 GB, and the #79 cgroup-v2 user-scope envelope. `perf_event_paranoid=4` with no sudo, so no hardware counters are available (amendment §1). They are not used.
- **#79 revalidation on final `main`** (see `ISSUE79_LOG.md`, 2026-09-29):
  - the gate repeats 1,680 checks with 0 failures;
  - outputs and planner paths are identical;
  - FH1/FH2/SH1 CPU fall within the declared band;
  - FH3/FH4 are flagged (1.62x / 1.49x the #79 headline FINAL) and kept flagged.

  The investigation found no server code change. The same level appears in #79's own fidelity window, and an interleaved N0 control is not inflated. The spread is per-launch, on the match-all and dense-counting phases. It is the reason Part A reports per-launch CV/min/max for every arm.

## 2026-09-29 — implementation (before any preregistered measurement)

All of it is additive. #79 FINAL's request, response and outputs are unchanged. The RED tests were written first and failed to compile, because the API did not exist yet.

- **`commerce-core`** (engine):
  - `CandidateSet { All { count }, Set(bitmap) }` and `CatalogIndex::candidate_set`. The match-all case stays logical. An indexable constraint that matches nothing is an empty `Set`, never `All`.
  - `all_ordinals_bitmap` (P0, identical to the private `all_ordinals`) and `all_ordinals_bitmap_by_range` (P0r).
  - Dense full-catalog facet paths: `facet_counts_ordinal_all` (a sequential column scan) and `facet_counts_bitmap_all` (value-bitmap `len()`).
  - `sort::top_k_presorted_all`: a presorted walk without a membership test.
  - Read-only views: `enum_column`, `enum_dictionary`, `enum_value_bitmap`.
  - Tests: `tests/i63_match_all.rs` (6 tests, including exhaustive `top_k_presorted_all` ≡ `top_k_presorted(full)` over ties, ±0.0, NaN and missing values, in both directions and at every limit).
- **FINAL-path refactors, disclosed.** Two changes touch code the FINAL path executes.
  - `indexed_candidates` now calls a shared private `indexable_intersection`; the logic is unchanged.
  - `top_k_presorted` now calls a generic `presorted_walk` with a `candidates.contains` closure and `candidates.iter()` tail; the behaviour is unchanged.
  - Part A's native FINAL arm is therefore served by the #63 binary, not #79's. Its output identity with #79 FINAL is checked through the equivalence dumps and fingerprints.
- **`issue79-eval`:**
  - `plp::CandidateMode` (`cand_mode=p0|p0r|p1|p2|p2b`, default `p0`, which is #79 FINAL verbatim). Non-P0 modes run the FINAL facet/sort algorithms over the B1 representation, and legacy modes are rejected for them.
  - The new `Diag` fields are skipped when unset, so P0's response JSON is byte-identical to #79's.
  - Server flag `--prebuilt-all true` (the P1 bitmap; its build time and bytes appear on the ready line).
  - Gate: `--cand-modes` (default `p0` reproduces #79) and `--include-reference true`.
  - Driver: mode syntax `facet:sort[:cand]`, plus `I63_DUMP_DIR` dumps written after the timed batch.
  - `cells::reference_cells()` returns #77's `filter_depth_{1,3,5}`, with byte-identical native request strings (tested). It is kept out of `all_cells()`.
  - `tests/i63_cand_modes.rs`: 650 oracle checks (10 requests × 5 cand modes × 13 facet/sort/constant combinations) on a 60-product multi-variant fixture.
- **`issue77-eval` `i77_measure`**, opt-in only. Unset, every #77/#79 request is byte-identical.
  - `I63_EQUAL_WORK=1` sets Solr JSON facets to `limit: -1`. Meilisearch equal work is #79's existing `I77_MEILI_LIKE_FOR_LIKE=1`.
  - `I63_DUMP_DIR` issues one untimed request per cell before the warmups and dumps the normalized `num_found`, complete facet maps, hit keys and backend request count.
  - New raw field `i63_equal_work`.
- **New `issue63-eval` crate:**
  - a counting global allocator;
  - the #74-floor runner (`CLOCK_THREAD_CPUTIME_ID`; ≥200 ops and ≥2 s CPU per point; chunked clock reads);
  - analytical bytes-touched estimators;
  - the equal-work `equivalence::compare` (exact maps and exact `num_found`; hits must be ID-only: `id`, plus native's `sort_value`);
  - the deterministic synthetic multi-variant expansion (`synthetic`), which includes cross-variant "trap" products;
  - binaries `i63_primitives` (B1/B2/C), `i63_facet_equivalence` and `i63_host_probe`;
  - 9 unit tests.
- **Driver** `scripts/issue63/run_i63.sh` (`gate`, `micro`, `confirm` phases) and **analysis** `scripts/issue63/analyze_i63.py`, which implements the §3.4 adoption rule and the §2 classification as preregistered.
- **Disclosed interpretation of §3.4's tie-break.**
  - P2/P2b keep the P1 bitmap for candidate top-K over match-all, so their persistent memory is P1's, not 0.
  - If the within-5% tie set is also tied on memory, the larger FH3+FH4 reduction stands.
- **Disclosed scope note.** No facet-loop change (for example u32 counters) was implemented as a server mode, so none is eligible for N⁺. B2 characterizes u32 counters only.

## 2026-09-29 — smoke checks and early exposure (disclosed; not evidence)

- **Harness smoke tests** at 100k, before any preregistered measurement. They cover the microbenchmark with a tiny floor, all Part A arms with dumps, and the equivalence checker, plus a negative control (Meilisearch's default configuration, correctly flagged `NOT_EQUIVALENT_WORK`). Outputs are under `artifacts/issue63/results/smoke_100k_not_evidence/`, without timings.
- **Early exposure, as in #79's log.**
  - The analysis script was dry-run on those smoke outputs to validate its tables.
  - That run displayed single-launch, 100k, tiny-floor native-vs-competitor CPU figures and microbench figures.
  - They were seen after amendment 1 and C1 were posted, and before N⁺ was frozen and before Part A.
  - No rule, threshold, cell or configuration depends on them: the §3.4 adoption rule is native-only and mechanical, and the Part A classification is fixed.
  - They are not reported as results.

## 2026-09-29 — binary provenance for every #63 measurement

- Every #63 binary was built once, from commit `b66af57`.
- Every later commit on the branch touches only `docs/`, `scripts/` and `artifacts/`: `git diff --stat b66af57 HEAD -- crates Cargo.toml Cargo.lock` is empty.
- Raw files record `git rev-parse HEAD` at the time they are written, so they can name a later docs/scripts commit. The Rust source is `b66af57`'s regardless.
- sha256 prefixes:

| binary | sha256 prefix |
|---|---|
| `i63_primitives` | `4d9dfc318bdf9123` |
| `i63_facet_equivalence` | `c7342392e79020b4` |
| `i63_host_probe` | `559dd17759f1b828` |
| `e3b_native_plp_server` | `4628bb7de505e1c1` |
| `e3b_native_measure` | `75db0278de85f26d` |
| `e3b_correctness_gate` | `396ad70b1fc06be4` |
| `i77_measure` | `5d7e7aec64881269` |

- Native Part A records its server's full sha256 in every raw file.
- No binary is rebuilt while a #63 phase runs. The release build is not touched until #63's measurements are complete.
