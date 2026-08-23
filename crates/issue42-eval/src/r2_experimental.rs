//! R2 experimental treatments (`docs/experiments/ISSUE42_PROTOCOL.md`'s R2
//! section): residual lexical semantics. All four operate on
//! `plan::execute_planned`'s already-computed `(PlannedQuery,
//! residual_lexical)` pair, per the protocol -- none modify
//! `commerce_core::plan` or `commerce_core::ir::query::compile`; every
//! query passed in is the real, unmodified `compile()`'s own output.
//!
//! Treatment A is the real, unmodified `execute_planned`, reused verbatim.
//! Treatments B/C/D call the same real, public
//! `commerce_core::index::CatalogIndex`/`LexicalDelegate` primitives
//! `execute_planned` itself calls internally (`indexed_candidates`,
//! `candidate_product_ids`, `execute_ranked`, `delegate.search`) --
//! `commerce_core::plan::verify_and_truncate` is `pub(crate)` and not
//! reachable from here, so where a treatment's behavior is identical to
//! A's (raw delegate hits non-empty), it simply calls the real
//! `execute_planned` again rather than reimplementing verification.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use commerce_core::domain::{AttributeValue, Catalog, ProductId, ProductTypeId, VariantId};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};
use commerce_core::plan::{
    execute_planned, plan, ExecutionOutcome, LexicalDelegate, LexicalHit, PlannedHit, PlannedQuery,
    PlannerPolicy,
};

/// Every token/short-phrase this fixture's residual text uses classifies
/// as one of these two -- `Required` (a delegate miss keeps the query at
/// zero results) or `Preferred` (a delegate miss triggers the structural
/// fallback). The full preregistered design also names `Contextual`
/// (entity-dependent) and `Unknown` (too rare to classify, falls back to
/// advisory) classes; neither is exercised by R2's own preregistered
/// workload (no row needs entity-dependent unit-word handling, and every
/// test token here has either zero or a clearly-countable nonzero
/// occurrence), so this pass implements only the two classes it can
/// actually test and measure -- a disclosed scope decision, not a silent
/// omission (`docs/experiments/ISSUE42_LOG.md`'s R2 section).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidualClass {
    Required,
    Preferred,
}

/// How many *other* product types a token must be observed under (title
/// word, or a registered Enum/MultiEnum value, or a `true` Boolean
/// attribute name) before it reads as a broadly-used, generically
/// descriptive term rather than a specific attribute value belonging to
/// one particular family. Two is the smallest number that can actually
/// distinguish "seen under exactly one other family" (a real, specific,
/// probably-incompatible value -- R2's row 6) from "seen broadly" (a
/// generic word -- R2's rows 1/3/5/7), and is what this experiment's own
/// fixture is constructed to exercise.
const CROSS_TYPE_BREADTH_THRESHOLD: usize = 2;

/// Compiled at "ingestion time" (i.e. once, from the whole `Catalog`, not
/// per query) -- the protocol's own requirement that classification not
/// involve a query-time model call. Deliberately does not touch
/// `commerce_core::cold_start::CatalogProfile`/`compile_lexicon`
/// themselves; this is new, additive analysis over the same `Catalog`
/// those already scan, kept separate until/unless Treatment D wins its
/// gate.
pub struct ResidualPolicy {
    /// lowercased token -> every `ProductTypeId` it was observed under,
    /// anywhere in the catalog.
    type_occurrences: BTreeMap<String, BTreeSet<ProductTypeId>>,
}

fn record_attribute_occurrence(
    map: &mut BTreeMap<String, BTreeSet<ProductTypeId>>,
    name: &str,
    value: &AttributeValue,
    product_type: ProductTypeId,
) {
    match value {
        AttributeValue::Enum(v) => {
            map.entry(v.to_lowercase())
                .or_default()
                .insert(product_type);
        }
        AttributeValue::MultiEnum(vs) => {
            for v in vs {
                map.entry(v.to_lowercase())
                    .or_default()
                    .insert(product_type);
            }
        }
        AttributeValue::Boolean(true) => {
            map.entry(name.to_lowercase())
                .or_default()
                .insert(product_type);
        }
        AttributeValue::Boolean(false) | AttributeValue::Numeric(_) | AttributeValue::Text(_) => {}
    }
}

