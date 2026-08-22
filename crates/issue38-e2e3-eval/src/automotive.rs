//! Issue #38 E2: a deterministic synthetic automotive-parts catalog --
//! the "previously unseen vertical" this experiment needs. Materially
//! different from WANDS (home furnishings) in every dimension that
//! matters for the E1-established architecture:
//!
//! - **Attribute schema**: `thread_size`, `voltage`, `cold_cranking_amps`,
//!   `heat_range`, `micron_rating`, `bulb_type`, `group_size` -- none of
//!   these exist in WANDS at all.
//! - **A genuinely new structural relationship**: `compatible_fitment`, a
//!   `MultiEnum` of `"{year} {make} {model}"` phrases (e.g. `"2015 honda
//!   civic"`) a part variant fits. Fitment is a many-to-many compatibility
//!   relationship, not a single-valued entity/attribute the way
//!   `product_type`/`color` are -- the first structural pattern this
//!   project's own architecture has not yet been measured against.
//!   Represented via the *already-existing* `Constraint::MultiEnumContains`
//!   mechanism (see this crate's own `lib.rs` doc comment for why that
//!   choice, not a new `StructuralConstraint` variant, is the in-scope
//!   one). **Deliberately a natural, space-joined phrase, not a
//!   pipe-joined key** (`fitment_tag`'s own doc comment): `ir::query::compile`
//!   only ever matches an exact, space-joined token window against the
//!   lexicon, so a pipe-joined key could never be discovered by any real
//!   query phrasing at all -- caught by direct code read before this
//!   generator was ever run, not after a failed experiment.
//! - **Shopper query vocabulary**: "brake pads for 2015 honda civic",
//!   "12v car battery", "aftermarket alternator" -- unrelated to any
//!   WANDS query pattern.
//!
//! Brand names are entirely fictional (no real aftermarket-parts brand is
//! named), to avoid implying any real-world OEM affiliation; vehicle
//! make/model names are real (Honda, Toyota, ...) since fitment
//! compatibility is a factual, non-trademark-sensitive descriptive
//! attribute in real aftermarket-parts commerce, exactly like listing a
//! shoe size or a paint color.

use std::collections::BTreeMap;

use commerce_core::domain::AttributeValue;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{RelevanceLabel, SynthProduct, SynthQuery, SynthVariant};

/// Fixed so every run of this generator produces byte-for-byte the same
/// catalog -- see this crate's `lib.rs` doc comment.
pub const SEED: u64 = 0x38E2_A0A0;

pub const FAMILY: &str = "automotive";

type AttrGenFn = fn(&mut ChaCha8Rng) -> Vec<(String, AttributeValue)>;

struct ProductTypeSpec {
    name: &'static str,
    category: &'static str,
    has_fitment: bool,
    attrs: AttrGenFn,
}

const BRANDS: &[&str] = &[
    "Ironclad Auto",
    "TrueDrive",
    "Apex Motorworks",
    "Vantage Parts",
    "Northbridge Auto",
    "Redline Supply",
    "Coastal Motor Co",
    "Summit Auto Parts",
];

/// (make, model, count_of_model_years) -- deliberately small and
/// deterministic so fitment overlap between products is high enough to
/// produce real Partial-vs-Exact contrast, matching a real aftermarket
/// catalog's own compatibility clustering.
const VEHICLES: &[(&str, &str)] = &[
    ("honda", "civic"),
    ("honda", "accord"),
    ("toyota", "camry"),
    ("toyota", "corolla"),
    ("ford", "f-150"),
    ("chevrolet", "silverado"),
    ("nissan", "altima"),
];
const YEARS: &[u32] = &[2012, 2013, 2014, 2015, 2016, 2017, 2018, 2019, 2020];

