# Issue #55 A2 — a credible, auditable promotion oracle for ProductType hyponym relations

Full artifact: `docs/research/artifacts/i55_a2_promotion_oracle/oracle_v1.json`
(317 rows, one per candidate pair). Reproduction:
`cargo run --release -p issue55-eval --bin i55_a2_promotion_oracle`.

## Governing directive

The same repository-owner closure directive that required A1
(`ISSUE55_HYPONYM_PROMOTION_GATE_DECISION.md`) required, as A2: "Build a
credible, auditable promotion oracle/adjudication set (positives,
negatives, ambiguous/unresolved, reachable triggers). Do not infer
`zero false promotions` from only the two inherited known-bad pairs."

A1 built the gate (only a recorded PROMOTE verdict may become a live
`ProductTypeAny` route) but deliberately shipped with an **empty**
promoted set — current production, as of A1, has zero active hyponym
expansions. A2 is what actually populates that set with real,
individually-justified adjudications for the full live candidate pool
(149 groups / 317 pairs), not just the two pairs already known bad.

## What the oracle does

A single Rust binary, `crates/issue55-eval/src/bin/i55_a2_promotion_oracle.rs`
— not a Python probe, unlike the two prior scoping scripts whose
methodology it reuses — because every input it needs
(`product_type_hyponym_groups`, the reachability check, the raw
per-product category-depth data) is already available from the exact
same production Rust code and the exact same `phase6a_eval::data::load_catalog`
JSONL this project's other Issue #55 diagnostics already load. No
separate CSV parse, no cross-language re-derivation risk.

For every one of the 317 candidate pairs, in priority order:

1. **REJECT** — the two confirmed cross-family false positives
   (`"beds"` → `"cat beds"`/`"dog beds & mats"`,
   `ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md` /
   `ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md`). An explicit,
   named override — not inferred from evidence absence.
2. **UNRESOLVED (ambiguous)** — the one disclosed low-practical-risk
   edge case (`"accent chests / cabinets"` → `"dartboards and
   cabinets"`), reachable only via an exact taxonomy-label string a
   free-text query would essentially never produce. Also an explicit
   override: the mechanical evidence below would otherwise have
   promoted it.
