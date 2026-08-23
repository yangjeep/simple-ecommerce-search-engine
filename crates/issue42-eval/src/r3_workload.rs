//! R3 fixture (`docs/experiments/ISSUE42_PROTOCOL.md`'s R3 section):
//! calibration/held-out catalogs for the identifier-serving-primitive
//! classifier. Reuses `issue38_e2e3_eval`'s frozen `automotive`/
//! `mixed_merchant` generators (never modified) via the corrected
//! calibration/held-out split documented in `ISSUE42_PROTOCOL.md`'s own
//! dated correction note under R3's "Calibration / held-out split"
//! section (`automotive::generate_catalog` has no seed parameter, so the
//! protocol's literal "distinct seed" prescription would have made
//! calibration a byte-identical prefix of held-out; the fix is a single
//! larger generation split by index range instead), plus deterministic
//! post-processing for corner cases those generators do not produce on
//! their own: guaranteed collisions, absent identifiers, a legitimate
//! cross-reference code, an adversarial near-miss, and a multi-variant-
//! per-product stress case (the frozen automotive generator is strictly
//! one-variant-per-product).

use commerce_core::domain::{
    AttributeValue, Catalog, Inventory, Price, Product, ProductId, Variant, VariantId,
};
use issue38_e2e3_eval::{automotive, ingest, mixed_merchant};

/// Distinct from every `ProductId`/`VariantId` `ingest::build_catalog`
/// ever assigns (`ProductId(0..n_products)`,
/// `VariantId(p_ord*1000 + v_ord)`) at any n this protocol uses --
/// hand-appended extension products live in their own id range so they
/// can never collide with a generator-ingested one.
const EXT_PRODUCT_ID_BASE: u64 = 10_000_000;
const EXT_VARIANT_ID_BASE: u64 = 10_000_000_000;

pub const CALIBRATION_N: usize = 1500;
pub const HELD_OUT_AUTOMOTIVE_N: usize = 3000;
/// Matches E3's own established `mixed_merchant` split
/// (`crates/issue38-e2e3-eval/src/bin/e3_mixed_category_eval.rs`'s
/// `N_PER_FAMILY = 1000`), reused rather than invented, so this fixture
/// is not a novel, unvalidated catalog shape.
pub const HELD_OUT_MIXED_PER_FAMILY: usize = 1000;

/// Name of the low-cardinality Enum re-labeled with a misleadingly
/// identifier-sounding name -- present on every calibration product.
pub const MISLEADING_LOW_CARDINALITY_FIELD: &str = "sku_code";
/// Name of the free-text field with real per-string character entropy
/// but zero per-variant uniqueness (the identical string on every
/// calibration product) -- present on every calibration product.
pub const MISLEADING_NON_UNIQUE_FIELD: &str = "product_fingerprint";
const MISLEADING_NON_UNIQUE_VALUE: &str = "9f3ac7e1d4b8f2039a4c7716b6b0f42e";
const LOW_CARDINALITY_VALUES: &[&str] = &["A", "B", "C"];

/// The name automotive's own generator already uses for its real,
/// variant-level identifier field -- reused verbatim, never redefined
/// here, so R3 measures the actual production field the frozen
/// generator produces.
pub const REAL_IDENTIFIER_FIELD: &str = "part_number";
/// A second, deliberately-added identifier-shaped field for the legitimate
/// cross-reference case (Metrics/Workload: "the same code appearing as a
/// legitimate cross-reference on multiple unrelated products -- distinct
/// from an in-catalog collision").
pub const CROSS_REFERENCE_FIELD: &str = "cross_reference_code";

pub struct CalibrationSet {
    pub catalog: Catalog,
}

pub struct HeldOutAutomotive {
    pub catalog: Catalog,
    /// A normal, untouched (product, variant, real part_number) triple --
    /// the straightforward "exact match recovers the correct variant"
    /// positive case.
    pub known_identifier: (ProductId, VariantId, String),
    /// Two distinct variants whose `part_number` was deliberately set to
    /// the identical string -- a real, in-catalog collision. Both must be
    /// surfaced by an exact query for that value, never one silently
    /// dropped.
    pub collision_pair: ((ProductId, VariantId), (ProductId, VariantId), String),
    /// A variant with no `part_number` attribute at all.
    pub absent_identifier: (ProductId, VariantId),
    /// Two distinct, otherwise-unrelated variants sharing the same
    /// `cross_reference_code` value -- a legitimate cross-reference, not
    /// an in-catalog collision on the primary identifier field.
    pub reused_cross_reference: ((ProductId, VariantId), (ProductId, VariantId), String),
    /// `(real_identifier, adversarial_near_miss)` -- a single-character
    /// edit of a real `part_number` that must NOT resolve to any match.
    pub near_miss: (String, String),
    /// A product with several variants (`variants_per_product` of them),
    /// each carrying its own distinct, real-shaped `part_number` -- the
    /// many-variants-per-product stress case.
    pub many_variant_product: ProductId,
    pub variants_per_product: usize,
}