impl ResidualPolicy {
    pub fn compile(catalog: &Catalog) -> Self {
        let mut type_occurrences: BTreeMap<String, BTreeSet<ProductTypeId>> = BTreeMap::new();
        for product in &catalog.products {
            for word in product.title.split_whitespace() {
                let cleaned: String = word
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(|c| c.to_lowercase())
                    .collect();
                if !cleaned.is_empty() {
                    type_occurrences
                        .entry(cleaned)
                        .or_default()
                        .insert(product.product_type);
                }
            }
            for (name, value) in &product.attributes {
                record_attribute_occurrence(
                    &mut type_occurrences,
                    name,
                    value,
                    product.product_type,
                );
            }
            for variant in &product.variants {
                for (name, value) in &variant.attributes {
                    record_attribute_occurrence(
                        &mut type_occurrences,
                        name,
                        value,
                        product.product_type,
                    );
                }
            }
        }
        ResidualPolicy { type_occurrences }
    }

    /// `None` when `token` was never observed anywhere in the catalog --
    /// the safest possible default (zero positive evidence) is to treat it
    /// as `Required`, not to guess it is harmless.
    pub fn classify(&self, token: &str, product_type: ProductTypeId) -> ResidualClass {
        match self.type_occurrences.get(&token.to_lowercase()) {
            None => ResidualClass::Required,
            Some(types) => {
                // A token classifies as Preferred either because it was
                // observed for this query's own product type (an
                // ordinary, on-type descriptive word), or because it was
                // observed broadly across other types (a generic
                // marketing word) -- both read as "not a specific,
                // probably-incompatible attribute value" the same way.
                if types.contains(&product_type) || types.len() >= CROSS_TYPE_BREADTH_THRESHOLD {
                    ResidualClass::Preferred
                } else {
                    ResidualClass::Required
                }
            }
        }
    }
}

fn corroborating_product_type(query: &CommerceQuery) -> Option<ProductTypeId> {
    query.constraints.iter().find_map(|c| match c {
        ResolvedConstraint::Structural(StructuralConstraint::ProductType(id)) => Some(*id),
        _ => None,
    })
}

fn to_planned_hit(h: commerce_core::index::RankedHit) -> PlannedHit {
    PlannedHit {
        product: h.product,
        variant: h.variant,
        score: h.score,
    }
}

/// Replicates -- does not reuse, since it is `pub(crate)` -- the exact raw
/// delegate call `execute_planned`'s own `Hybrid`/`Punt` branches make, so
/// a treatment can inspect whether the delegate found *anything* before
/// deciding how to react. Never called for `FastPath` (the delegate is
/// never consulted there at all).
fn raw_delegate_hits(
    query: &CommerceQuery,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
    outcome: ExecutionOutcome,
) -> Vec<LexicalHit> {
    match outcome {
        ExecutionOutcome::Hybrid => {
            let candidate_ords = index.indexed_candidates(&query.constraints);
            let restrict_to = index.candidate_product_ids(&candidate_ords);
            let oversampled_limit = k * policy.delegate_oversample;
            delegate
                .map(|d| {
                    d.search(
                        &query.residual_lexical,
                        Some(&restrict_to),
                        oversampled_limit,
                    )
                })
                .unwrap_or_default()
        }
        ExecutionOutcome::Punt => {
            let limit = if query.constraints.is_empty() {
                k
            } else {
                k * policy.delegate_oversample
            };
            delegate
                .map(|d| d.search(&query.residual_lexical, None, limit))
                .unwrap_or_default()
        }
        ExecutionOutcome::FastPath => Vec::new(),
    }
}

