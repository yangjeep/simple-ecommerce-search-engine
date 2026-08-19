use commerce_core::domain::{Constraint, NumericOp, ProductId, VariantId};
use commerce_core::fixtures::variant_safety_catalog;

fn color(value: &str) -> Constraint {
    Constraint::Enum {
        attribute: "color".to_string(),
        value: value.to_string(),
    }
}

fn size(op: NumericOp, value: f64) -> Constraint {
    Constraint::Numeric {
        attribute: "size".to_string(),
        op,
        value,
    }
}

#[test]
fn black_size_8_matches_only_the_black_variant() {
    let catalog = variant_safety_catalog();
    let hits = catalog.search(&[color("Black"), size(NumericOp::Eq, 8.0)]);
    assert_eq!(hits, vec![(ProductId(1), VariantId(101))]);
}

#[test]
fn red_size_9_matches_only_the_red_variant() {
    let catalog = variant_safety_catalog();
    let hits = catalog.search(&[color("Red"), size(NumericOp::Eq, 9.0)]);
    assert_eq!(hits, vec![(ProductId(1), VariantId(102))]);
}

/// The falsifiable claim from Issue #2 Gate 1: a product with a black
/// size-8 variant and a red size-9 variant must not satisfy "black size 9".
/// A matcher that treats each attribute independently across the whole
/// product (rather than per-variant) would wrongly return a hit here.
#[test]
fn black_size_9_matches_nothing() {
    let catalog = variant_safety_catalog();
    let hits = catalog.search(&[color("Black"), size(NumericOp::Eq, 9.0)]);
    assert!(hits.is_empty(), "cross-variant false match: {hits:?}");
}

#[test]
fn red_size_8_matches_nothing() {
    let catalog = variant_safety_catalog();
    let hits = catalog.search(&[color("Red"), size(NumericOp::Eq, 8.0)]);
    assert!(hits.is_empty(), "cross-variant false match: {hits:?}");
}

#[test]
fn product_level_attributes_apply_to_every_variant() {
    let catalog = variant_safety_catalog();
    let waterproof = Constraint::Boolean {
        attribute: "waterproof".to_string(),
        value: true,
    };
    let hits = catalog.search(&[waterproof]);
    assert_eq!(
        hits,
        vec![
            (ProductId(1), VariantId(101)),
            (ProductId(1), VariantId(102)),
        ]
    );
}

#[test]
fn multi_enum_and_text_constraints_match_shared_attributes() {
    let catalog = variant_safety_catalog();
    let cushioned = Constraint::MultiEnumContains {
        attribute: "features".to_string(),
        value: "cushioned".to_string(),
    };
    let mesh = Constraint::Text {
        attribute: "material".to_string(),
        contains: "mesh".to_string(),
    };
    assert_eq!(catalog.search(&[cushioned]).len(), 2);
    assert_eq!(catalog.search(&[mesh]).len(), 2);
}

#[test]
fn numeric_range_constraint_narrows_by_variant() {
    let catalog = variant_safety_catalog();
    let hits = catalog.search(&[size(NumericOp::Gte, 9.0)]);
    assert_eq!(hits, vec![(ProductId(1), VariantId(102))]);
}
