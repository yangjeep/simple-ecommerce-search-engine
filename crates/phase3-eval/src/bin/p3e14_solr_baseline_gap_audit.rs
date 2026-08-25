//! Issue #14 Phase 3 -- throwaway diagnostic (NOT a permanent experiment
//! binary): audits how much of `round1_eval::solr::solr_query_for`'s
//! "fair Solr baseline" construction silently drops structural signal
//! once `residual_lexical` is non-empty.
//!
//! `solr_query_for` sets `q` = the full original query text ONLY when
//! `residual_lexical` is empty; once `residual_lexical` is non-empty, `q`
//! becomes ONLY `residual_lexical.join(" ")`. Structural signal only ever
//! reaches Solr via `extract_brand_color`'s `fq`, which captures exactly:
//!   - `StructuralConstraint::Brand(id)` (if the id resolves to a name)
//!   - `StructuralConstraint::BrandAny(ids)` (via `ids.first()`, if that
//!     id resolves to a name)
//!   - `Constraint::Enum { attribute: "color", .. }` (the ONLY Attribute
//!     variant it matches -- `MultiEnumContains`/`Boolean`/`Numeric`/
//!     `Text`, even on attribute "color", are NOT captured)
//!     Every other `query.constraints` entry -- `ProductType`, `Category`,
//!     `PriceUnderCents`, `PriceOverCents`, any non-"color" `Enum`, or any
//!     non-`Enum` Attribute constraint -- has no `fq` substitute and is
//!     silently absent from Solr's `q`/`fq` entirely once `residual_lexical`
//!     is non-empty. Ambiguous spans (`query.ambiguous`) are absent from both
//!     `q` and `fq` unconditionally (never resolved to structure at all).
//!
//! Reports four disjoint/overlapping populations (see module doc / task
//! for exact definitions):
//!   1. P3-E05's anchored-lexical population (UnresolvedResidual-rejected
//!      + non-empty query.constraints): fraction with >=1 constraint that
//!        has NO fq substitute per the classification above.
//!   2. P3-E09's single-token-lexical population: sanity check that its
//!      constraints/ambiguous are always empty (P3-E09 unaffected).
//!   3. Ambiguous-rejected + non-empty residual_lexical, restricted the
//!      same way P3-E12 restricts its own tractable population (single
//!      ambiguous span, all-Constraint candidates, top_freq>0): raw
//!      count, cross-checked against P3-E12's own reported 4,279/4,356.
//!   4. Whole 22,458-query corpus: fraction with non-empty
//!      residual_lexical AND (>=1 no-fq-substitute constraint OR
//!      non-empty ambiguous).
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e14_solr_baseline_gap_audit
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::BTreeMap;
use std::path::PathBuf;

use commerce_core::admission::{admit, AdmissionDecision, AdmissionPolicy, RejectReason};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::Constraint;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::lexicon::ResolvedTerm;
use commerce_core::ir::structural::StructuralConstraint;
use commerce_core::ir::{compile, CommerceQuery, ResolvedConstraint};
use round1_eval::catalog;
use round1_eval::data::{self, EsciLabel};

/// Whether this single `ResolvedConstraint` has NO substitute in
/// `extract_brand_color`'s `fq` -- i.e. it would be silently dropped from
/// Solr's query entirely once `residual_lexical` is non-empty. Mirrors
/// `round1_eval::solr::extract_brand_color`'s two match arms exactly:
/// only `Structural::Brand`, `Structural::BrandAny`, and
/// `Attribute(Constraint::Enum { attribute: "color", .. })` are captured.
fn has_no_fq_substitute(c: &ResolvedConstraint) -> bool {
    match c {
        ResolvedConstraint::Structural(StructuralConstraint::Brand(_)) => false,
        ResolvedConstraint::Structural(StructuralConstraint::BrandAny(_)) => false,
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(_)) => true,
        ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(_)) => true,
        ResolvedConstraint::Structural(StructuralConstraint::Category(_)) => true,
        ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(_)) => true,
        ResolvedConstraint::Structural(StructuralConstraint::PriceOverCents(_)) => true,
        ResolvedConstraint::Attribute(Constraint::Enum { attribute, .. }) => attribute != "color",
        ResolvedConstraint::Attribute(Constraint::MultiEnumContains { .. }) => true,
        ResolvedConstraint::Attribute(Constraint::Boolean { .. }) => true,
        ResolvedConstraint::Attribute(Constraint::Numeric { .. }) => true,
        ResolvedConstraint::Attribute(Constraint::Text { .. }) => true,
    }
}

