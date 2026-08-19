//! Issue #6 P1-C: predictive semantic prefill. `ir::query::compile`'s
//! lexicon resolution is exact-substring-only -- a query for "air force 1
//! white" can never resolve `Brand=Nike` unless "nike" literally appears in
//! the query text. This module tests whether catalog-derived knowledge
//! (which real products carry which phrase in their title, and which
//! brand those products overwhelmingly share) can *infer* that latent
//! structure instead, without an LLM/model call (CLAUDE.md's hot-path
//! rule) and without assuming a prediction must become a hard filter
//! (Issue #6's own instruction).
//!
//! `commerce_core` defines the mechanism ([`TitlePhraseIndex`]), not an
//! implementation, exactly like `plan::LexicalDelegate`/
//! `control_plane::provider::ModelProvider`: no full-text search engine is
//! a dependency of this crate. The real implementation
//! (`phase2_eval::bin::prefill_eval`) reuses the same Tantivy title index
//! already built and validated for lexical delegation (P2-E01/P2-E05).

use std::collections::HashMap;

use crate::domain::{BrandId, Catalog, ProductId};
use crate::index::CatalogIndex;
use crate::ir::{CommerceQuery, Preference, ResolvedConstraint, StructuralConstraint};

/// What `apply_predictive_prefill` needs of a full-text engine: which real
/// products carry `phrase` (adjacent-word match) in their title, up to
/// `limit`. `limit` bounds the cost of the purity estimate below to a
/// sample rather than a full scan -- this module never claims an exact
/// population count, only a confidence estimate from a bounded sample.
pub trait TitlePhraseIndex {
    fn products_containing_phrase(&self, phrase: &str, limit: usize) -> Vec<ProductId>;
}

/// One catalog-derived prediction: "if this phrase were treated as
/// implying a brand, `brand` is the dominant one among products whose
/// title actually contains it." `purity` is `dominant_count / occurrence`
/// (fraction of the *sampled* matching products carrying `brand`, not the
/// true population fraction -- stated, not overclaimed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrefillPrediction {
    pub brand: BrandId,
    pub purity: f64,
    pub occurrence: usize,
}

/// Estimate a phrase's brand-predictiveness from a bounded sample of real
/// products whose title contains it. Returns `None` if the phrase matches
/// no product at all (nothing to predict from).
pub fn predict_brand_from_phrase(
    phrase: &str,
    catalog: &Catalog,
    index: &CatalogIndex,
    phrase_index: &dyn TitlePhraseIndex,
    sample_limit: usize,
) -> Option<PrefillPrediction> {
    let matches = phrase_index.products_containing_phrase(phrase, sample_limit);
    if matches.is_empty() {
        return None;
    }
    let mut counts: HashMap<BrandId, usize> = HashMap::new();
    for product_id in &matches {
        if let Some(product) = index.lookup_product(catalog, *product_id) {
            *counts.entry(product.brand).or_insert(0) += 1;
        }
    }
    let total = matches.len();
    let (dominant_brand, dominant_count) = counts.into_iter().max_by_key(|&(_, c)| c)?;
    Some(PrefillPrediction {
        brand: dominant_brand,
        purity: dominant_count as f64 / total as f64,
        occurrence: total,
    })
}

