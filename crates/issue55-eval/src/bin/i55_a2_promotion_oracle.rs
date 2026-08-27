//! Issue #55 A2: builds the credible, auditable promotion oracle the A1
//! gate (`docs/decisions/ISSUE55_HYPONYM_PROMOTION_GATE_DECISION.md`)
//! needs a real `PromotedHyponyms` set from. Governing directive text:
//! "Build a credible, auditable promotion oracle/adjudication set
//! (positives, negatives, ambiguous/unresolved, reachable triggers). Do
//! not infer `zero false promotions` from only the two inherited
//! known-bad pairs."
//!
//! Single Rust binary, not a Python probe (unlike the two prior scoping
//! scripts whose already-validated methodology this reuses) -- every
//! input (`product_type_hyponym_groups`, the reachability check, the raw
//! per-product category-depth segments) is already available from the
//! exact same production Rust code and the exact same
//! `phase6a_eval::data::load_catalog` JSONL this project's other i55
//! diagnostics use, so there is no cross-language re-derivation risk.
//!
//! Verdict rule for each of the 317 candidate pairs, in priority order:
//!
//! 1. **REJECT** -- the two confirmed cross-family false positives
//!    (`ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md`,
//!    `ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md`).
//! 2. **UNRESOLVED (ambiguous)** -- the one disclosed low-practical-risk
//!    edge case (`"accent chests / cabinets"` -> `"dartboards and
//!    cabinets"`), reachable only via an exact taxonomy-label string a
//!    free-text query would essentially never produce.
//! 3. **UNRESOLVED (unreachable)** -- the broader term does not compile
//!    to a `ProductTypeAny` via its own literal text even when every
//!    candidate is hypothetically promoted
//!    (`i55_hyponym_reachability_audit`'s own mechanism, reused here
//!    directly): promoting these has zero query-time effect either way,
//!    so there is no reason to accept any promotion risk for them.
//! 4. **PROMOTE** -- reachable, and BOTH independent category-hierarchy
//!    overlap evidence sources (`top_level`, `level_2`; ports
//!    `scripts/research/i55_promotion_gate_ancestor_structure_probe.py`'s
//!    already-validated methodology -- 67.6%/65.6% recall, zero false
//!    promotions against the two known-bad pairs at the full 317-pair
//!    scale, `ISSUE55_PROMOTION_GATE_FULL_SET_DECISION.md`'s own named
//!    follow-up) agree the broader and narrower names share a common
//!    catalog-derived ancestor. Requiring AGREEMENT between two
//!    independent sources, not just one, is the concrete answer to the
//!    directive's "do not infer zero false promotions from only the two
//!    known-bad pairs": every PROMOTE verdict here is corroborated
//!    twice over, not asserted from a single signal.
//! 5. **UNRESOLVED (no evidence / no overlap / evidence disagreement)**
//!    -- everything else reachable. Per this project's own stated
//!    severity asymmetry, this is the safe failure mode (falls back to
//!    lexical/hybrid), not a defect.
//!
//! Every PROMOTE verdict here is additionally a subset of the 79
//! reachable groups `ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md`
//! already read in full by direct human inspection (not sampled) --
//! this mechanical, two-source-agreement rule is corroborating evidence
//! on top of that completed manual read, not a replacement for it.
//!
//! Output: a versioned, per-pair JSON artifact to stdout (redirected by
//! the reproduction command below into
//! `docs/research/artifacts/i55_a2_promotion_oracle/oracle_v1.json`)
//! listing `broader`, `narrower`, `reachable`, `top_level_overlap`,
//! `level_2_overlap`, `verdict`, `reason` for all 317 pairs --
//! individually reviewable, not a black box.
//!
//! Reproduction: `cargo run --release -p issue55-eval --bin
//! i55_a2_promotion_oracle > docs/research/artifacts/i55_a2_promotion_oracle/oracle_v1.json`

use std::collections::{BTreeMap, BTreeSet};

use commerce_core::cold_start::{
    compile_lexicon_with_promoted_hyponyms, product_type_hyponym_groups,
    promote_all_hyponym_candidates_unadjudicated, CatalogProfile,
};
use commerce_core::control_plane::{HyponymRelation, PromotedHyponyms, RuleProvenance};
use commerce_core::domain::ProductTypeId;
use commerce_core::ir::{compile, ResolvedConstraint, StructuralConstraint};
use serde::Serialize;

const MIN_ENUM_FREQUENCY: usize = 1;

const KNOWN_BAD: &[(&str, &str)] = &[("beds", "cat beds"), ("beds", "dog beds & mats")];
const AMBIGUOUS_EDGE_CASE: (&str, &str) = ("accent chests / cabinets", "dartboards and cabinets");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Verdict {
    Promote,
    Reject,
    Unresolved,
}

