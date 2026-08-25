# Issue #55 H3 Decision — variant-scoped conjunction correctness on real Product/Variant data

Full protocol: `docs/experiments/ISSUE55_H3_PROTOCOL.md`. Full
log/raw numbers: `docs/experiments/ISSUE55_H3_LOG.md`. Raw artifacts:
`docs/research/artifacts/i55_h3_variant_real_data_correctness/`.

## What was tested

This is Issue #55's own H3 hypothesis, tested directly: `commerce_core`'s
"same-variant conjunction" correctness guarantee (CLAUDE.md's hard rule,
"Product/Variant correctness is non-negotiable") had only ever been
proven on a synthetic, 1-product/2-variant fixture
(`crates/commerce-core/tests/variant_safety.rs`) — neither WANDS nor
ESCI, this project's only real datasets to date, has genuine Product/
Variant structure.

A related but **distinct** gap exists in `docs/decisions/ISSUE47_DECISION.md`
— note that file lives only on the *unmerged* `claude/issue-47-e2d-adaptive-consensus`
branch (PR #53), not on `main` or this branch, so it is cited here from
that PR's content, not a local path. That document's "external validity
against a genuine Product/Variant/relationship dataset remains NOT
ESTABLISHED" finding is about whether Issue #47's own E2d LLM-consensus
*controller* generalizes to a catalog with real variant grouping (its
`worst_case_robust` proof is stated as sound only where
`has_real_variant_grouping=false`) — a narrower, LLM-control-plane
question this experiment does not touch at all (no proposal model, no
E2d controller, no canonicalizer is exercised here). `magento/magento2-sample-data`
was independently named in that document's own "what would be built
next" list as the best-identified real Product/Variant candidate for
that future work, not yet integrated there.

This experiment answers H3 on its own terms — `commerce_core`'s core
execution-correctness guarantee, with no LLM/control-plane involvement —
and, as a side effect, adds the first real Product/Variant dataset
(fetch/prepare scripts, checksums) this repository has, which a future
Issue #47-style experiment could reuse rather than needing to acquire its
own.

This experiment ingested 22 real configurable apparel products (155
variants after a disclosed, deterministic sparsification that creates 138
genuine cross-variant trap opportunities — see protocol §3) into
`commerce_core`'s real domain model, built the real production
`CatalogIndex`, and exhaustively ran every (color, size) combination each
product's own real vocabulary supports — 293 queries total — through both
the naive reference (`Catalog::search`) and the production
`plan::execute_planned`/`CatalogIndex` path, checking each against an
independently computed ground truth.

## Verdict: **KEEP** (confirmed, with a disclosed scope boundary)

**Zero mismatches across all 293 exhaustive queries**, on both
implementations. 138 of those queries were genuine cross-variant traps
(a color real on one variant, a size real on a different variant of the
same product, never co-occurring) — every one correctly returned nothing
for that product (while still correctly returning genuine matches on
*other* products that legitimately share the same combination — 41 such
cross-product combinations were exercised). The variant-scoped
conjunction guarantee, previously resting on one synthetic fixture, now
has real-data confirmation through the actual production execution path,
not just the naive per-variant reference implementation.

This directly confirms Issue #55's H3 on real data for the first time. It
does **not** close Issue #47's own external-validity gap (that gap is
about the E2d LLM-consensus controller specifically, unexercised here) —
see "What was tested" above for why these are related but separate
claims.

### Disclosed scope boundary — not a limitation of the verdict, but of its reach

This experiment exercised `ExecutionOutcome::FastPath` only — the
correct, unconditional route for a pure structural conjunction with no
free-text residual, and the literal query shape Issue #55's own H3
example describes ("query = black AND size 9"). `Hybrid`'s variant safety
depends on the same shared `matches_variant` re-verification step
(already covered by Issue #42's R1/R2/R3 work) and was not independently
re-tested here — doing so meaningfully would require wiring a real
lexical delegate against this dataset, which is a reasonable future
increment, not required to answer this round's question.

### Disclosed methodology note — sparsification, not fabrication

Magento's real fixture data enumerates a full color x size cartesian
product per product, which has zero within-product trap opportunities by
construction. A deterministic checkerboard rule (documented in
`scripts/datasets/prepare_magento_configurable.py`) removes roughly half
of each product's combinations, verified by an assertion to still
preserve every real color/size value's presence somewhere in the
product. Every product name, attribute value, and category is untouched
real data; only which combinations count as an in-stock SKU is a
disclosed modification, deterministic and reproducible, analogous to how
real retail catalogs do not stock every color in every size.

## Action taken

- New eval crate `crates/issue55-eval` and new dataset scripts committed;
  zero `commerce_core` production code changed.
- `docs/decisions/README.md`'s chronology gains an entry for this
  checkpoint.
- No GitHub issue to close — this directly answers Issue #55's own H3
  hypothesis, tracked in the epic itself, not a standalone issue.

## Architecture delta

**Positive evidence for the architecture's stability/correctness pillar**:
the variant-scoped conjunction guarantee — a hard, non-negotiable
correctness rule — now has real-data confirmation, not just synthetic-
fixture confirmation, through the actual production serving path. This
does not change any structural design; it removes a previously-flagged
"not established" caveat from the evidence base. The disclosed scope
boundary (FastPath only; Hybrid untested here) is recorded as a residual
open thread, not folded into an overclaimed "variant safety is fully
proven everywhere" statement.