fn structural_only_hits(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    k: usize,
) -> Vec<PlannedHit> {
    index
        .execute_ranked(query, catalog, k)
        .into_iter()
        .map(to_planned_hit)
        .collect()
}

/// Treatment A: the real, unmodified `execute_planned`, reused verbatim.
pub fn execute_a(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
) -> (PlannedQuery, Vec<PlannedHit>) {
    execute_planned(query, catalog, index, delegate, k, policy)
}

/// Treatment B: if the delegate returns zero raw hits for a `Hybrid`/
/// `Punt` outcome with a non-empty residual, fall back to the structural
/// candidate set alone (ranked by `execute_ranked`'s own default signal,
/// same as `FastPath`). Unconditional -- never checks whether the
/// residual term was actually disqualifying. `FastPath` outcomes and
/// queries with no structural constraint at all (row 8's regression
/// guard) are untouched, identical to A.
pub fn execute_b(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
) -> (PlannedQuery, Vec<PlannedHit>) {
    let planned = plan(query, index, catalog.products.len(), policy);
    if matches!(planned.outcome, ExecutionOutcome::FastPath) || query.constraints.is_empty() {
        return execute_planned(query, catalog, index, delegate, k, policy);
    }
    let raw = raw_delegate_hits(query, index, delegate, k, policy, planned.outcome);
    if raw.is_empty() {
        (planned, structural_only_hits(query, catalog, index, k))
    } else {
        execute_planned(query, catalog, index, delegate, k, policy)
    }
}

/// Treatment C: residual lexical text is *always* advisory -- delegate
/// results, if any, are used only to re-order the structural candidate
/// set (structural hits whose product the delegate also ranked come
/// first, in the delegate's own order; every other structural hit keeps
/// its own relative order after), never to filter it. If the delegate
/// returns nothing, behaves exactly like B for that query.
pub fn execute_c(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
) -> (PlannedQuery, Vec<PlannedHit>) {
    let planned = plan(query, index, catalog.products.len(), policy);
    if matches!(planned.outcome, ExecutionOutcome::FastPath) || query.constraints.is_empty() {
        return execute_planned(query, catalog, index, delegate, k, policy);
    }
    let raw = raw_delegate_hits(query, index, delegate, k, policy, planned.outcome);
    let structural_limit = (k * policy.delegate_oversample).max(k);
    let structural_hits = index.execute_ranked(query, catalog, structural_limit);
    if raw.is_empty() {
        let hits = structural_hits
            .into_iter()
            .take(k)
            .map(to_planned_hit)
            .collect();
        return (planned, hits);
    }
    let mut ordered = Vec::new();
    let mut seen: BTreeSet<(ProductId, VariantId)> = BTreeSet::new();
    for lexical_hit in &raw {
        if let Some(h) = structural_hits
            .iter()
            .find(|h| h.product == lexical_hit.product && !seen.contains(&(h.product, h.variant)))
        {
            seen.insert((h.product, h.variant));
            ordered.push(to_planned_hit(*h));
        }
    }
    for h in &structural_hits {
        if !seen.contains(&(h.product, h.variant)) {
            seen.insert((h.product, h.variant));
            ordered.push(to_planned_hit(*h));
        }
    }
    ordered.truncate(k);
    (planned, ordered)
}

