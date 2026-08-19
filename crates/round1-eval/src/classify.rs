//! R1-E02: classify real queries into execution categories, measure the
//! Semantic FIB hit rate, and — separately — semantic precision against
//! real ESCI human judgments. `evidence class: real`, `independence: yes`
//! (the lexicon is derived from catalog data via `cold_start`; the query
//! set and its judgments come from Amazon's own held-out test split, not
//! from this project's compiler or lexicon).

use std::collections::{HashMap, HashSet};

use commerce_core::domain::{AttributeValue, Catalog, Constraint, ProductId};
use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};

use crate::data::JudgedExample;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QueryClass {
    ExactIdLookup,
    Ambiguous,
    StructuralOnly,
    StructuralPlusLexical,
    SemanticOccasion,
    LexicalDominant,
    UnresolvedPunt,
}

impl QueryClass {
    pub fn label(self) -> &'static str {
        match self {
            QueryClass::ExactIdLookup => "exact_id_lookup",
            QueryClass::Ambiguous => "ambiguous",
            QueryClass::StructuralOnly => "structural_only",
            QueryClass::StructuralPlusLexical => "structural_plus_lexical",
            QueryClass::SemanticOccasion => "semantic_occasion",
            QueryClass::LexicalDominant => "lexical_dominant",
            QueryClass::UnresolvedPunt => "unresolved_punt",
        }
    }
}

const OCCASION_WORDS: &[&str] = &[
    "gift",
    "gifts",
    "birthday",
    "wedding",
    "anniversary",
    "christmas",
    "valentine",
    "valentines",
    "mother's",
    "mothers",
    "father's",
    "fathers",
    "graduation",
    "baby",
    "shower",
    "halloween",
    "thanksgiving",
    "easter",
    "housewarming",
    "retirement",
];

fn is_occasion_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    OCCASION_WORDS
        .iter()
        .any(|w| lower.split_whitespace().any(|t| t == *w))
}

/// Classify one query. `known_product_ids` are real catalog ASINs, used
/// only to detect the (rare, but real) case of a shopper pasting a literal
/// product identifier as their query.
pub fn classify(
    query: &str,
    compiled: &CommerceQuery,
    known_product_ids: &HashSet<&str>,
) -> QueryClass {
    if known_product_ids.contains(query.trim()) {
        return QueryClass::ExactIdLookup;
    }
    if !compiled.ambiguous.is_empty() {
        return QueryClass::Ambiguous;
    }
    let has_structural = !compiled.constraints.is_empty();
    let has_residual = !compiled.residual_lexical.is_empty();
    match (has_structural, has_residual) {
        (true, false) => QueryClass::StructuralOnly,
        (true, true) => QueryClass::StructuralPlusLexical,
        (false, _) if is_occasion_query(query) => QueryClass::SemanticOccasion,
        (false, true) if compiled.residual_lexical.len() >= 2 => QueryClass::LexicalDominant,
        (false, _) => QueryClass::UnresolvedPunt,
    }
}

#[derive(Debug, Default, Clone)]
pub struct ClassCounts {
    pub counts: HashMap<QueryClass, usize>,
    pub total: usize,
}

impl ClassCounts {
    pub fn record(&mut self, class: QueryClass) {
        *self.counts.entry(class).or_insert(0) += 1;
        self.total += 1;
    }

    pub fn fraction(&self, class: QueryClass) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        *self.counts.get(&class).unwrap_or(&0) as f64 / self.total as f64
    }

    pub fn print(&self) {
        println!("=== Query classification (evidence class: real, independence: yes) ===");
        println!("total queries: {}", self.total);
        let mut classes: Vec<QueryClass> = self.counts.keys().copied().collect();
        classes.sort();
        for class in classes {
            let n = self.counts[&class];
            println!(
                "  {:<26} {:>7}  ({:.1}%)",
                class.label(),
                n,
                n as f64 / self.total as f64 * 100.0
            );
        }
        let deterministic = self.fraction(QueryClass::StructuralOnly)
            + self.fraction(QueryClass::StructuralPlusLexical);
        println!();
        println!(
            "Semantic FIB hit rate (structural_only + structural_plus_lexical + exact_id_lookup): {:.1}%",
            (deterministic + self.fraction(QueryClass::ExactIdLookup)) * 100.0
        );
        println!(
            "Punt rate (unresolved_punt): {:.1}%",
            self.fraction(QueryClass::UnresolvedPunt) * 100.0
        );
        println!(
            "Ambiguity rate: {:.1}%",
            self.fraction(QueryClass::Ambiguous) * 100.0
        );
        println!();
    }
}