/// **Deliberately a natural, space-joined phrase ("2015 honda civic"), not
/// a pipe-joined key ("honda|civic|2015")**: `ir::query::compile`'s real
/// phrase-lexicon lookup (`crates/commerce-core/src/ir/query.rs`) matches
/// only an exact, space-joined, lowercased token window against
/// `SemanticLexicon`'s keys (`tokens[i..i+window].join(" ")`) -- a
/// pipe-joined key can never equal any such window, no matter how a
/// shopper phrases the query, which would have made this crate's own
/// fitment queries silently untestable through the real compile() path
/// (verified by direct read of `query.rs`'s `compile` before this was
/// ever run, not discovered after a failed experiment run). This format
/// matches this module's own query templates' word order exactly
/// ("brake pads for {year} {make} {model}"), so it is a real, literal,
/// discoverable lexicon entry -- not an injected shortcut.
fn fitment_tag(make: &str, model: &str, year: u32) -> String {
    format!("{year} {make} {model}")
}

fn random_fitment_set(rng: &mut ChaCha8Rng) -> Vec<String> {
    // Most parts fit one vehicle line across a contiguous multi-year
    // range (a real aftermarket-parts pattern: "fits Civic 2014-2017"),
    // not an arbitrary scatter -- so Partial-vs-Exact ground truth means
    // something (adjacent years are a plausible near-miss, an unrelated
    // make is not).
    let (make, model) = VEHICLES[rng.gen_range(0..VEHICLES.len())];
    let start_idx = rng.gen_range(0..YEARS.len());
    let span = rng.gen_range(1..=4.min(YEARS.len() - start_idx));
    YEARS[start_idx..start_idx + span]
        .iter()
        .map(|&y| fitment_tag(make, model, y))
        .collect()
}

fn part_number(rng: &mut ChaCha8Rng, brand: &str, type_code: &str) -> String {
    let brand_code: String = brand
        .split_whitespace()
        .map(|w| w.chars().next().unwrap_or('X'))
        .collect::<String>()
        .to_uppercase();
    format!("{brand_code}-{:04}-{type_code}", rng.gen_range(1000..9999))
}