/// Confidence thresholds for turning a [`PrefillPrediction`] into query
/// structure. No [`Default`] impl, matching `plan::PlannerPolicy`'s own
/// discipline: no evidence-backed recommended value exists yet (this
/// module's own evaluation harness is what measures one) -- callers must
/// construct one explicitly.
#[derive(Debug, Clone, PartialEq)]
pub struct PrefillPolicy {
    /// Sizes of word windows checked against `phrase_index`, e.g. `[2, 3]`
    /// for two- and three-word phrases. Single-word phrases are
    /// deliberately excluded by convention (callers should pass sizes of
    /// two or more): a single already-recognized word is what the
    /// existing lexicon already handles, and single generic words are
    /// exactly the false-positive-brand failure mode P2-E11 found -- this
    /// module targets *multi-word* latent structure specifically.
    pub ngram_sizes: Vec<usize>,
    /// How many matching products `TitlePhraseIndex` samples per phrase
    /// for the purity estimate.
    pub sample_limit: usize,
    /// A prediction at or above this purity and occurrence becomes a hard
    /// `StructuralConstraint::Brand` -- Issue #6: "hard Constraint only at
    /// very high confidence."
    pub high_confidence_min_purity: f64,
    pub high_confidence_min_occurrence: usize,
    /// A prediction below the high-confidence bar but at or above this one
    /// becomes a ranking-only `Preference::StructuralBoost` instead --
    /// Issue #6: "scored Preference for softer semantic matches." Below
    /// this bar, no signal is added at all (today's behavior).
    pub medium_confidence_min_purity: f64,
    pub medium_confidence_min_occurrence: usize,
    /// Weight given to a medium-confidence `Preference::StructuralBoost`.
    pub preference_weight: f64,
}

