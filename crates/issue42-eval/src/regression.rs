//! Regression-check layer (Issue #42 shared infrastructure,
//! `docs/experiments/ISSUE42_PROTOCOL.md`'s "Regression-check layer"
//! section).
//!
//! For every workload query R1/R2/R3 mark "positive" (a claimed
//! exact/relevant match), [`assert_positive`] checks -- against the
//! independent [`crate::oracle`], not against any generator's own
//! judgment map -- that the *actually materialized* catalog for this run
//! really contains at least one `Exact` match. For every "negative"/
//! adversarial query, [`assert_negative`] checks the forbidden
//! interpretation has zero `Exact` matches. Both are `#[test]`-level
//! regression checks meant to run in CI, generalizing the pattern
//! `issue38_e2e3_eval::ground_truth::assert_every_query_of_template_has_an_exact_match`
//! already established, to also cover negatives and to check against this
//! crate's own independent oracle rather than a generator's labels.

use commerce_core::domain::Catalog;

use crate::oracle::{self, QueryIntent};

/// Assert a "positive" query intent has at least one `Exact` oracle match
/// in the actually materialized `catalog` for this run. `description`
/// identifies the query in a failure message; it is not interpreted.
pub fn assert_positive(catalog: &Catalog, intent: &QueryIntent, description: &str) {
    let result = oracle::classify(catalog, intent);
    assert!(
        !result.exact.is_empty(),
        "positive query {description:?} expected at least one Exact oracle match in the \
         actually materialized catalog; found none ({} Partial matches instead) -- this is a \
         real generator/catalog defect (the claimed case never actually occurs this run), not a \
         labeling nuance",
        result.partial.len()
    );
}

/// Assert a "negative"/adversarial query intent has zero `Exact` oracle
/// matches in `catalog` -- e.g. a wrong-family typed interpretation that
/// must never resolve to a hard match.
pub fn assert_negative(catalog: &Catalog, forbidden_intent: &QueryIntent, description: &str) {
    let result = oracle::classify(catalog, forbidden_intent);
    assert!(
        result.exact.is_empty(),
        "negative query {description:?} expected zero Exact oracle matches for the forbidden \
         interpretation; found {} -- this would be exactly the wrong-family false-positive class \
         R1 exists to catch",
        result.exact.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::{resolve_product_type, AttrRequirement};
    use commerce_core::domain::AttributeValue;
    use issue38_e2e3_eval::{apparel, automotive, ingest};

    /// A real positive: some generated Wiper Blades product really does
    /// carry a Numeric `size` in automotive's 16..=28 range. Reuses the
    /// *catalog* automotive::generate_catalog materializes, never its own
    /// generate_workload/judgment map.
    #[test]
    fn automotive_wiper_blade_numeric_size_is_a_real_positive() {
        let products = automotive::generate_catalog(200);
        let target = products
            .iter()
            .find(|p| p.product_type == "Wiper Blades")
            .expect("at least one Wiper Blades product at n=200");
        let size = target
            .product_attributes
            .iter()
            .find_map(|(k, v)| match (k.as_str(), v) {
                ("size", AttributeValue::Numeric(n)) => Some(*n),
                _ => None,
            })
            .expect("Wiper Blades always has a Numeric size product attribute");

        let ingested = ingest::build_catalog(&products);
        let intent = QueryIntent {
            product_type: Some(resolve_product_type(
                &ingested.product_types,
                "Wiper Blades",
            )),
            attributes: vec![AttrRequirement::NumericEquals {
                attribute: "size".to_string(),
                value: size,
                epsilon: 1e-9,
            }],
        };
        assert_positive(
            &ingested.catalog,
            &intent,
            &format!("Wiper Blades Numeric size={size} (real generated value)"),
        );
    }

    /// The exact E3/issue #40 size-schema-conflict shape, re-proven here
    /// independently of E3's own binary: a real apparel Jeans product's
    /// Enum `size` value, reinterpreted as a *Numeric* size intent against
    /// the *same* Jeans product type, must have zero Exact matches -- an
    /// Enum "34" and a Numeric 34.0 are never the same fact to this
    /// oracle, even though both stringify to "34".
    #[test]
    fn apparel_jeans_enum_size_never_satisfies_a_numeric_reinterpretation() {
        let products = apparel::generate_catalog(200);
        let target = products
            .iter()
            .find(|p| p.product_type == "Jeans")
            .expect("at least one Jeans product at n=200");
        let size_str = target.variants[0]
            .variant_attributes
            .iter()
            .find_map(|(k, v)| match (k.as_str(), v) {
                ("size", AttributeValue::Enum(s)) => Some(s.clone()),
                _ => None,
            })
            .expect("Jeans variants always have an Enum size attribute");
        let numeric_value: f64 = size_str
            .parse()
            .expect("this generator's Jeans size values are numeric-looking strings");

        let ingested = ingest::build_catalog(&products);
        let forbidden = QueryIntent {
            product_type: Some(resolve_product_type(&ingested.product_types, "Jeans")),
            attributes: vec![AttrRequirement::NumericEquals {
                attribute: "size".to_string(),
                value: numeric_value,
                epsilon: 1e-9,
            }],
        };
        assert_negative(
            &ingested.catalog,
            &forbidden,
            &format!("Jeans, Numeric size={numeric_value} reinterpretation of an Enum value"),
        );
    }

    /// `assert_positive` itself must fail loudly on a genuinely absent
    /// case -- proving the helper is not vacuously true.
    #[test]
    #[should_panic(expected = "expected at least one Exact oracle match")]
    fn assert_positive_panics_when_the_claimed_case_does_not_exist() {
        let products = automotive::generate_catalog(50);
        let ingested = ingest::build_catalog(&products);
        let impossible = QueryIntent {
            product_type: Some(resolve_product_type(
                &ingested.product_types,
                "Wiper Blades",
            )),
            attributes: vec![AttrRequirement::NumericEquals {
                attribute: "size".to_string(),
                value: -999.0,
                epsilon: 1e-9,
            }],
        };
        assert_positive(&ingested.catalog, &impossible, "impossible negative size");
    }

    /// `assert_negative` itself must fail loudly when the forbidden case
    /// actually does occur -- proving the helper is not vacuously true.
    #[test]
    #[should_panic(expected = "expected zero Exact oracle matches")]
    fn assert_negative_panics_when_the_forbidden_case_actually_occurs() {
        let products = automotive::generate_catalog(200);
        let target = products
            .iter()
            .find(|p| p.product_type == "Wiper Blades")
            .expect("at least one Wiper Blades product at n=200");
        let size = target
            .product_attributes
            .iter()
            .find_map(|(k, v)| match (k.as_str(), v) {
                ("size", AttributeValue::Numeric(n)) => Some(*n),
                _ => None,
            })
            .expect("Wiper Blades always has a Numeric size product attribute");
        let ingested = ingest::build_catalog(&products);
        let actually_present = QueryIntent {
            product_type: Some(resolve_product_type(
                &ingested.product_types,
                "Wiper Blades",
            )),
            attributes: vec![AttrRequirement::NumericEquals {
                attribute: "size".to_string(),
                value: size,
                epsilon: 1e-9,
            }],
        };
        assert_negative(
            &ingested.catalog,
            &actually_present,
            "deliberately mislabeled real value",
        );
    }
}