/// Does a real product (looked up in the ingested catalog) satisfy a
/// compiled query's *hard* constraints under the **existing compiler's**
/// aggregation rule — every extracted constraint must hold simultaneously
/// (AND across everything, including multiple values extracted for the
/// same single-valued attribute). Only `Brand` (Structural) and `color`
/// (Attribute::Enum) are meaningful on this dataset — see `catalog.rs` for
/// why product-type/category/price are sentinels and therefore never
/// appear in a compiled query's constraints at all (no lexicon entries
/// exist for them).
pub fn product_satisfies_and(
    catalog: &Catalog,
    product_id: ProductId,
    constraints: &[ResolvedConstraint],
) -> bool {
    let Some(product) = catalog.products.get(product_id.0 as usize) else {
        return false;
    };
    constraints.iter().all(|c| match c {
        ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => product.brand == *id,
        ResolvedConstraint::Attribute(Constraint::Enum { attribute, value })
            if attribute == "color" =>
        {
            let variant = &product.variants[0];
            matches!(variant.attributes.get("color"), Some(AttributeValue::Enum(v)) if v == value)
        }
        // Any other constraint kind cannot appear from this real-data
        // lexicon (only Brand/color entries exist); if one somehow did,
        // fail closed rather than silently pass.
        _ => false,
    })
}

/// R1-E02's proposed remediation, tested as a hypothesis rather than
/// assumed to help: group extracted constraints by attribute (all `Brand`
/// values together, all `color` values together), require at least one
/// value per group to match (OR *within* a single-valued attribute) but
/// still require every group that was extracted at all to have a match
/// (AND *across* attributes). This is the natural fix for the
/// self-contradictory-conjunction failure mode found in R1-E02 (a single
/// `color` attribute can't equal two different values at once, but the
/// *query* may have genuinely intended either).
fn product_satisfies_or_within_attribute(
    catalog: &Catalog,
    product_id: ProductId,
    constraints: &[ResolvedConstraint],
) -> bool {
    let Some(product) = catalog.products.get(product_id.0 as usize) else {
        return false;
    };
    let mut brand_candidates = Vec::new();
    let mut color_candidates: Vec<&str> = Vec::new();
    for c in constraints {
        match c {
            ResolvedConstraint::Structural(StructuralConstraint::Brand(id)) => {
                brand_candidates.push(*id)
            }
            ResolvedConstraint::Attribute(Constraint::Enum { attribute, value })
                if attribute == "color" =>
            {
                color_candidates.push(value.as_str());
            }
            _ => return false,
        }
    }
    let brand_ok = brand_candidates.is_empty() || brand_candidates.contains(&product.brand);
    let color_ok = color_candidates.is_empty()
        || {
            let variant = &product.variants[0];
            matches!(variant.attributes.get("color"), Some(AttributeValue::Enum(v)) if color_candidates.contains(&v.as_str()))
        };
    brand_ok && color_ok
}

#[derive(Debug, Default, Clone)]
pub struct PrecisionReport {
    pub queries_measured: usize,
    pub filtered_products_total: usize,
    pub filtered_relevant: usize,
    pub judged_relevant_total: usize,
    pub judged_relevant_surviving_filter: usize,
    /// Same recall question restricted to `Exact`-labeled judgments only
    /// (excluding `Substitute`): a hard brand+color AND-filter is
    /// *supposed* to exclude substitutes — ESCI's own "Substitute" label
    /// means "different brand/attribute, same underlying need," which a
    /// strict structural filter will correctly reject. Recall against
    /// Exact-only is the fairer test of whether the filter is too
    /// aggressive, versus recall against Exact+Substitute combined, which
    /// conflates "filter is too strict" with "filter is doing exactly
    /// what a hard filter is supposed to do."
    pub judged_exact_total: usize,
    pub judged_exact_surviving_filter: usize,
}