/// Scan `raw_text` for word windows (sized per `policy.ngram_sizes`) that
/// [`predict_brand_from_phrase`] resolves with enough confidence, and add
/// the resulting signal to `query` -- additively, per ADR 0010 (a
/// `Preference` never removes anything from `residual_lexical`; a
/// predicted hard `Constraint` is likewise only ever *added*, never
/// replacing or removing an existing token). Two safety rules, both
/// non-negotiable per CLAUDE.md's "product/variant correctness is
/// non-negotiable":
///
/// 1. If `query` already carries an explicit hard `Brand`/`BrandAny`
///    constraint (the shopper typed a real, recognized brand name), no
///    prediction is added at all, regardless of confidence -- an inferred
///    signal must never contradict or duplicate an explicit one.
/// 2. A phrase identical to the predicted brand's own canonical name is
///    skipped -- that is what the existing lexicon already resolves
///    (or would, if it were a recognized brand), not a genuinely inferred
///    association; only report `ambiguous`-shaped new information.
#[allow(clippy::too_many_arguments)]
pub fn apply_predictive_prefill(
    query: &mut CommerceQuery,
    raw_text: &str,
    catalog: &Catalog,
    index: &CatalogIndex,
    phrase_index: &dyn TitlePhraseIndex,
    brand_name_by_id: &HashMap<BrandId, String>,
    policy: &PrefillPolicy,
) {
    let already_has_explicit_brand = query.constraints.iter().any(|c| {
        matches!(
            c,
            ResolvedConstraint::Structural(
                StructuralConstraint::Brand(_) | StructuralConstraint::BrandAny(_)
            )
        )
    });
    if already_has_explicit_brand {
        return;
    }

    let tokens: Vec<String> = raw_text.split_whitespace().map(str::to_lowercase).collect();
    let mut best: Option<(BrandId, f64, usize)> = None; // (brand, purity, occurrence)

    for &n in &policy.ngram_sizes {
        if n == 0 || tokens.len() < n {
            continue;
        }
        for window in tokens.windows(n) {
            let phrase = window.join(" ");
            let Some(prediction) = predict_brand_from_phrase(
                &phrase,
                catalog,
                index,
                phrase_index,
                policy.sample_limit,
            ) else {
                continue;
            };
            if brand_name_by_id
                .get(&prediction.brand)
                .is_some_and(|name| name.eq_ignore_ascii_case(&phrase))
            {
                continue; // not genuinely inferred -- the phrase already *is* the brand name
            }
            let qualifies_high = prediction.purity >= policy.high_confidence_min_purity
                && prediction.occurrence >= policy.high_confidence_min_occurrence;
            let qualifies_medium = prediction.purity >= policy.medium_confidence_min_purity
                && prediction.occurrence >= policy.medium_confidence_min_occurrence;
            if !qualifies_high && !qualifies_medium {
                continue;
            }
            let better = best.is_none_or(|(_, bp, bo)| {
                prediction.purity > bp || (prediction.purity == bp && prediction.occurrence > bo)
            });
            if better {
                best = Some((prediction.brand, prediction.purity, prediction.occurrence));
            }
        }
    }

    let Some((brand, purity, occurrence)) = best else {
        return;
    };
    if purity >= policy.high_confidence_min_purity
        && occurrence >= policy.high_confidence_min_occurrence
    {
        query
            .constraints
            .push(ResolvedConstraint::Structural(StructuralConstraint::Brand(
                brand,
            )));
    } else {
        query.preferences.push(Preference::StructuralBoost(
            StructuralConstraint::Brand(brand),
            policy.preference_weight,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        attributes, BrandId, CategoryId, Inventory, Price, Product, ProductTypeId, Variant,
        VariantId,
    };

    struct FakePhraseIndex {
        matches: HashMap<String, Vec<ProductId>>,
    }

    impl TitlePhraseIndex for FakePhraseIndex {
        fn products_containing_phrase(&self, phrase: &str, limit: usize) -> Vec<ProductId> {
            self.matches
                .get(phrase)
                .map(|ids| ids.iter().copied().take(limit).collect())
                .unwrap_or_default()
        }
    }

    fn product(id: u64, brand: BrandId) -> Product {
        Product {
            id: ProductId(id),
            product_type: ProductTypeId(1),
            brand,
            category: CategoryId(1),
            title: format!("product {id}"),
            attributes: attributes([]),
            variants: vec![Variant {
                id: VariantId(id + 1000),
                attributes: attributes([]),
                price: Price::usd(1000),
                inventory: Inventory::in_stock(1),
            }],
        }
    }

    fn nike_id() -> BrandId {
        BrandId(1)
    }

    fn default_policy() -> PrefillPolicy {
        PrefillPolicy {
            ngram_sizes: vec![2, 3],
            sample_limit: 50,
            high_confidence_min_purity: 0.9,
            high_confidence_min_occurrence: 10,
            medium_confidence_min_purity: 0.6,
            medium_confidence_min_occurrence: 3,
            preference_weight: 0.5,
        }
    }

    #[test]
    fn high_purity_prediction_becomes_a_hard_constraint() {
        let catalog = Catalog {
            products: (1..=10).map(|i| product(i, nike_id())).collect(),
        };
        let index = CatalogIndex::build(&catalog);
        let phrase_index = FakePhraseIndex {
            matches: HashMap::from([("air force".to_string(), (1..=10).map(ProductId).collect())]),
        };
        let brand_names = HashMap::from([(nike_id(), "nike".to_string())]);
        let mut query = CommerceQuery {
            residual_lexical: vec!["air".into(), "force".into(), "1".into(), "white".into()],
            ..Default::default()
        };

        apply_predictive_prefill(
            &mut query,
            "air force 1 white",
            &catalog,
            &index,
            &phrase_index,
            &brand_names,
            &default_policy(),
        );

        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                nike_id()
            ))]
        );
        assert!(query.preferences.is_empty());
        // Additive per ADR 0010: the literal text stays searchable too.
        assert!(query.residual_lexical.contains(&"air".to_string()));
    }

    #[test]
    fn medium_purity_prediction_becomes_a_soft_preference_not_a_constraint() {
        let mut products: Vec<Product> = (1..=7).map(|i| product(i, nike_id())).collect();
        products.extend((8..=10).map(|i| product(i, BrandId(2))));
        let catalog = Catalog { products };
        let index = CatalogIndex::build(&catalog);
        let phrase_index = FakePhraseIndex {
            matches: HashMap::from([("air force".to_string(), (1..=10).map(ProductId).collect())]),
        };
        let brand_names = HashMap::from([(nike_id(), "nike".to_string())]);
        let mut query = CommerceQuery {
            residual_lexical: vec!["air".into(), "force".into()],
            ..Default::default()
        };

        apply_predictive_prefill(
            &mut query,
            "air force sneaker",
            &catalog,
            &index,
            &phrase_index,
            &brand_names,
            &default_policy(),
        );

        assert!(query.constraints.is_empty());
        assert_eq!(
            query.preferences,
            vec![Preference::StructuralBoost(
                StructuralConstraint::Brand(nike_id()),
                0.5
            )]
        );
    }

    #[test]
    fn low_purity_prediction_adds_no_signal_at_all() {
        let mut products: Vec<Product> = (1..=5).map(|i| product(i, nike_id())).collect();
        products.extend((6..=10).map(|i| product(i, BrandId(2))));
        let catalog = Catalog { products };
        let index = CatalogIndex::build(&catalog);
        let phrase_index = FakePhraseIndex {
            matches: HashMap::from([("air force".to_string(), (1..=10).map(ProductId).collect())]),
        };
        let brand_names = HashMap::from([(nike_id(), "nike".to_string())]);
        let mut query = CommerceQuery::default();

        apply_predictive_prefill(
            &mut query,
            "air force sneaker",
            &catalog,
            &index,
            &phrase_index,
            &brand_names,
            &default_policy(),
        );

        assert!(query.constraints.is_empty());
        assert!(query.preferences.is_empty());
    }

    #[test]
    fn an_explicit_brand_constraint_already_present_suppresses_prediction_entirely() {
        let catalog = Catalog {
            products: (1..=10).map(|i| product(i, nike_id())).collect(),
        };
        let index = CatalogIndex::build(&catalog);
        let phrase_index = FakePhraseIndex {
            matches: HashMap::from([("air force".to_string(), (1..=10).map(ProductId).collect())]),
        };
        let brand_names = HashMap::from([(nike_id(), "nike".to_string())]);
        // The shopper explicitly typed a *different* recognized brand --
        // the prediction must never override or duplicate explicit signal.
        let mut query = CommerceQuery {
            constraints: vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(2),
            ))],
            residual_lexical: vec!["air".into(), "force".into()],
            ..Default::default()
        };

        apply_predictive_prefill(
            &mut query,
            "adidas air force style",
            &catalog,
            &index,
            &phrase_index,
            &brand_names,
            &default_policy(),
        );

        assert_eq!(
            query.constraints,
            vec![ResolvedConstraint::Structural(StructuralConstraint::Brand(
                BrandId(2)
            ))]
        );
        assert!(query.preferences.is_empty());
    }

    #[test]
    fn a_phrase_identical_to_the_predicted_brands_own_name_is_not_a_genuine_prediction() {
        let catalog = Catalog {
            products: (1..=10).map(|i| product(i, nike_id())).collect(),
        };
        let index = CatalogIndex::build(&catalog);
        // The phrase IS the brand's own canonical name -- this is what
        // the existing lexicon already resolves, not inferred structure.
        let phrase_index = FakePhraseIndex {
            matches: HashMap::from([("nike air".to_string(), (1..=10).map(ProductId).collect())]),
        };
        let brand_names = HashMap::from([(nike_id(), "nike air".to_string())]);
        let mut query = CommerceQuery {
            residual_lexical: vec!["nike".into(), "air".into()],
            ..Default::default()
        };

        apply_predictive_prefill(
            &mut query,
            "nike air running",
            &catalog,
            &index,
            &phrase_index,
            &brand_names,
            &default_policy(),
        );

        assert!(query.constraints.is_empty());
        assert!(query.preferences.is_empty());
    }

    #[test]
    fn no_match_in_the_phrase_index_predicts_nothing() {
        let catalog = Catalog { products: vec![] };
        let index = CatalogIndex::build(&catalog);
        let phrase_index = FakePhraseIndex {
            matches: HashMap::new(),
        };
        assert_eq!(
            predict_brand_from_phrase("air force", &catalog, &index, &phrase_index, 50),
            None
        );
    }
}
