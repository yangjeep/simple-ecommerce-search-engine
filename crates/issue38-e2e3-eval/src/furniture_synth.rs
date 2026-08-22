//! Issue #38 E3: a small synthetic furniture family for the mixed-category
//! merchant catalog. Deliberately *not* real WANDS data (self-contained,
//! license-free, and clearly synthetic) -- shaped like WANDS's own real
//! schema (product_type/category/color/material) specifically so this
//! family shares the `color`/`material` field *names* with apparel and
//! automotive, the ambiguity E3 exists to test, plus deliberately noisy
//! titles (promotional junk, inconsistent casing, typos) that real WANDS
//! titles mostly do not have.

use std::collections::BTreeMap;

use commerce_core::domain::AttributeValue;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{RelevanceLabel, SynthProduct, SynthQuery, SynthVariant};

pub const SEED: u64 = 0x38E3_F0F0;
pub const FAMILY: &str = "furniture";

const BRANDS: &[&str] = &["Hearthstone Home", "Meadowbrook", "Craftline"];

struct FurnitureType {
    name: &'static str,
    category: &'static str,
    materials: &'static [&'static str],
    colors: &'static [&'static str],
}

const FURNITURE_TYPES: &[FurnitureType] = &[
    FurnitureType {
        name: "Coffee Tables",
        category: "Furniture / Living Room / Tables / Coffee Tables",
        materials: &["Wood", "Glass", "Metal"],
        colors: &["Espresso", "Oak", "Black", "White"],
    },
    FurnitureType {
        name: "Sofas",
        category: "Furniture / Living Room / Seating / Sofas",
        materials: &["Fabric", "Leather", "Velvet"],
        colors: &["Charcoal", "Navy", "Black", "Beige"],
    },
    FurnitureType {
        name: "Bookcases",
        category: "Furniture / Storage / Bookcases",
        materials: &["Wood", "Metal"],
        colors: &["Espresso", "White", "Black"],
    },
    FurnitureType {
        name: "Dining Chairs",
        category: "Furniture / Dining Room / Seating / Dining Chairs",
        materials: &["Wood", "Leather", "Metal"],
        colors: &["Black", "Oak", "White"],
    },
];

// Deliberate title noise: promotional junk, inconsistent casing, common
// typos -- real merchant catalogs are messier than WANDS's own
// relatively clean titles, and E3 is specifically supposed to stress
// that gap.
const NOISE_PREFIXES: &[&str] = &["NEW!! ", "", "", "BEST SELLER: ", ""];
const NOISE_SUFFIXES: &[&str] = &[" - FREE SHIPPING", "", "", " w/ Storage", " (Qty 1)"];

fn maybe_typo(word: &str, rng: &mut ChaCha8Rng) -> String {
    // A single, deterministic, disclosed typo pattern (doubled letter),
    // applied to ~15% of titles -- not meant to be exhaustive, just a
    // real, non-invented source of noise on a fixed budget.
    if rng.gen_bool(0.15) && word.len() > 3 {
        let mid = word.len() / 2;
        format!("{}{}{}", &word[..mid], &word[mid..mid + 1], &word[mid..])
    } else {
        word.to_string()
    }
}

pub fn generate_catalog(n_products: usize) -> Vec<SynthProduct> {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let mut products = Vec::with_capacity(n_products);

    for i in 0..n_products {
        let t = &FURNITURE_TYPES[i % FURNITURE_TYPES.len()];
        let brand = BRANDS[rng.gen_range(0..BRANDS.len())];
        let color = t.colors[rng.gen_range(0..t.colors.len())];
        let has_material = !rng.gen_bool(0.15);
        let material = t.materials[rng.gen_range(0..t.materials.len())];

        let mut product_attrs =
            vec![("color".to_string(), AttributeValue::Enum(color.to_string()))];
        if has_material {
            product_attrs.push((
                "material".to_string(),
                AttributeValue::Enum(material.to_string()),
            ));
        }

        let prefix = NOISE_PREFIXES[rng.gen_range(0..NOISE_PREFIXES.len())];
        let suffix = NOISE_SUFFIXES[rng.gen_range(0..NOISE_SUFFIXES.len())];
        let noisy_name = maybe_typo(t.name, &mut rng);
        let title = format!("{prefix}{brand} {color} {noisy_name}{suffix}");
        let external_id = format!("furn-{i:05}");

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
                variant_attributes: vec![],
                price_cents: rng.gen_range(4000..=120000),
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

    // Derived from real generated products' own actual color values per
    // type (up to 2 distinct ones encountered), not the type's fixed
    // candidate color list -- guarantees an Exact-labeled match exists
    // for each emitted query. An earlier version iterated
    // `t.colors.iter().take(2)` directly: every query still had *some*
    // non-empty ground truth (a same-type, different-color product is
    // always `Partial`), so `ground_truth::assert_self_consistent` never
    // caught it, but there was no guarantee the query's own claimed
    // `Exact` case was ever actually generated -- the same "fixed
    // literal, no generation guarantee" defect class an adversarial
    // review found in `apparel.rs`'s color loop and
    // `mixed_merchant.rs`'s `size_schema_conflict` template.
    for t in FURNITURE_TYPES {
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
            let id = format!("furn-q-color-{qi:03}");
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

    queries.shuffle(&mut rng);
    (queries, judgments)
}