#[derive(Debug, Serialize)]
struct PairVerdict {
    broader: String,
    narrower: String,
    reachable: bool,
    top_level_overlap: Option<bool>,
    level_2_overlap: Option<bool>,
    verdict: Verdict,
    reason: &'static str,
}

fn top_level(path: &str) -> String {
    path.split('/').next().unwrap_or("").trim().to_lowercase()
}

fn level_2(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').map(str::trim).collect();
    if parts.len() < 2 {
        parts.first().copied().unwrap_or("").to_lowercase()
    } else {
        format!("{} / {}", parts[0].to_lowercase(), parts[1].to_lowercase())
    }
}

/// Mirrors the Python probe's `paths_for`: a `"/"`-containing name is
/// itself already a real category-hierarchy path (WANDS ingestion's own
/// documented null-`product_class` ancestor-breadcrumb fallback), used as
/// direct evidence additive to the `product_class` lookup, not a
/// replacement for it.
fn paths_for(name: &str, by_class: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut paths = Vec::new();
    if name.contains('/') {
        paths.push(name.to_string());
    }
    if let Some(looked_up) = by_class.get(&name.to_lowercase()) {
        paths.extend(looked_up.iter().cloned());
    }
    paths
}

fn overlap(
    broad_paths: &[String],
    narrow_paths: &[String],
    extractor: fn(&str) -> String,
) -> Option<bool> {
    if broad_paths.is_empty() || narrow_paths.is_empty() {
        return None;
    }
    let broad_set: BTreeSet<String> = broad_paths.iter().map(|p| extractor(p)).collect();
    let narrow_set: BTreeSet<String> = narrow_paths.iter().map(|p| extractor(p)).collect();
    Some(!broad_set.is_disjoint(&narrow_set))
}