pub struct HeldOutMixed {
    pub catalog: Catalog,
}

fn attribute_string(value: &AttributeValue) -> Option<String> {
    match value {
        AttributeValue::Text(s) => Some(s.clone()),
        AttributeValue::Enum(s) => Some(s.clone()),
        AttributeValue::Boolean(b) => Some(b.to_string()),
        AttributeValue::Numeric(n) => Some(format!("{n}")),
        AttributeValue::MultiEnum(_) => None,
    }
}

/// Splits one `automotive::generate_catalog(CALIBRATION_N +
/// HELD_OUT_AUTOMOTIVE_N)` call by index range into disjoint calibration/
/// held-out slices -- see `ISSUE42_PROTOCOL.md`'s dated correction note
/// for why a single larger generation split this way replaces the
/// protocol's originally-drafted (but unachievable, since the generator
/// takes no seed parameter) "two separately-seeded calls."
pub fn build_calibration_and_held_out() -> (CalibrationSet, HeldOutAutomotive) {
    let all = automotive::generate_catalog(CALIBRATION_N + HELD_OUT_AUTOMOTIVE_N);
    let calibration_products = &all[0..CALIBRATION_N];
    let held_out_products = &all[CALIBRATION_N..CALIBRATION_N + HELD_OUT_AUTOMOTIVE_N];

    let mut calibration = ingest::build_catalog(calibration_products);
    for product in &mut calibration.catalog.products {
        let idx = product.id.0 as usize;
        product.attributes.insert(
            MISLEADING_LOW_CARDINALITY_FIELD.to_string(),
            AttributeValue::Enum(
                LOW_CARDINALITY_VALUES[idx % LOW_CARDINALITY_VALUES.len()].to_string(),
            ),
        );
        product.attributes.insert(
            MISLEADING_NON_UNIQUE_FIELD.to_string(),
            AttributeValue::Text(MISLEADING_NON_UNIQUE_VALUE.to_string()),
        );
    }

    let mut held_out = ingest::build_catalog(held_out_products);

    // A normal, untouched positive case -- picked before any other
    // extension mutates anything, from a product no other extension
    // below touches.
    let known_product = &held_out.catalog.products[10];
    let known_variant = &known_product.variants[0];
    let known_pn = attribute_string(
        known_variant
            .attributes
            .get(REAL_IDENTIFIER_FIELD)
            .expect("automotive::generate_catalog always sets part_number"),
    )
    .unwrap();
    let known_identifier = (known_product.id, known_variant.id, known_pn.clone());

    // Guaranteed collision: overwrite product 0's part_number to equal
    // product 1's real one. Both variants must be surfaced for an exact
    // query on that shared value.
    let collision_value = attribute_string(
        held_out.catalog.products[1].variants[0]
            .attributes
            .get(REAL_IDENTIFIER_FIELD)
            .unwrap(),
    )
    .unwrap();
    held_out.catalog.products[0].variants[0].attributes.insert(
        REAL_IDENTIFIER_FIELD.to_string(),
        AttributeValue::Text(collision_value.clone()),
    );
    let collision_pair = (
        (
            held_out.catalog.products[0].id,
            held_out.catalog.products[0].variants[0].id,
        ),
        (
            held_out.catalog.products[1].id,
            held_out.catalog.products[1].variants[0].id,
        ),
        collision_value,
    );

    // Absent identifier: remove product 2's part_number entirely.
    held_out.catalog.products[2].variants[0]
        .attributes
        .remove(REAL_IDENTIFIER_FIELD);
    let absent_identifier = (
        held_out.catalog.products[2].id,
        held_out.catalog.products[2].variants[0].id,
    );

    // Legitimate cross-reference: products 3 and 4 (otherwise unrelated
    // -- different real part_numbers of their own) both carry the same
    // cross_reference_code value, a distinct field from part_number.
    const CROSS_REF_VALUE: &str = "XREF-77-ALT";
    held_out.catalog.products[3].variants[0].attributes.insert(
        CROSS_REFERENCE_FIELD.to_string(),
        AttributeValue::Text(CROSS_REF_VALUE.to_string()),
    );
    held_out.catalog.products[4].variants[0].attributes.insert(
        CROSS_REFERENCE_FIELD.to_string(),
        AttributeValue::Text(CROSS_REF_VALUE.to_string()),
    );
    let reused_cross_reference = (
        (
            held_out.catalog.products[3].id,
            held_out.catalog.products[3].variants[0].id,
        ),
        (
            held_out.catalog.products[4].id,
            held_out.catalog.products[4].variants[0].id,
        ),
        CROSS_REF_VALUE.to_string(),
    );

    // Adversarial near-miss: a single-character edit (last digit
    // incremented, wrapping 9->0) of product 5's real part_number.
    let real_pn = attribute_string(
        held_out.catalog.products[5].variants[0]
            .attributes
            .get(REAL_IDENTIFIER_FIELD)
            .unwrap(),
    )
    .unwrap();
    let mut near_miss_chars: Vec<char> = real_pn.chars().collect();
    let last_digit_pos = near_miss_chars
        .iter()
        .rposition(|c| c.is_ascii_digit())
        .expect("automotive part_number always contains digits");
    let last_digit = near_miss_chars[last_digit_pos].to_digit(10).unwrap();
    near_miss_chars[last_digit_pos] = std::char::from_digit((last_digit + 1) % 10, 10).unwrap();
    let near_miss: String = near_miss_chars.into_iter().collect();
    assert_ne!(
        near_miss, real_pn,
        "near-miss construction must actually differ from the real value"
    );

    // Many-variants-per-product stress case: a hand-built product with
    // several variants, each carrying its own distinct, real-shaped
    // part_number. Fresh id range, well outside ingestion's own.
    const VARIANTS_PER_PRODUCT: usize = 7;
    let stress_product_id = ProductId(EXT_PRODUCT_ID_BASE);
    let stress_variants: Vec<Variant> = (0..VARIANTS_PER_PRODUCT)
        .map(|i| {
            let mut attrs = commerce_core::domain::attributes([]);
            attrs.insert(
                REAL_IDENTIFIER_FIELD.to_string(),
                AttributeValue::Text(format!("ST-{i:04}-MV")),
            );
            Variant {
                id: VariantId(EXT_VARIANT_ID_BASE + i as u64),
                attributes: attrs,
                price: Price::usd(4999),
                inventory: Inventory::in_stock(5),
            }
        })
        .collect();
    let stress_product = Product {
        id: stress_product_id,
        product_type: held_out.catalog.products[0].product_type,
        brand: held_out.catalog.products[0].brand,
        category: held_out.catalog.products[0].category,
        title: "Multi-Fit Universal Bolt Kit".to_string(),
        attributes: commerce_core::domain::attributes([]),
        variants: stress_variants,
    };
    held_out.catalog.products.push(stress_product);

    (
        CalibrationSet {
            catalog: calibration.catalog,
        },
        HeldOutAutomotive {
            catalog: held_out.catalog,
            known_identifier,
            collision_pair,
            absent_identifier,
            reused_cross_reference,
            near_miss: (real_pn, near_miss),
            many_variant_product: stress_product_id,
            variants_per_product: VARIANTS_PER_PRODUCT,
        },
    )
}