const PRODUCT_TYPES: &[ProductTypeSpec] = &[
    ProductTypeSpec {
        name: "Brake Pads",
        category: "Automotive / Brakes / Brake Pads",
        has_fitment: true,
        attrs: |rng| {
            let position = ["Front", "Rear"][rng.gen_range(0..2)];
            let grade = ["Ceramic", "Semi-Metallic", "Organic"][rng.gen_range(0..3)];
            vec![
                (
                    "position".to_string(),
                    AttributeValue::Enum(position.to_string()),
                ),
                (
                    "material_grade".to_string(),
                    AttributeValue::Enum(grade.to_string()),
                ),
            ]
        },
    },
    ProductTypeSpec {
        name: "Oil Filters",
        category: "Automotive / Engine / Filters / Oil Filters",
        has_fitment: true,
        attrs: |rng| {
            vec![(
                "micron_rating".to_string(),
                AttributeValue::Numeric(rng.gen_range(10..=40) as f64),
            )]
        },
    },
    ProductTypeSpec {
        name: "Spark Plugs",
        category: "Automotive / Engine / Ignition / Spark Plugs",
        has_fitment: true,
        attrs: |rng| {
            let thread = ["14mm", "12mm", "10mm"][rng.gen_range(0..3)];
            vec![
                (
                    "thread_size".to_string(),
                    AttributeValue::Enum(thread.to_string()),
                ),
                (
                    "heat_range".to_string(),
                    AttributeValue::Numeric(rng.gen_range(4..=8) as f64),
                ),
            ]
        },
    },
    ProductTypeSpec {
        name: "Headlight Bulbs",
        category: "Automotive / Lighting / Headlight Bulbs",
        has_fitment: false,
        attrs: |rng| {
            let bulb = ["H11", "H4", "H7", "9005"][rng.gen_range(0..4)];
            vec![
                (
                    "bulb_type".to_string(),
                    AttributeValue::Enum(bulb.to_string()),
                ),
                (
                    "lumens".to_string(),
                    AttributeValue::Numeric(rng.gen_range(800..=2200) as f64),
                ),
            ]
        },
    },
    ProductTypeSpec {
        name: "Air Filters",
        category: "Automotive / Engine / Filters / Air Filters",
        has_fitment: true,
        attrs: |rng| {
            let ftype = ["Panel", "Cylindrical"][rng.gen_range(0..2)];
            vec![(
                "filter_type".to_string(),
                AttributeValue::Enum(ftype.to_string()),
            )]
        },
    },
    ProductTypeSpec {
        name: "Wiper Blades",
        category: "Automotive / Exterior / Wiper Blades",
        has_fitment: true,
        attrs: |rng| {
            // Deliberately named "size", not "length_inches": a real
            // shopper does search "22 inch wiper blade size", and this
            // creates a genuine, naturally-arising E3 schema conflict --
            // apparel's own "size" attribute (Enum: S/M/L or a
            // numeric-looking string) collides with this Numeric one
            // under the identical field name, not an artificially
            // injected test fixture. See `mixed_merchant.rs`'s doc
            // comment.
            vec![(
                "size".to_string(),
                AttributeValue::Numeric(rng.gen_range(16..=28) as f64),
            )]
        },
    },
    ProductTypeSpec {
        name: "Car Batteries",
        category: "Automotive / Electrical / Batteries",
        has_fitment: false,
        attrs: |rng| {
            let group = ["24F", "35", "65"][rng.gen_range(0..3)];
            vec![
                ("voltage".to_string(), AttributeValue::Numeric(12.0)),
                (
                    "cold_cranking_amps".to_string(),
                    AttributeValue::Numeric(rng.gen_range(400..=850) as f64),
                ),
                (
                    "group_size".to_string(),
                    AttributeValue::Enum(group.to_string()),
                ),
            ]
        },
    },
    ProductTypeSpec {
        name: "Alternators",
        category: "Automotive / Electrical / Alternators",
        has_fitment: true,
        attrs: |rng| {
            vec![(
                "amperage_output".to_string(),
                AttributeValue::Numeric(rng.gen_range(90..=170) as f64),
            )]
        },
    },
    ProductTypeSpec {
        name: "Radiators",
        category: "Automotive / Cooling / Radiators",
        has_fitment: true,
        attrs: |rng| {
            let m = ["Aluminum", "Copper-Brass"][rng.gen_range(0..2)];
            vec![("material".to_string(), AttributeValue::Enum(m.to_string()))]
        },
    },
    ProductTypeSpec {
        name: "Shock Absorbers",
        category: "Automotive / Suspension / Shock Absorbers",
        has_fitment: true,
        attrs: |rng| {
            let position = ["Front", "Rear"][rng.gen_range(0..2)];
            vec![(
                "position".to_string(),
                AttributeValue::Enum(position.to_string()),
            )]
        },
    },
];

/// `n_products` real products (each with 1 variant, matching this crate's
/// "one SKU = one product+variant" simplification, the same one
/// `phase6a_eval::catalog`/`round1_eval::catalog` already use for their
/// own real-data ingestion).
pub fn generate_catalog(n_products: usize) -> Vec<SynthProduct> {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    let mut products = Vec::with_capacity(n_products);

    for i in 0..n_products {
        let spec = &PRODUCT_TYPES[i % PRODUCT_TYPES.len()];
        let brand = BRANDS[rng.gen_range(0..BRANDS.len())];
        let oem = if rng.gen_bool(0.3) {
            "OEM"
        } else {
            "Aftermarket"
        };
        let type_code: String = spec
            .name
            .split_whitespace()
            .map(|w| w.chars().next().unwrap_or('X'))
            .collect::<String>()
            .to_uppercase();
        let pn = part_number(&mut rng, brand, &type_code);
        let warranty = [12, 24, 36, 60][rng.gen_range(0..4)];

        let mut product_attrs = (spec.attrs)(&mut rng);
        product_attrs.push((
            "oem_or_aftermarket".to_string(),
            AttributeValue::Enum(oem.to_string()),
        ));

        let mut variant_attrs = vec![
            ("part_number".to_string(), AttributeValue::Text(pn.clone())),
            (
                "warranty_months".to_string(),
                AttributeValue::Numeric(warranty as f64),
            ),
        ];
        if spec.has_fitment {
            variant_attrs.push((
                "compatible_fitment".to_string(),
                AttributeValue::MultiEnum(random_fitment_set(&mut rng)),
            ));
        }

        let title = format!("{brand} {} {}", spec.name, oem);
        let external_id = format!("auto-{i:05}");
        products.push(SynthProduct {
            external_id: external_id.clone(),
            family: FAMILY,
            product_type: spec.name.to_string(),
            category_path: spec.category.to_string(),
            brand: brand.to_string(),
            title,
            product_attributes: product_attrs,
            variants: vec![SynthVariant {
                external_id: format!("{external_id}-v0"),
                variant_attributes: variant_attrs,
                price_cents: rng.gen_range(1500..=45000),
            }],
        });
    }

    products
}