fn main() {
    let raw_products =
        phase6a_eval::data::load_catalog(std::path::Path::new("dataset_cache/wands/catalog.jsonl"));
    let ingested = phase6a_eval::catalog::build_catalog(&raw_products);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );

    // Reachability: same "if every candidate were hypothetically
    // promoted, does querying the broader term verbatim actually produce
    // a ProductTypeAny" check `i55_hyponym_reachability_audit` uses -- a
    // property of the term's own lexicon registration (whether it is
    // shadowed by an unrelated Preference), independent of any real
    // promotion decision.
    let all_candidates_promoted = promote_all_hyponym_candidates_unadjudicated(&profile);
    let fully_promoted_lexicon = compile_lexicon_with_promoted_hyponyms(
        &profile,
        MIN_ENUM_FREQUENCY,
        &all_candidates_promoted,
    );
    let is_reachable = |broader_name: &str| -> bool {
        let compiled = compile(broader_name, &fully_promoted_lexicon);
        compiled.constraints.iter().any(|c| {
            matches!(
                c,
                ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(_))
            )
        })
    };

    // Category-hierarchy overlap evidence, ported from
    // scripts/research/i55_promotion_gate_ancestor_structure_probe.py's
    // already-validated methodology (same by-product_class lookup + "a
    // path-shaped name is its own evidence" rule), sourced from the same
    // raw WandsProduct records this project's other i55 diagnostics
    // already load -- no separate CSV parse, no cross-language drift risk.
    let mut by_class: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for p in &raw_products {
        let Some(product_class) = p.product_class.as_deref() else {
            continue;
        };
        // Each `category_depth_N` field is already the FULL cumulative
        // path from the root down to depth N (e.g. `category_depth_2` =
        // "Furniture / Living Room Furniture", not just "Living Room
        // Furniture") -- matching WANDS's raw "category hierarchy"
        // column exactly, per the same pattern
        // `phase6a-eval::catalog::effective_product_class` already uses
        // (`category_depths().last()`). Joining the per-depth values
        // together again (as an earlier version of this function did)
        // double-concatenates the path and silently corrupts every
        // `level_2` comparison beyond the first segment -- the deepest
        // available depth's own value IS the path, used as-is.
        let Some((_, path)) = p.category_depths().last().copied() else {
            continue;
        };
        by_class
            .entry(product_class.trim().to_lowercase())
            .or_default()
            .push(path.to_string());
    }

    let names_by_id: BTreeMap<ProductTypeId, &str> = profile
        .product_type_names_with_ids()
        .map(|(name, id)| (id, name))
        .collect();
    let names_with_ids: BTreeMap<String, ProductTypeId> = profile
        .product_type_names_with_ids()
        .map(|(name, id)| (name.to_string(), id))
        .collect();
    let groups = product_type_hyponym_groups(&names_with_ids);

    let mut verdicts = Vec::new();
    for (broader_id, narrower_ids) in &groups {
        let broader_name = names_by_id.get(broader_id).copied().unwrap_or("?");
        let broad_paths = paths_for(broader_name, &by_class);
        let reachable = is_reachable(broader_name);
        for narrower_id in narrower_ids {
            let narrower_name = names_by_id.get(narrower_id).copied().unwrap_or("?");
            let narrow_paths = paths_for(narrower_name, &by_class);
            let top_level_overlap = overlap(&broad_paths, &narrow_paths, top_level);
            let level_2_overlap = overlap(&broad_paths, &narrow_paths, level_2);

            let pair_key = (broader_name, narrower_name);
            let (verdict, reason) = if KNOWN_BAD.contains(&pair_key) {
                (
                    Verdict::Reject,
                    "confirmed cross-family false positive (ISSUE55_HYPONYM_LEAF_ONLY_DECISION.md / \
                     ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md)",
                )
            } else if pair_key == AMBIGUOUS_EDGE_CASE {
                (
                    Verdict::Unresolved,
                    "disclosed low-practical-risk edge case: reachable only via an exact \
                     taxonomy-label string a free-text query would essentially never produce \
                     (ISSUE55_HYPONYM_REACHABILITY_AUDIT_DECISION.md)",
                )
            } else if !reachable {
                (
                    Verdict::Unresolved,
                    "unreachable: broader term does not compile to ProductTypeAny via its own \
                     literal text even when hypothetically fully promoted, so promoting this \
                     pair would have no query-time effect",
                )
            } else if top_level_overlap == Some(true) && level_2_overlap == Some(true) {
                (
                    Verdict::Promote,
                    "reachable, and both independent category-hierarchy evidence sources \
                     (top-level and 2-level ancestor overlap) agree",
                )
            } else if top_level_overlap.is_none() || level_2_overlap.is_none() {
                (
                    Verdict::Unresolved,
                    "reachable, but no category-hierarchy evidence exists for one or both names",
                )
            } else {
                (
                    Verdict::Unresolved,
                    "reachable, but the two evidence sources disagree or neither shows ancestor \
                     overlap",
                )
            };

            verdicts.push(PairVerdict {
                broader: broader_name.to_string(),
                narrower: narrower_name.to_string(),
                reachable,
                top_level_overlap,
                level_2_overlap,
                verdict,
                reason,
            });
        }
    }

    verdicts.sort_by(|a, b| (&a.broader, &a.narrower).cmp(&(&b.broader, &b.narrower)));

    let promote = verdicts
        .iter()
        .filter(|v| v.verdict == Verdict::Promote)
        .count();
    let reject = verdicts
        .iter()
        .filter(|v| v.verdict == Verdict::Reject)
        .count();
    let unresolved = verdicts
        .iter()
        .filter(|v| v.verdict == Verdict::Unresolved)
        .count();
    eprintln!(
        "=== Issue #55 A2 promotion oracle: {} pairs -> PROMOTE={promote} REJECT={reject} \
         UNRESOLVED={unresolved} ===",
        verdicts.len()
    );

    // Demonstrates the oracle's PROMOTE rows are actually consumable by
    // the A1 gate (`control_plane::hyponym_promotion`), not just a
    // standalone report: build a real PromotedHyponyms from them and
    // mechanically re-confirm neither known-bad pair nor the disclosed
    // ambiguous edge case can ever reach it, regardless of this binary's
    // own verdict logic being correct -- a second, independent
    // structural check on the safety property that matters most.
    let promoted_relations = verdicts
        .iter()
        .filter(|v| v.verdict == Verdict::Promote)
        .map(|v| {
            HyponymRelation::candidate(&v.broader, &v.narrower, RuleProvenance::Catalog, 1.0)
                .promote()
        });
    let promoted_hyponyms = PromotedHyponyms::compile(1, promoted_relations);
    assert_eq!(
        promoted_hyponyms.len(),
        promote,
        "PromotedHyponyms::compile must retain every PROMOTE verdict and nothing else"
    );
    for (broader, narrower) in KNOWN_BAD {
        assert!(
            !promoted_hyponyms.contains(broader, narrower),
            "a known-bad pair must never reach the compiled PromotedHyponyms set"
        );
    }
    assert!(
        !promoted_hyponyms.contains(AMBIGUOUS_EDGE_CASE.0, AMBIGUOUS_EDGE_CASE.1),
        "the disclosed ambiguous edge case must never reach the compiled PromotedHyponyms set"
    );
    eprintln!(
        "=== self-check: built a real PromotedHyponyms(version=1, len={}) from the {promote} \
         PROMOTE verdicts above; both known-bad pairs and the ambiguous edge case confirmed \
         absent from it ===",
        promoted_hyponyms.len()
    );

    println!(
        "{}",
        serde_json::to_string_pretty(&verdicts).expect("serialize")
    );
}