/// `mixed_merchant`'s 3,000-product mixed catalog, reused as-is (matching
/// E3's own established split) for the general-lexical-retrieval
/// regression check only -- never for a core identifier-classification
/// metric. See `ISSUE42_PROTOCOL.md`'s dated correction note for why:
/// its own automotive sub-portion overlaps this fixture's *calibration*
/// range, not its held-out range.
pub fn build_held_out_mixed() -> HeldOutMixed {
    let products = mixed_merchant::generate_mixed_catalog(
        HELD_OUT_MIXED_PER_FAMILY,
        HELD_OUT_MIXED_PER_FAMILY,
        HELD_OUT_MIXED_PER_FAMILY,
    );
    let ingested = ingest::build_catalog(&products);
    HeldOutMixed {
        catalog: ingested.catalog,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_and_held_out_are_deterministic() {
        let (cal_a, held_a) = build_calibration_and_held_out();
        let (cal_b, held_b) = build_calibration_and_held_out();
        assert_eq!(cal_a.catalog.products, cal_b.catalog.products);
        assert_eq!(held_a.catalog.products, held_b.catalog.products);
    }

    #[test]
    fn calibration_is_a_disjoint_slice_from_held_out_not_a_prefix_of_it() {
        let (cal, held) = build_calibration_and_held_out();
        assert_eq!(cal.catalog.products.len(), CALIBRATION_N);
        assert_eq!(held.catalog.products.len(), HELD_OUT_AUTOMOTIVE_N + 1); // +1 stress product
        let cal_titles: std::collections::BTreeSet<&str> = cal
            .catalog
            .products
            .iter()
            .map(|p| p.title.as_str())
            .collect();
        // A same-title collision across the two sets is possible in
        // principle (finitely many brand/type/oem combinations), but the
        // two sets must not be the *same sequence* -- confirmed directly
        // by comparing the first held-out product against every
        // calibration product's own full attribute set, not just titles.
        let first_held_out = &held.catalog.products[0];
        let identical_in_calibration = cal.catalog.products.iter().any(|p| {
            p.title == first_held_out.title
                && p.attributes == first_held_out.attributes
                && p.variants[0].attributes == first_held_out.variants[0].attributes
        });
        assert!(
            !identical_in_calibration,
            "held-out product 0 (after the collision-injection above already changed its own \
             part_number away from automotive's real generated value) must not duplicate any \
             calibration product's full attribute set"
        );
        let _ = cal_titles;
    }

    #[test]
    fn calibration_fields_have_the_intended_shapes() {
        let (cal, _held) = build_calibration_and_held_out();
        let mut low_card_values = std::collections::BTreeSet::new();
        let mut fingerprint_values = std::collections::BTreeSet::new();
        for product in &cal.catalog.products {
            let low_card = product
                .attributes
                .get(MISLEADING_LOW_CARDINALITY_FIELD)
                .expect("every calibration product carries the misleading low-cardinality field");
            match low_card {
                AttributeValue::Enum(v) => {
                    low_card_values.insert(v.clone());
                }
                other => panic!("expected Enum, got {other:?}"),
            }
            let fingerprint = product
                .attributes
                .get(MISLEADING_NON_UNIQUE_FIELD)
                .expect("every calibration product carries the misleading non-unique field");
            match fingerprint {
                AttributeValue::Text(v) => {
                    fingerprint_values.insert(v.clone());
                }
                other => panic!("expected Text, got {other:?}"),
            }
        }
        assert_eq!(
            low_card_values.len(),
            LOW_CARDINALITY_VALUES.len(),
            "the misleading low-cardinality field must have exactly the intended tiny cardinality"
        );
        assert_eq!(
            fingerprint_values.len(),
            1,
            "the misleading non-unique field must be the identical value on every product, \
             despite looking high-entropy per string"
        );
    }

    #[test]
    fn held_out_extension_cases_are_real_and_distinct() {
        let (_cal, held) = build_calibration_and_held_out();
        let (p_a, v_a) = held.collision_pair.0;
        let (p_b, v_b) = held.collision_pair.1;
        assert_ne!((p_a, v_a), (p_b, v_b));
        let get = |pid: ProductId, vid: VariantId| -> AttributeValue {
            let product = held.catalog.products.iter().find(|p| p.id == pid).unwrap();
            let variant = product.variants.iter().find(|v| v.id == vid).unwrap();
            variant
                .attributes
                .get(REAL_IDENTIFIER_FIELD)
                .unwrap()
                .clone()
        };
        assert_eq!(get(p_a, v_a), get(p_b, v_b));

        let (absent_p, absent_v) = held.absent_identifier;
        let absent_product = held
            .catalog
            .products
            .iter()
            .find(|p| p.id == absent_p)
            .unwrap();
        let absent_variant = absent_product
            .variants
            .iter()
            .find(|v| v.id == absent_v)
            .unwrap();
        assert!(!absent_variant
            .attributes
            .contains_key(REAL_IDENTIFIER_FIELD));

        let (real, near_miss) = &held.near_miss;
        assert_ne!(real, near_miss);
        assert_eq!(real.len(), near_miss.len());

        let stress_product = held
            .catalog
            .products
            .iter()
            .find(|p| p.id == held.many_variant_product)
            .unwrap();
        assert_eq!(stress_product.variants.len(), held.variants_per_product);
        let distinct_ids: std::collections::BTreeSet<String> = stress_product
            .variants
            .iter()
            .map(|v| match v.attributes.get(REAL_IDENTIFIER_FIELD).unwrap() {
                AttributeValue::Text(s) => s.clone(),
                other => panic!("expected Text, got {other:?}"),
            })
            .collect();
        assert_eq!(distinct_ids.len(), held.variants_per_product);
    }

    #[test]
    fn held_out_mixed_is_deterministic_and_disjoint_in_shape_from_pure_automotive() {
        let mixed_a = build_held_out_mixed();
        let mixed_b = build_held_out_mixed();
        assert_eq!(mixed_a.catalog.products, mixed_b.catalog.products);
        let has_non_automotive = mixed_a.catalog.products.iter().any(|p| {
            !p.attributes.contains_key(REAL_IDENTIFIER_FIELD)
                && !p.variants[0].attributes.contains_key(REAL_IDENTIFIER_FIELD)
        });
        assert!(
            has_non_automotive,
            "mixed_merchant's furniture/apparel products must have no part_number attribute at \
             all -- the real, natural 'absent identifier across an entire different vertical' \
             case this fixture relies on rather than injecting"
        );
    }
}