impl PrecisionReport {
    /// "When the compiler claims structural resolution, how often is the
    /// interpretation actually correct?" — of the judged products that
    /// pass the compiled structural filter, what fraction are labeled
    /// Exact/Substitute (relevant) rather than Complement/Irrelevant.
    pub fn precision(&self) -> f64 {
        if self.filtered_products_total == 0 {
            return 0.0;
        }
        self.filtered_relevant as f64 / self.filtered_products_total as f64
    }

    /// Of the products a human judged relevant (Exact or Substitute),
    /// what fraction would *survive* the compiled structural filter?
    pub fn filter_recall(&self) -> f64 {
        if self.judged_relevant_total == 0 {
            return 0.0;
        }
        self.judged_relevant_surviving_filter as f64 / self.judged_relevant_total as f64
    }

    /// The fairer recall question: of products judged an Exact match
    /// specifically, what fraction survive the filter? See the field doc
    /// comment on `judged_exact_total`.
    pub fn exact_recall(&self) -> f64 {
        if self.judged_exact_total == 0 {
            return 0.0;
        }
        self.judged_exact_surviving_filter as f64 / self.judged_exact_total as f64
    }

    pub fn print(&self, label: &str) {
        println!("=== Semantic precision: {label} (evidence class: real) ===");
        println!("queries measured: {}", self.queries_measured);
        println!(
            "precision (of judged products passing the structural filter, fraction relevant E+S): {:.1}% ({}/{})",
            self.precision() * 100.0,
            self.filtered_relevant,
            self.filtered_products_total
        );
        println!(
            "filter recall vs Exact+Substitute (\"relevant\"): {:.1}% ({}/{})",
            self.filter_recall() * 100.0,
            self.judged_relevant_surviving_filter,
            self.judged_relevant_total
        );
        println!(
            "filter recall vs Exact only (fairer test of over-aggressive filtering): {:.1}% ({}/{})",
            self.exact_recall() * 100.0,
            self.judged_exact_surviving_filter,
            self.judged_exact_total
        );
        println!();
    }
}

/// Which aggregation rule to test — see `product_satisfies_and` (the
/// existing compiler's actual behavior) vs.
/// `product_satisfies_or_within_attribute` (R1-E02's proposed fix,
/// evaluated as a hypothesis).
#[derive(Debug, Clone, Copy)]
pub enum AggregationRule {
    ExistingAnd,
    OrWithinAttribute,
}

/// For every query classified as `StructuralOnly` or
/// `StructuralPlusLexical`, cross-reference its compiled constraints
/// against every judged (product, label) pair for that query in the real
/// ESCI test set.
pub fn measure_precision(
    catalog: &Catalog,
    asin_to_product_id: &HashMap<String, ProductId>,
    judgments_by_query: &HashMap<u64, Vec<&JudgedExample>>,
    compiled_by_query: &HashMap<u64, (QueryClass, CommerceQuery)>,
    rule: AggregationRule,
) -> PrecisionReport {
    let satisfies = match rule {
        AggregationRule::ExistingAnd => product_satisfies_and,
        AggregationRule::OrWithinAttribute => product_satisfies_or_within_attribute,
    };
    let mut report = PrecisionReport::default();
    for (query_id, (class, compiled)) in compiled_by_query {
        if !matches!(
            class,
            QueryClass::StructuralOnly | QueryClass::StructuralPlusLexical
        ) {
            continue;
        }
        let Some(judgments) = judgments_by_query.get(query_id) else {
            continue;
        };
        report.queries_measured += 1;
        for j in judgments {
            let Some(&product_id) = asin_to_product_id.get(&j.product_id) else {
                continue;
            };
            let relevant = j.label.is_relevant();
            let exact = j.label == crate::data::EsciLabel::Exact;
            if relevant {
                report.judged_relevant_total += 1;
            }
            if exact {
                report.judged_exact_total += 1;
            }
            if satisfies(catalog, product_id, &compiled.constraints) {
                report.filtered_products_total += 1;
                if relevant {
                    report.filtered_relevant += 1;
                    report.judged_relevant_surviving_filter += 1;
                }
                if exact {
                    report.judged_exact_surviving_filter += 1;
                }
            }
        }
    }
    report
}