/// Ground truth is computed directly from the same generation parameters
/// above, not hand-labeled after the fact -- every judgment is derived by
/// re-deriving each product's own attributes and comparing them to the
/// query's own intended target, so it is exactly as trustworthy as the
/// generator itself.
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
    let attr_multi = |p: &SynthProduct, name: &str| -> Option<Vec<String>> {
        p.variants[0]
            .variant_attributes
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| match v {
                AttributeValue::MultiEnum(vs) => Some(vs.clone()),
                _ => None,
            })
    };

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

    // Template 1: fitment-exact structural query, the core new fitment
    // pattern this experiment exists to test. Derived from real generated
    // Brake Pads products' own actual `compatible_fitment` values (up to
    // 8 distinct ones encountered), not a fixed `VEHICLES`/`YEARS`
    // candidate iteration -- an adversarial review found the original
    // fixed-candidate version could silently emit a query whose claimed
    // `Exact` case was never actually generated (caught by a dedicated
    // regression test, `tests::automotive_ground_truth_is_self_consistent`,
    // at this crate's own smaller test catalog size; this experiment's
    // real E2 run happened to use a large enough catalog for the bug to
    // stay hidden by chance).
    let mut qi = 0;
    let mut fitment_tags_seen: Vec<String> = Vec::new();
    for p in products {
        if p.product_type != "Brake Pads" {
            continue;
        }
        if let Some(fits) = attr_multi(p, "compatible_fitment") {
            for tag in fits {
                if !fitment_tags_seen.contains(&tag) {
                    fitment_tags_seen.push(tag);
                }
            }
        }
        if fitment_tags_seen.len() >= 8 {
            break;
        }
    }
    for tag in fitment_tags_seen {
        let text = format!("brake pads for {tag}");
        let id = format!("auto-q-fitment-{qi:03}");
        let tag_for_judge = tag.clone();
        push_query(id, text, "fitment_exact", &move |p| {
            if p.product_type != "Brake Pads" {
                return RelevanceLabel::Irrelevant;
            }
            match attr_multi(p, "compatible_fitment") {
                Some(fits) if fits.contains(&tag_for_judge) => RelevanceLabel::Exact,
                Some(_) => RelevanceLabel::Partial,
                None => RelevanceLabel::Partial,
            }
        });
        qi += 1;
    }

    // Template 2: material_grade + position, two independent attributes,
    // both corroborated by the same registered product-type phrase.
    for grade in ["Ceramic", "Semi-Metallic", "Organic"] {
        for position in ["Front", "Rear"] {
            let text = format!(
                "{} {} brake pads",
                grade.to_lowercase(),
                position.to_lowercase()
            );
            let id = format!("auto-q-gradepos-{qi:03}");
            push_query(id, text, "attribute_plus_entity", &|p| {
                if p.product_type != "Brake Pads" {
                    return RelevanceLabel::Irrelevant;
                }
                let g = attr_str(p, "material_grade");
                let pos = attr_str(p, "position");
                match (
                    g.as_deref() == Some(grade),
                    pos.as_deref() == Some(position),
                ) {
                    (true, true) => RelevanceLabel::Exact,
                    (true, false) | (false, true) => RelevanceLabel::Partial,
                    (false, false) => RelevanceLabel::Partial,
                }
            });
            qi += 1;
        }
    }

    // Template 3: thread_size spark plugs.
    for thread in ["14mm", "12mm", "10mm"] {
        let text = format!("{thread} spark plug");
        let id = format!("auto-q-thread-{qi:03}");
        push_query(id, text, "attribute_plus_entity", &|p| {
            if p.product_type != "Spark Plugs" {
                return RelevanceLabel::Irrelevant;
            }
            if attr_str(p, "thread_size").as_deref() == Some(thread) {
                RelevanceLabel::Exact
            } else {
                RelevanceLabel::Partial
            }
        });
        qi += 1;
    }

    // Template 4: voltage batteries (numeric).
    {
        let text = "12v car battery".to_string();
        let id = format!("auto-q-voltage-{qi:03}");
        push_query(id, text, "numeric_entity", &|p| {
            if p.product_type == "Car Batteries" {
                RelevanceLabel::Exact
            } else {
                RelevanceLabel::Irrelevant
            }
        });
        qi += 1;
    }

    // Template 5: oem/aftermarket + product type.
    for oem in ["oem", "aftermarket"] {
        for spec in PRODUCT_TYPES.iter().take(3) {
            let text = format!("{oem} {}", spec.name.to_lowercase());
            let id = format!("auto-q-oem-{qi:03}");
            let want = if oem == "oem" { "OEM" } else { "Aftermarket" };
            push_query(id, text, "attribute_plus_entity", &|p| {
                if p.product_type != spec.name {
                    return RelevanceLabel::Irrelevant;
                }
                if attr_str(p, "oem_or_aftermarket").as_deref() == Some(want) {
                    RelevanceLabel::Exact
                } else {
                    RelevanceLabel::Partial
                }
            });
            qi += 1;
        }
    }

    // Template 6: exact part-number lookup, a clean single-hit case.
    for p in products.iter().step_by(97) {
        let pn = p.variants[0]
            .variant_attributes
            .iter()
            .find(|(k, _)| k == "part_number")
            .and_then(|(_, v)| match v {
                AttributeValue::Text(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap();
        let text = format!("part number {pn}");
        let id = format!("auto-q-partnum-{qi:03}");
        let target_id = p.variants[0].external_id.clone();
        push_query(id, text, "exact_lookup", &move |candidate| {
            if candidate.variants[0].external_id == target_id {
                RelevanceLabel::Exact
            } else {
                RelevanceLabel::Irrelevant
            }
        });
        qi += 1;
    }

    // Template 7 (regression check, not renamed WANDS): incomplete
    // product-type phrasing designed to make the *only* recognized
    // phrase a coincidental attribute value, with no entity phrase
    // present at all -- the exact P9-E05 resolution-priority scenario,
    // reproduced against fully unrelated vocabulary. "pads" alone is
    // never registered (only the 2-word "brake pads" phrase is), so
    // "ceramic front pads" should resolve material_grade=Ceramic +
    // position=Front with NO entity -- both must demote to Preference
    // per the P9-E05 rule, not become a wrong hard filter.
    for grade in ["Ceramic", "Organic"] {
        let text = format!("{} front pads", grade.to_lowercase());
        let id = format!("auto-q-vocabgap-{qi:03}");
        push_query(id, text, "vocabulary_gap_no_entity", &|p| {
            if p.product_type != "Brake Pads" {
                return RelevanceLabel::Irrelevant;
            }
            let g = attr_str(p, "material_grade");
            let pos = attr_str(p, "position");
            match (g.as_deref() == Some(grade), pos.as_deref() == Some("Front")) {
                (true, true) => RelevanceLabel::Exact,
                (true, false) | (false, true) => RelevanceLabel::Partial,
                (false, false) => RelevanceLabel::Partial,
            }
        });
        qi += 1;
    }

    // Shuffle deterministically so query order is not itself an artifact
    // of template authoring order.
    queries.shuffle(&mut rng);
    (queries, judgments)
}
