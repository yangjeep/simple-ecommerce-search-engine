use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    AttributeMap, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId, Constraint,
    ProductType, ProductTypeId,
};
use crate::ir::{Candidate, ResolvedConstraint, SemanticLexicon, StructuralConstraint};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EnumSource {
    attribute: String,
    value: String,
    is_multi: bool,
}

/// Everything Gate 6's cold-start profiler can extract from a catalog
/// snapshot without a model call: one pass over every product/variant,
/// deduplicated by value (CLAUDE.md: "do not perform one LLM call per
/// SKU" — this profiler makes zero calls at all, structural counting only).
/// Values are indexed by their lowercase form so [`compile_lexicon`] can
/// key lexicon entries the same way `ir::query::compile` looks them up,
/// but each source retains the *original* casing to build a correct
/// `Constraint`. Two different attributes sharing the same lowercase
/// value (e.g. "green" as both a color and a feature tag) fall out
/// naturally as multiple `EnumSource`s under one key — a genuine
/// ambiguity the profiler cannot and must not silently resolve.
#[derive(Debug, Default)]
pub struct CatalogProfile {
    brand_names: BTreeMap<String, BrandId>,
    product_type_names: BTreeMap<String, ProductTypeId>,
    category_names: BTreeMap<String, CategoryId>,
    boolean_attributes: BTreeSet<String>,
    enum_candidates: BTreeMap<String, BTreeSet<EnumSource>>,
    numeric_values: BTreeMap<String, Vec<f64>>,
    price_cents: Vec<i64>,
}

impl CatalogProfile {
    pub fn build(
        catalog: &Catalog,
        brands: &[Brand],
        product_types: &[ProductType],
        categories: &[Category],
    ) -> Self {
        let mut profile = CatalogProfile::default();
        for b in brands {
            profile.brand_names.insert(b.name.to_lowercase(), b.id);
        }
        for pt in product_types {
            profile
                .product_type_names
                .insert(pt.name.to_lowercase(), pt.id);
        }
        for c in categories {
            profile.category_names.insert(c.name.to_lowercase(), c.id);
        }

        for product in &catalog.products {
            profile.index_attributes(&product.attributes);
            for variant in &product.variants {
                profile.index_attributes(&variant.attributes);
                profile.price_cents.push(variant.price.amount_cents);
            }
        }

        profile.price_cents.sort_unstable();
        profile.price_cents.dedup();
        for values in profile.numeric_values.values_mut() {
            values.sort_by(f64::total_cmp);
            values.dedup();
        }
        profile
    }

    fn index_attributes(&mut self, attrs: &AttributeMap) {
        for (name, value) in attrs {
            match value {
                AttributeValue::Enum(v) => self.index_enum_source(name, v, false),
                AttributeValue::MultiEnum(vs) => {
                    for v in vs {
                        self.index_enum_source(name, v, true);
                    }
                }
                AttributeValue::Boolean(true) => {
                    self.boolean_attributes.insert(name.clone());
                }
                AttributeValue::Boolean(false) => {}
                AttributeValue::Numeric(n) => {
                    self.numeric_values
                        .entry(name.clone())
                        .or_default()
                        .push(*n);
                }
                // Free text has no discrete vocabulary to profile; Gate 3's
                // narrow-then-verify path already handles Text at query time.
                AttributeValue::Text(_) => {}
            }
        }
    }

    fn index_enum_source(&mut self, attribute: &str, value: &str, is_multi: bool) {
        self.enum_candidates
            .entry(value.to_lowercase())
            .or_default()
            .insert(EnumSource {
                attribute: attribute.to_string(),
                value: value.to_string(),
                is_multi,
            });
    }

    pub fn product_type_names(&self) -> impl Iterator<Item = &str> {
        self.product_type_names.keys().map(String::as_str)
    }

    pub fn brand_names(&self) -> impl Iterator<Item = &str> {
        self.brand_names.keys().map(String::as_str)
    }

    pub fn boolean_attributes(&self) -> impl Iterator<Item = &str> {
        self.boolean_attributes.iter().map(String::as_str)
    }

    pub fn enum_value_keys(&self) -> impl Iterator<Item = &str> {
        self.enum_candidates.keys().map(String::as_str)
    }

    pub fn numeric_values(&self, attribute: &str) -> &[f64] {
        self.numeric_values
            .get(attribute)
            .map_or(&[], Vec::as_slice)
    }

    pub fn max_price_cents(&self) -> Option<i64> {
        self.price_cents.last().copied()
    }

    /// How many distinct raw values were seen (brands + product types +
    /// categories + boolean attributes + distinct enum/multi-enum value
    /// strings) versus how many catalog attribute occurrences were
    /// compressed into them — the "profile/compress semantic problems"
    /// half of Gate 6, reported as a ratio rather than asserted on, since
    /// the interesting number depends entirely on the catalog's own
    /// diversity.
    pub fn distinct_value_count(&self) -> usize {
        self.brand_names.len()
            + self.product_type_names.len()
            + self.category_names.len()
            + self.boolean_attributes.len()
            + self.enum_candidates.len()
    }
}

/// Deterministically derive a [`SemanticLexicon`] from a [`CatalogProfile`]
/// — no model call, no randomness. Every derived attribute-value mapping
/// becomes a *hard* constraint candidate: unlike the hand-curated
/// `fixtures::shoe_lexicon`, the profiler has no signal to distinguish a
/// decisive attribute (color, size) from a descriptive/soft one
/// (cushioned, breathable), so it cannot propose `ir::Preference`s the way
/// a human curator did in Gate 2/4 — see `docs/adr/0006-cold-start-fuzzing.md`.
pub fn compile_lexicon(profile: &CatalogProfile) -> SemanticLexicon {
    let mut lex = SemanticLexicon::new();
    for (name, id) in &profile.brand_names {
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::Brand(*id)),
                1.0,
            )],
        );
    }
    for (name, id) in &profile.product_type_names {
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::ProductType(*id)),
                1.0,
            )],
        );
    }
    for (name, id) in &profile.category_names {
        lex.insert(
            name,
            vec![Candidate::constraint(
                ResolvedConstraint::Structural(StructuralConstraint::Category(*id)),
                1.0,
            )],
        );
    }
    for attribute in &profile.boolean_attributes {
        lex.insert(
            attribute,
            vec![Candidate::constraint(
                ResolvedConstraint::Attribute(Constraint::Boolean {
                    attribute: attribute.clone(),
                    value: true,
                }),
                1.0,
            )],
        );
    }
    for (value_lower, sources) in &profile.enum_candidates {
        let candidates: Vec<Candidate> = sources
            .iter()
            .map(|source| {
                let constraint = if source.is_multi {
                    Constraint::MultiEnumContains {
                        attribute: source.attribute.clone(),
                        value: source.value.clone(),
                    }
                } else {
                    Constraint::Enum {
                        attribute: source.attribute.clone(),
                        value: source.value.clone(),
                    }
                };
                Candidate::constraint(ResolvedConstraint::Attribute(constraint), 1.0)
            })
            .collect();
        lex.insert(value_lower, candidates);
    }
    lex
}