/// Human-readable kind label for the breakdown table, independent of
/// whether it has an fq substitute.
fn kind_label(c: &ResolvedConstraint) -> String {
    match c {
        ResolvedConstraint::Structural(StructuralConstraint::Brand(_)) => {
            "Structural::Brand".into()
        }
        ResolvedConstraint::Structural(StructuralConstraint::BrandAny(_)) => {
            "Structural::BrandAny".into()
        }
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(_)) => {
            "Structural::ProductType".into()
        }
        ResolvedConstraint::Structural(StructuralConstraint::ProductTypeAny(_)) => {
            "Structural::ProductTypeAny".into()
        }
        ResolvedConstraint::Structural(StructuralConstraint::Category(_)) => {
            "Structural::Category".into()
        }
        ResolvedConstraint::Structural(StructuralConstraint::PriceUnderCents(_)) => {
            "Structural::PriceUnderCents".into()
        }
        ResolvedConstraint::Structural(StructuralConstraint::PriceOverCents(_)) => {
            "Structural::PriceOverCents".into()
        }
        ResolvedConstraint::Attribute(Constraint::Enum { attribute, .. }) => {
            if attribute == "color" {
                "Attribute::Enum(color)".into()
            } else {
                format!("Attribute::Enum({attribute})")
            }
        }
        ResolvedConstraint::Attribute(Constraint::MultiEnumContains { attribute, .. }) => {
            format!("Attribute::MultiEnumContains({attribute})")
        }
        ResolvedConstraint::Attribute(Constraint::Boolean { attribute, .. }) => {
            format!("Attribute::Boolean({attribute})")
        }
        ResolvedConstraint::Attribute(Constraint::Numeric { attribute, .. }) => {
            format!("Attribute::Numeric({attribute})")
        }
        ResolvedConstraint::Attribute(Constraint::Text { attribute, .. }) => {
            format!("Attribute::Text({attribute})")
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let queries_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/queries.jsonl"));

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: BTreeMap<u64, (String, BTreeMap<String, EsciLabel>)> = BTreeMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), BTreeMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
    }
    judged_by_query.retain(|_, (_, judged)| judged.values().any(|l| l.is_relevant()));
    let total = judged_by_query.len();
    println!("{total} distinct real judged queries with at least one relevant label loaded (this is the corpus every other Phase 3 binary in this dir uses)");

    println!("\ncompiling every real query...");
    let compiled_cache: BTreeMap<u64, CommerceQuery> = judged_by_query
        .iter()
        .map(|(&qid, (text, _))| (qid, compile(text, &lexicon)))
        .collect();

    let unlimited_policy = AdmissionPolicy {
        max_candidates: usize::MAX,
    };

    // ---------------------------------------------------------------
    // Population 1: P3-E05's anchored-lexical population.
    // ---------------------------------------------------------------
    let mut anchored_pop = 0usize;
    let mut anchored_with_gap = 0usize;
    let mut anchored_kind_counts: BTreeMap<String, usize> = BTreeMap::new();
    for compiled in compiled_cache.values() {
        let AdmissionDecision::Reject(RejectReason::UnresolvedResidual) =
            admit(compiled, &index, &unlimited_policy)
        else {
            continue;
        };
        if compiled.constraints.is_empty() {
            continue;
        }
        anchored_pop += 1;
        let mut has_gap = false;
        for c in &compiled.constraints {
            if has_no_fq_substitute(c) {
                has_gap = true;
                *anchored_kind_counts.entry(kind_label(c)).or_insert(0) += 1;
            }
        }
        if has_gap {
            anchored_with_gap += 1;
        }
    }
    println!("\n=== Population 1: P3-E05 anchored-lexical population (UnresolvedResidual-rejected AND query.constraints non-empty) ===");
    println!("  population size: {anchored_pop}");
    println!(
        "  with >=1 constraint that has NO fq substitute (per extract_brand_color's real coverage): {anchored_with_gap}/{anchored_pop} ({:.2}%)",
        if anchored_pop > 0 {
            anchored_with_gap as f64 / anchored_pop as f64 * 100.0
        } else {
            0.0
        }
    );
    println!("  breakdown of no-fq-substitute constraint kinds found (a query may contribute to more than one kind):");
    for (kind, count) in &anchored_kind_counts {
        println!("    {kind}: {count}");
    }

    // ---------------------------------------------------------------
    // Population 2: P3-E09's single-token-lexical population sanity
    // check -- confirm constraints/ambiguous are always empty.
    // ---------------------------------------------------------------
    let mut single_token_pop = 0usize;
    let mut single_token_nonempty_constraints = 0usize;
    let mut single_token_nonempty_ambiguous = 0usize;
    for compiled in compiled_cache.values() {
        if compiled.residual_lexical.len() != 1 {
            continue;
        }
        if !compiled.constraints.is_empty() {
            continue;
        }
        if !compiled.ambiguous.is_empty() {
            continue;
        }
        single_token_pop += 1;
        // (by construction both are empty here; counters kept at 0 as a
        // tautological sanity check that the filter above is exhaustive)
        if !compiled.constraints.is_empty() {
            single_token_nonempty_constraints += 1;
        }
        if !compiled.ambiguous.is_empty() {
            single_token_nonempty_ambiguous += 1;
        }
    }
    println!("\n=== Population 2: P3-E09 single-token-lexical population sanity check (residual_token_count==1, constraints empty, ambiguous empty) ===");
    println!("  population size: {single_token_pop}");
    println!(
        "  of these, nonempty constraints: {single_token_nonempty_constraints} (must be 0 by the filter's own definition)"
    );
    println!(
        "  of these, nonempty ambiguous: {single_token_nonempty_ambiguous} (must be 0 by the filter's own definition)"
    );
    // Independent, non-tautological version: same residual_token_count==1
    // filter WITHOUT pre-filtering on constraints/ambiguous, to show P3-E09's
    // own restriction never actually excludes anything.
    let mut single_token_raw = 0usize;
    let mut single_token_raw_nonempty_constraints = 0usize;
    let mut single_token_raw_nonempty_ambiguous = 0usize;
    for compiled in compiled_cache.values() {
        if compiled.residual_lexical.len() != 1 {
            continue;
        }
        single_token_raw += 1;
        if !compiled.constraints.is_empty() {
            single_token_raw_nonempty_constraints += 1;
        }
        if !compiled.ambiguous.is_empty() {
            single_token_raw_nonempty_ambiguous += 1;
        }
    }
    println!(
        "  independent check (residual_token_count==1 alone, no pre-filter): {single_token_raw} queries; nonempty constraints: {single_token_raw_nonempty_constraints}; nonempty ambiguous: {single_token_raw_nonempty_ambiguous}"
    );

    // ---------------------------------------------------------------
    // Population 3: Ambiguous-rejected + non-empty residual_lexical,
    // restricted to P3-E12's own tractable-population definition, as a
    // cross-check against its reported 4,279/4,356.
    // ---------------------------------------------------------------
    let mut ambiguous_count = 0usize;
    let mut excluded_multi_span = 0usize;
    let mut excluded_not_all_constraint = 0usize;
    let mut excluded_zero_top_freq = 0usize;
    let mut residual_nonempty_after_single_span_all_constraint = 0usize;
    let mut single_span_all_constraint_population = 0usize;
    for compiled in compiled_cache.values() {
        let AdmissionDecision::Reject(reason) = admit(compiled, &index, &unlimited_policy) else {
            continue;
        };
        if reason != RejectReason::Ambiguous {
            continue;
        }
        ambiguous_count += 1;
        if compiled.ambiguous.len() != 1 {
            excluded_multi_span += 1;
            continue;
        }
        let span = &compiled.ambiguous[0];
        let constraint_candidates: Vec<_> = span
            .candidates
            .iter()
            .filter_map(|c| match &c.resolved {
                ResolvedTerm::Constraint(rc) => Some(rc.clone()),
                ResolvedTerm::Preference(_) => None,
            })
            .collect();
        if constraint_candidates.len() != span.candidates.len() || constraint_candidates.len() < 2 {
            excluded_not_all_constraint += 1;
            continue;
        }
        single_span_all_constraint_population += 1;

        // Same frequency-dominance computation P3-E12 does, purely to
        // reproduce its exact exclusion order (residual_lexical does not
        // depend on which candidate wins, so this only affects which
        // denominator population the residual check runs against).
        let mut freqs: Vec<(usize, u64)> = constraint_candidates
            .iter()
            .enumerate()
            .map(|(i, c)| (i, index.indexed_candidates(std::slice::from_ref(c)).len()))
            .collect();
        freqs.sort_unstable_by_key(|&(_, f)| std::cmp::Reverse(f));
        let (_, top_freq) = freqs[0];
        if top_freq == 0 {
            excluded_zero_top_freq += 1;
            continue;
        }

        if !compiled.residual_lexical.is_empty() {
            residual_nonempty_after_single_span_all_constraint += 1;
        }
    }
    println!("\n=== Population 3: ambiguous-rejected + non-empty residual_lexical, P3-E12 tractable-population cross-check ===");
    println!("  total ambiguous-rejected queries: {ambiguous_count}");
    println!("  excluded (multi-span): {excluded_multi_span}");
    println!("  excluded (not all candidates are Constraint / <2 candidates): {excluded_not_all_constraint}");
    println!(
        "  single-span, all-Constraint population (P3-E12's reported '4,356'): {single_span_all_constraint_population}"
    );
    println!(
        "  of those, excluded (winner's own top catalog frequency == 0): {excluded_zero_top_freq}"
    );
    println!(
        "  of those remaining, residual_lexical still non-empty (P3-E12's reported 'excluded_residual_still_nonempty', target '4,279'): {residual_nonempty_after_single_span_all_constraint}"
    );
    println!(
        "  cross-check ratio: {residual_nonempty_after_single_span_all_constraint}/{single_span_all_constraint_population} ({:.2}%)",
        if single_span_all_constraint_population > 0 {
            residual_nonempty_after_single_span_all_constraint as f64
                / single_span_all_constraint_population as f64
                * 100.0
        } else {
            0.0
        }
    );

    // ---------------------------------------------------------------
    // Population 4: whole-corpus bound.
    // ---------------------------------------------------------------
    let mut whole_nonempty_residual = 0usize;
    let mut whole_affected = 0usize;
    let mut whole_affected_via_constraint_gap = 0usize;
    let mut whole_affected_via_ambiguous = 0usize;
    for compiled in compiled_cache.values() {
        if compiled.residual_lexical.is_empty() {
            continue;
        }
        whole_nonempty_residual += 1;
        let has_constraint_gap = compiled.constraints.iter().any(has_no_fq_substitute);
        let has_ambiguous = !compiled.ambiguous.is_empty();
        if has_constraint_gap {
            whole_affected_via_constraint_gap += 1;
        }
        if has_ambiguous {
            whole_affected_via_ambiguous += 1;
        }
        if has_constraint_gap || has_ambiguous {
            whole_affected += 1;
        }
    }
    println!("\n=== Population 4: whole-corpus bound (non-empty residual_lexical AND (>=1 no-fq-substitute constraint OR non-empty ambiguous)) ===");
    println!("  whole corpus size: {total}");
    println!(
        "  non-empty residual_lexical: {whole_nonempty_residual} ({:.2}% of corpus)",
        whole_nonempty_residual as f64 / total as f64 * 100.0
    );
    println!(
        "  ...of those, via a no-fq-substitute structural/attribute constraint: {whole_affected_via_constraint_gap}"
    );
    println!("  ...of those, via a non-empty ambiguous span: {whole_affected_via_ambiguous}");
    println!(
        "  TOTAL potentially affected (union): {whole_affected}/{total} ({:.2}% of whole corpus)",
        whole_affected as f64 / total as f64 * 100.0
    );

    println!("\ndone.");
}