3. **UNRESOLVED (unreachable)** — the broader term does not compile to
   a `ProductTypeAny` via its own literal text even when every
   candidate is hypothetically promoted (reuses
   `i55_hyponym_reachability_audit`'s own check directly). Promoting
   these has zero query-time effect either way, so there is no reason
   to accept any promotion risk for them. 136/317 pairs (43%).
4. **PROMOTE** — reachable, and BOTH independent category-hierarchy
   overlap evidence sources (top-level, 2-level ancestor path,
   porting `scripts/research/i55_promotion_gate_ancestor_structure_probe.py`'s
   already-validated methodology) agree the broader and narrower names
   share a common catalog-derived ancestor. Requiring **agreement
   between two independent sources**, not just one, is the concrete
   answer to "do not infer zero false promotions from only the two
   known-bad pairs": every PROMOTE verdict is corroborated twice over,
   not asserted from a single signal, and named as future work in the
   original Priority-2 scoping log
   (`ISSUE55_SEMANTIC_PROMOTION_LOG.md`: "agreement among >=2
   independent sources").
5. **UNRESOLVED (no evidence / no overlap / disagreement)** —
   everything else reachable. Per this project's own stated severity
   asymmetry ("a promotion error is substantially more serious than
   leaving a relation unresolved"), this is the safe failure mode, not
   a defect.

## Result

317 pairs total, 181 reachable / 136 unreachable (moot):

| Verdict | Count | % of reachable |
|---|---|---|
| PROMOTE | 113 | 62.4% |
| REJECT | 2 | 1.1% |
| UNRESOLVED (evidence disagrees / no overlap) | 57 | 31.5% |
| UNRESOLVED (no category-hierarchy evidence) | 8 | 4.4% |
| UNRESOLVED (disclosed ambiguous edge case) | 1 | 0.6% |
| UNRESOLVED (unreachable, 136/317 overall) | — | (not part of the 181) |

**Zero known false promotions**: both confirmed-bad pairs and the
disclosed edge case are excluded by explicit override, confirmed by two
independent means — the verdict logic itself, and a second, structural
self-check inside the same binary that builds a real `PromotedHyponyms`
from the 113 PROMOTE rows and mechanically re-asserts
`!promoted_hyponyms.contains(broader, narrower)` for all three, so the
guarantee does not rest on the verdict logic being bug-free in only one
place.

A direct read through all 113 PROMOTE pairs found nothing resembling
the "beds"/pet-products cross-family shape — every promoted pair reads
as a genuine subtype relation (e.g. `"desks" -> "kids desks"`,
`"recliners" -> ".../ recliners / gray recliners"`,
`"trunks" -> "storage & organization / storage furniture / storage
trunks"`). This read is additional to, not a replacement for, the
category-overlap evidence gate itself.

**Cross-validated against the full-scale single-source probe already on
record**: `top_level`/`level_2` overlap counts among all 317 pairs
reproduce `ISSUE55_PROMOTION_GATE_FULL_SET_DECISION.md`'s own recorded
numbers exactly (202 top-level, 196 level-2, 16 no-evidence for both) —
confirming this Rust port is methodologically faithful to the
already-validated Python probe, not a new, independently-drifted
implementation.

Every PROMOTE verdict here is additionally a subset of the 79 reachable
groups `ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md` already read in
full by direct human inspection (not sampled) in a prior checkpoint —
this mechanical, two-source-agreement rule is corroborating evidence on
top of that completed manual read, not a replacement for it.

## A real bug found and fixed before trusting any number

The first version of this binary reconstructed each product's category
path by joining its `category_depth_1..6` fields with `" / "` again.
This is wrong: each `category_depth_N` field is **already the full
cumulative path** from the root down to depth N (e.g.
`category_depth_2 = "Furniture / Living Room Furniture"`, not just
`"Living Room Furniture"`) — the same convention
`phase6a-eval::catalog::effective_product_class` already relies on
(`category_depths().last()`). Joining the fields again silently
double-concatenated every path (e.g. `"Furniture / Furniture / Living
Room Furniture / Furniture / Living Room Furniture / Chairs & Seating /
..."`), which happened to leave `top_level` (first segment) correct by
accident but corrupted every `level_2` comparison beyond the first
segment. This produced only 14 PROMOTE verdicts before the fix — an 8x
undercount discovered by cross-checking the Rust port's raw overlap
counts against `ISSUE55_PROMOTION_GATE_FULL_SET_DECISION.md`'s already-
published 202/196 numbers (a mismatch that should not have existed for
a faithful port) rather than trusting the first plausible-looking
output. Fixed by using `category_depths().last()` directly, as the
existing ingestion code already does; the corrected run reproduces
202/196 exactly. Disclosed here rather than silently corrected, per
this project's own discipline of preserving methodology mistakes rather
than rewriting history.

## What this deliberately does NOT establish, disclosed rather than smoothed over

- **This still does not wire any promotion into a live `SemanticContext`.**
  A2 builds the oracle and demonstrates (via the in-binary self-check)
  that its PROMOTE rows compile into a real, safe `PromotedHyponyms` —
  it does not change `compile_lexicon`'s default or any eval binary's
  measurement path. Wiring a real promoted set into whatever compiles
  the production/measurement lexicon, and re-measuring candidate-set
  recall/NDCG with it, is separate follow-on work, not done here.
- **"Zero false promotions" here means zero *known* false promotions
  among the 113,** verified by explicit override plus a direct read of
  the full PROMOTE list plus agreement with the two independent
  category-overlap sources plus consistency with a prior session's
  independent full manual read of the entire 181-pair reachable set. It
  is not a formal proof, and not a claim that some subtler cross-family
  relation could never exist among the 113 — the same epistemic caveat
  every prior checkpoint in this thread has carried forward honestly.
- **Precision/coverage, stated plainly**: of the 181 practically-
  relevant (reachable) pairs, 62.4% are resolved PROMOTE, 1.1% REJECT,
  36.5% UNRESOLVED (safe fallback, not a wrong answer). Of all 317
  candidates including the 136 unreachable ones, 35.6% get any real
  resolution at all — the remaining 43% are moot by construction
  (unreachable), not a coverage failure.
- **The two-independent-source-agreement bar is a design choice, not
  the only defensible one.** A single-source (`top_level`-only) rule
  would add only 5 more reachable pairs beyond the two-source set (118
  vs. 113, after the category-path bug fix — a much smaller gap than it
  first appeared before that fix), none of them the known-bad pairs;
  this was checked directly rather than assumed, and the two-source bar
  was kept as the more conservative, doubly-corroborated choice given
  how close the two options turned out to be.
- **`i55_hyponym_reachability_audit.rs` required a small, necessary,
  disclosed fix as a direct consequence of A1**: its own reachability
  question ("does this literal text compile to a `ProductTypeAny`
  today") became vacuous once `compile_lexicon`'s default promoted set
  went empty. Fixed to compile against a hypothetically-fully-promoted
  lexicon instead (via the new `promote_all_hyponym_candidates_unadjudicated`
  helper A1 already added for exactly this purpose), and reproduces its
  own prior `run1.txt` artifact byte-for-byte after the fix — confirmed
  directly, not assumed.

## Next step (named, not implemented here)

Wire a real `PromotedHyponyms` (built from this oracle's 113 PROMOTE
rows, or a filtered/reviewed subset of them) into an actual measurement
path, and re-run the candidate-set recall / NDCG measurements checkpoint
14 and 22 originally reported, to learn how much of that previously-
measured benefit a genuinely adjudicated (rather than blanket) promotion
set actually recovers. Not done here so A2's own oracle-construction
work stays independently reviewable from any recall claim built on top
of it.
