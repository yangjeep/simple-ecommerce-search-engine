//! Issue #38 E3: the apparel family for the mixed-category merchant
//! catalog. Deliberately shares field *names* with the other families
//! (`color`, `material`, `size`) while using different value vocabularies
//! and, for `size`, a different `AttributeValue` *kind* than the
//! automotive family's numeric `length_inches`-style fields ever use --
//! apparel's `size` is always `Enum` (S/M/L/XL or a numeric-looking
//! string like "34" for waist size, still stored as `Enum` since it is a
//! catalog *label*, not a measured quantity the way `wiper_blades`'
//! `length_inches` is).

use std::collections::BTreeMap;

use commerce_core::domain::AttributeValue;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{RelevanceLabel, SynthProduct, SynthQuery, SynthVariant};

pub const SEED: u64 = 0x38E3_A9A0;
pub const FAMILY: &str = "apparel";

const BRANDS: &[&str] = &[
    "Harborline",
    "Fieldcrest Wear",
    "Northstitch",
    "Cascade Apparel",
];

struct ApparelType {
    name: &'static str,
    category: &'static str,
    sizes: &'static [&'static str],
    materials: &'static [&'static str],
    colors: &'static [&'static str],
}

const APPAREL_TYPES: &[ApparelType] = &[
    ApparelType {
        name: "T-Shirts",
        category: "Apparel / Tops / T-Shirts",
        sizes: &["S", "M", "L", "XL"],
        materials: &["Cotton", "Polyester", "Cotton-Blend"],
        colors: &["Black", "Navy", "Red", "White", "Charcoal"],
    },
    ApparelType {
        name: "Jeans",
        category: "Apparel / Bottoms / Jeans",
        sizes: &["30", "32", "34", "36", "38"],
        materials: &["Denim", "Stretch-Denim"],
        colors: &["Black", "Indigo", "Light Wash"],
    },
    ApparelType {
        name: "Sneakers",
        category: "Apparel / Footwear / Sneakers",
        sizes: &["8", "9", "10", "11", "12"],
        materials: &["Leather", "Mesh", "Canvas"],
        colors: &["Black", "White", "Red"],
    },
    ApparelType {
        name: "Jackets",
        category: "Apparel / Outerwear / Jackets",
        sizes: &["S", "M", "L", "XL"],
        materials: &["Leather", "Nylon", "Wool-Blend"],
        colors: &["Black", "Olive", "Navy"],
    },
];

pub fn generate_catalog(n_products: usize) -> Vec<SynthProduct> {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let mut products = Vec::with_capacity(n_products);

    for i in 0..n_products {
        let t = &APPAREL_TYPES[i % APPAREL_TYPES.len()];
        let brand = BRANDS[rng.gen_range(0..BRANDS.len())];
        let color = t.colors[rng.gen_range(0..t.colors.len())];
        // Sparse: material is deliberately absent on ~20% of products,
        // testing min_enum_frequency-style trust behavior on genuinely
        // incomplete real-world-shaped data, not just absent-by-family.
        let has_material = !rng.gen_bool(0.2);
        let material = t.materials[rng.gen_range(0..t.materials.len())];
        let size = t.sizes[rng.gen_range(0..t.sizes.len())];
        let gender = ["Mens", "Womens", "Unisex"][rng.gen_range(0..3)];

        let mut product_attrs = vec![
            ("color".to_string(), AttributeValue::Enum(color.to_string())),
            (
                "gender".to_string(),
                AttributeValue::Enum(gender.to_string()),
            ),
        ];
        if has_material {
            product_attrs.push((
                "material".to_string(),
                AttributeValue::Enum(material.to_string()),
            ));
        }

        let variant_attrs = vec![("size".to_string(), AttributeValue::Enum(size.to_string()))];

        let title = format!("{brand} {color} {} {}", t.name, gender);
        let external_id = format!("app-{i:05}");
        products.push(SynthProduct {
            external_id: external_id.clone(),
            family: FAMILY,
            product_type: t.name.to_string(),
            category_path: t.category.to_string(),
            brand: brand.to_string(),
            title,
            product_attributes: product_attrs,
            variants: vec![SynthVariant {
                external_id: format!("{external_id}-v0"),
                variant_attributes: variant_attrs,
                price_cents: rng.gen_range(1200..=18000),
            }],
        });
    }

    products
}