/// Treatment D: at ingestion time, classify every residual token via
/// [`ResidualPolicy`]. At query time, a `Required`-classified residual
/// with zero delegate hits keeps the query at zero results (A's own
/// behavior); a `Preferred` one triggers B's structural fallback. With no
/// corroborating `ProductType` entity to classify against, falls back to
/// A's own (already correct) behavior rather than guessing.
pub fn execute_d(
    query: &CommerceQuery,
    catalog: &Catalog,
    index: &CatalogIndex,
    delegate: Option<&dyn LexicalDelegate>,
    k: usize,
    policy: &PlannerPolicy,
    residual_policy: &ResidualPolicy,
) -> (PlannedQuery, Vec<PlannedHit>) {
    let planned = plan(query, index, catalog.products.len(), policy);
    if matches!(planned.outcome, ExecutionOutcome::FastPath) || query.constraints.is_empty() {
        return execute_planned(query, catalog, index, delegate, k, policy);
    }
    let raw = raw_delegate_hits(query, index, delegate, k, policy, planned.outcome);
    if !raw.is_empty() {
        return execute_planned(query, catalog, index, delegate, k, policy);
    }
    let Some(product_type) = corroborating_product_type(query) else {
        return execute_planned(query, catalog, index, delegate, k, policy);
    };
    let any_required = query
        .residual_lexical
        .iter()
        .any(|tok| residual_policy.classify(tok, product_type) == ResidualClass::Required);
    if any_required {
        execute_planned(query, catalog, index, delegate, k, policy)
    } else {
        (planned, structural_only_hits(query, catalog, index, k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
    use commerce_core::ir::compile;
    use phase9_eval::bitmap_delegate::{build_index, BitmapTantivyDelegate};

    fn setup() -> (
        crate::r2_workload::ResidualWorkloadCatalog,
        commerce_core::ir::SemanticLexicon,
        CatalogIndex,
    ) {
        let fixture = crate::r2_workload::build();
        let profile = CatalogProfile::build(
            &fixture.catalog,
            &fixture.brands,
            &fixture.product_types,
            &fixture.categories,
        );
        let lexicon = compile_lexicon(&profile, 1);
        let index = CatalogIndex::build(&fixture.catalog);
        (fixture, lexicon, index)
    }

    #[test]
    fn residual_policy_classifies_benign_words_as_preferred_for_both_entities() {
        let (fixture, _lexicon, _index) = setup();
        let policy = ResidualPolicy::compile(&fixture.catalog);
        let sofas = ProductTypeId(1);
        let boots = ProductTypeId(2);
        for word in ["furniture", "waterproof", "bestseller", "clearance"] {
            assert_eq!(
                policy.classify(word, sofas),
                ResidualClass::Preferred,
                "{word} should be Preferred for Sofas"
            );
            assert_eq!(
                policy.classify(word, boots),
                ResidualClass::Preferred,
                "{word} should be Preferred for Boots"
            );
        }
    }

    #[test]
    fn residual_policy_classifies_velvet_as_required_for_boots_but_preferred_for_sofas() {
        let (fixture, _lexicon, _index) = setup();
        let policy = ResidualPolicy::compile(&fixture.catalog);
        assert_eq!(
            policy.classify("velvet", ProductTypeId(2)),
            ResidualClass::Required
        );
        assert_eq!(
            policy.classify("velvet", ProductTypeId(1)),
            ResidualClass::Preferred
        );
    }

    #[test]
    fn residual_policy_classifies_never_observed_token_as_required() {
        let (fixture, _lexicon, _index) = setup();
        let policy = ResidualPolicy::compile(&fixture.catalog);
        assert_eq!(
            policy.classify("banana", ProductTypeId(1)),
            ResidualClass::Required
        );
        assert_eq!(
            policy.classify("banana", ProductTypeId(2)),
            ResidualClass::Required
        );
    }

    #[test]
    fn treatment_a_is_exactly_the_real_execute_planned_output() {
        let (fixture, lexicon, index) = setup();
        let built = build_index(&fixture.catalog).unwrap();
        let delegate = BitmapTantivyDelegate::new(
            &built.index,
            vec![built.title_field, built.description_field],
        )
        .unwrap();
        let policy = PlannerPolicy {
            selectivity_threshold: 0.05,
            delegate_oversample: 20,
        };
        let query = compile("furniture sofas", &lexicon);
        let direct = execute_planned(
            &query,
            &fixture.catalog,
            &index,
            Some(&delegate),
            10,
            &policy,
        );
        let via_a = execute_a(
            &query,
            &fixture.catalog,
            &index,
            Some(&delegate),
            10,
            &policy,
        );
        assert_eq!(direct.0, via_a.0);
        assert_eq!(direct.1, via_a.1);
    }
}