pub fn generate_workload(
    products: &[SynthProduct],
) -> (
    Vec<SynthQuery>,
    BTreeMap<String, BTreeMap<String, RelevanceLabel>>,
) {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED.wrapping_add(1));
    let mut queries = Vec::new();
    let mut judgments: BTreeMap<String, BTreeMap<String, RelevanceLabel>> = BTreeMap::new();

    let attr_str = |p: &SynthProduct, name: &str| -> Option<String> {
        p.product_attributes
            .iter()
            .chain(p.variants[0].variant_attributes.iter())
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| match v {
                AttributeValue::Enum(s) => Some(s.clone()),
                _ => None,
            })
    };

    let mut qi = 0;
    let mut push_query =
        |id: String,
         text: String,
         template: &'static str,
         judge: &dyn Fn(&SynthProduct) -> RelevanceLabel| {
            let mut per_query = BTreeMap::new();
            for p in products {
                let label = judge(p);
                if !matches!(label, RelevanceLabel::Irrelevant) {
                    per_query.insert(p.variants[0].external_id.clone(), label);
                }
            }
            judgments.insert(id.clone(), per_query);
            queries.push(SynthQuery { id, text, template });
        };

    // color + product type. Derived from real generated products' own
    // actual color values per type (up to 2 distinct ones encountered),
    // not the type's fixed candidate color list -- guarantees an
    // Exact-labeled match exists for each emitted query. An earlier
    // version iterated `t.colors.iter().take(2)` directly: every query
    // still had *some* non-empty ground truth (a same-type,
    // different-color product is always `Partial`), so
    // `ground_truth::assert_self_consistent` never caught it, but there
    // was no guarantee the query's own claimed `Exact` case (that exact
    // color, for that type) was ever actually generated -- the same
    // "fixed literal, no generation guarantee" defect class an
    // adversarial review found in `size_numeric_keyword` below and in
    // `mixed_merchant.rs`'s `size_schema_conflict` template, just with a
    // `Partial` fallback masking it from the emptiness check.
    for t in APPAREL_TYPES {
        let mut colors_seen: Vec<String> = Vec::new();
        for p in products {
            if p.product_type != t.name {
                continue;
            }
            if let Some(color) = attr_str(p, "color") {
                if !colors_seen.contains(&color) {
                    colors_seen.push(color);
                }
            }
            if colors_seen.len() >= 2 {
                break;
            }
        }
        for color in colors_seen {
            let text = format!("{} {}", color.to_lowercase(), t.name.to_lowercase());
            let id = format!("app-q-color-{qi:03}");
            let color_for_judge = color.clone();
            push_query(id, text, "attribute_plus_entity", &move |p| {
                if p.product_type != t.name {
                    return RelevanceLabel::Irrelevant;
                }
                if attr_str(p, "color").as_deref() == Some(color_for_judge.as_str()) {
                    RelevanceLabel::Exact
                } else {
                    RelevanceLabel::Partial
                }
            });
            qi += 1;
        }
    }

    // numeric-shaped jeans size ("size 34"), the same hard-coded "size N"
    // keyword parse the E3 schema-conflict diagnostic specifically
    // targets. Derived from real generated Jeans products' own actual
    // size values (up to 2 distinct ones encountered), not a fixed
    // literal list -- an earlier version hard-coded `["32", "34"]`
    // directly, the exact "no generation guarantee" bug an adversarial
    // review found here after it was already found and fixed once in
    // `mixed_merchant.rs`'s `size_schema_conflict` template.
    //
    // Not currently exercised by any experiment binary (`e2_unseen_vertical_eval.rs`
    // exercises `automotive`; `e3_mixed_category_eval.rs` exercises
    // `mixed_merchant`'s own cross-category workload, whose
    // `same_catalog_control` template already provides the intended
    // "this family alone, on the identical mixed catalog" control point)
    // -- kept correct and exercised by this crate's own
    // `apparel_ground_truth_is_self_consistent` unit test, not as a
    // stand-in for a full-pipeline measurement that does not exist.
    let mut jeans_sizes_seen: Vec<String> = Vec::new();
    for p in products {
        if p.product_type != "Jeans" {
            continue;
        }
        if let Some(size) = attr_str(p, "size") {
            if !jeans_sizes_seen.contains(&size) {
                jeans_sizes_seen.push(size);
            }
        }
        if jeans_sizes_seen.len() >= 2 {
            break;
        }
    }
    for size in jeans_sizes_seen {
        let text = format!("size {size} jeans");
        let id = format!("app-q-size-{qi:03}");
        let size_for_judge = size.clone();
        push_query(id, text, "size_numeric_keyword", &move |p| {
            if p.product_type != "Jeans" {
                return RelevanceLabel::Irrelevant;
            }
            if attr_str(p, "size").as_deref() == Some(size_for_judge.as_str()) {
                RelevanceLabel::Exact
            } else {
                RelevanceLabel::Partial
            }
        });
        qi += 1;
    }

    queries.shuffle(&mut rng);
    (queries, judgments)
}
