//! Issue #38 E3: assembles the furniture + apparel + automotive families
//! into one mixed-category merchant catalog, and generates the
//! cross-category-ambiguous query workload E3 specifically exists to
//! test.
//!
//! **Shared ambiguous fields, by design**: `color` (furniture:
//! Espresso/Oak/Black/White; apparel: Black/Navy/Red/White/Charcoal;
//! automotive has no `color` at all -- a real sparse-field case, not
//! every family populates every shared name). `material` (furniture:
//! Wood/Glass/Metal/Fabric/Leather/Velvet; apparel:
//! Cotton/Polyester/Leather/Denim/Mesh/Canvas/Nylon/Wool-Blend;
//! automotive: Aluminum/Copper-Brass -- "Leather" genuinely overlaps
//! between furniture and apparel). `size` -- the deliberate schema
//! conflict: apparel's `size` is always `AttributeValue::Enum`
//! (`"S"`/`"34"`/...); automotive's Wiper Blades `size` is always
//! `AttributeValue::Numeric` (see `automotive.rs`'s own doc comment on
//! why this is a naturally-arising conflict, not an injected fixture).
//!
//! **The `compile()`-level consequence this naturally exposes**: the
//! "size N" keyword branch in `ir::query::compile` (`crates/commerce-core/src/ir/query.rs`)
//! unconditionally resolves any `"size <number>"` phrase to a hard
//! `Constraint::Numeric{attribute:"size", ...}` *before* the phrase-
//! lexicon scan ever runs -- it never has a chance to discover that
//! `"34"` is *also* a registered `Enum` candidate for apparel jeans sizing.
//! `Constraint::matches`'s own `_ => false` catch-all means this can
//! never produce a *wrong* hard filter (a `Numeric` constraint checked
//! against an `Enum`-valued attribute always safely returns `false`, no
//! panic, no false positive -- verified directly against
//! `crates/commerce-core/src/domain/constraint.rs`), but it does mean a
//! query like "size 34" can only ever discover the automotive
//! (Numeric-typed) product family, never the apparel (Enum-typed) one --
//! a real, disclosed **recall gap**, not a correctness violation. See
//! `docs/experiments/ISSUE38_LOG.md`'s E3 section for the measured
//! consequence and why it is filed as a scoped design question rather
//! than patched here.
//!
//! **A second finding, found by direct measurement while building this
//! module's own `same_catalog_control` template, not anticipated in
//! advance**: `commerce_core::plan::execute_planned`'s `Hybrid`/`Punt`
//! outcomes let the lexical delegate's *residual* free-text match veto an
//! otherwise well-formed, correctly-narrowed structural query. A query
//! combining a recognized `ProductType` phrase with one extra residual
//! word that matches zero indexed documents (e.g. "furniture sofas",
//! where "furniture" appears in no generated title) returns *zero* hits
//! end to end, even though the `ProductType` constraint alone identifies
//! hundreds of correct candidates already sitting in the index --
//! `execute_planned` never falls back to the structural candidate set
//! when the delegate itself returns nothing. This reads as stronger than
//! `plan/mod.rs`'s own doc comment implies ("the delegate ranks free text
//! only within that narrowed set" suggests a ranking signal, not a hard
//! second filter). See `residual_veto_probe` below, kept as a dedicated,
//! disclosed template (not folded into `same_catalog_control`, which was
//! rewritten to drop the confounding residual word once this was found).

use std::collections::BTreeMap;

use commerce_core::domain::AttributeValue;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::{apparel, automotive, furniture_synth, RelevanceLabel, SynthProduct, SynthQuery};

pub const SEED: u64 = 0x38E3_C0DE;

pub fn generate_mixed_catalog(
    furniture_n: usize,
    apparel_n: usize,
    automotive_n: usize,
) -> Vec<SynthProduct> {
    let mut products = Vec::with_capacity(furniture_n + apparel_n + automotive_n);
    products.extend(furniture_synth::generate_catalog(furniture_n));
    products.extend(apparel::generate_catalog(apparel_n));
    products.extend(automotive::generate_catalog(automotive_n));
    products
}

fn attr_str(p: &SynthProduct, name: &str) -> Option<String> {
    p.product_attributes
        .iter()
        .chain(p.variants[0].variant_attributes.iter())
        .find(|(k, _)| k == name)
        .and_then(|(_, v)| match v {
            AttributeValue::Enum(s) => Some(s.clone()),
            _ => None,
        })
}

fn attr_numeric(p: &SynthProduct, name: &str) -> Option<f64> {
    p.product_attributes
        .iter()
        .chain(p.variants[0].variant_attributes.iter())
        .find(|(k, _)| k == name)
        .and_then(|(_, v)| match v {
            AttributeValue::Numeric(n) => Some(*n),
            _ => None,
        })
}

/// The cross-category-ambiguous workload: queries that, by construction,
/// have real ground-truth relevant products in *more than one* family, so
/// the ground truth itself is the honest test of whether the engine
/// (correctly) treats them as spanning categories rather than (wrongly)
/// picking one family via a hidden bias.
pub fn generate_cross_category_workload(
    products: &[SynthProduct],
) -> (
    Vec<SynthQuery>,
    BTreeMap<String, BTreeMap<String, RelevanceLabel>>,
) {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED.wrapping_add(1));
    let mut queries = Vec::new();
    let mut judgments: BTreeMap<String, BTreeMap<String, RelevanceLabel>> = BTreeMap::new();
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

    // Bare color, no product type at all -- genuinely relevant to every
    // family that has a matching color, no family more "exact" than
    // another (no entity phrase exists in the query to disambiguate).
    for color in ["black", "white"] {
        let text = color.to_string();
        let id = format!("mix-q-barecolor-{qi:03}");
        push_query(
            id,
            text,
            "cross_category_bare_attribute",
            &move |p| match attr_str(p, "color").map(|c| c.to_lowercase()) {
                Some(c) if c == color => RelevanceLabel::Partial,
                _ => RelevanceLabel::Irrelevant,
            },
        );
        qi += 1;
    }

    // "leather black": material + color, both shared field names,
    // "Leather" genuinely present in both furniture (sofas) and apparel
    // (jackets/sneakers) -- no product type given, so both families'
    // matching products are equally Partial.
    {
        let text = "leather black".to_string();
        let id = format!("mix-q-leatherblack-{qi:03}");
        push_query(id, text, "cross_category_bare_attribute", &|p| {
            let m = attr_str(p, "material");
            let c = attr_str(p, "color");
            match (m.as_deref(), c.as_deref()) {
                (Some("Leather"), Some("Black")) => RelevanceLabel::Partial,
                (Some("Leather"), _) | (_, Some("Black")) => RelevanceLabel::Partial,
                _ => RelevanceLabel::Irrelevant,
            }
        });
        qi += 1;
    }

    // The schema-conflict queries themselves: "size N" for an N *derived
    // from a real generated product's own actual size value* in each
    // family (an apparel Jeans product's Enum size, an automotive Wiper
    // Blades product's Numeric size) -- not an arbitrary literal like
    // "size 34", which earlier had no guarantee of ever being generated
    // and was caught failing `ground_truth::assert_self_consistent` at a
    // small catalog size (a real generator bug: a fixed literal has no
    // guaranteed ground truth, unlike every other template in this crate,
    // which derives its query directly from a real generated product --
    // `automotive.rs`'s part-number template is the precedent this
    // follows). Each query's ground truth correctly spans BOTH families
    // whenever a same-valued match exists in the other family too (the
    // judge closure below checks both unconditionally) -- it need not,
    // to demonstrate compile()'s hard-coded numeric "size" branch can
    // never discover the Enum-typed half of a match even when one exists
    // (see this module's own doc comment).
    let jeans_anchor = products
        .iter()
        .find(|p| p.product_type == "Jeans")
        .and_then(|p| attr_str(p, "size"));
    let wiper_anchor = products
        .iter()
        .find(|p| p.product_type == "Wiper Blades")
        .and_then(|p| attr_numeric(p, "size"))
        .map(|n| format!("{n:.0}"));
    for anchor in [jeans_anchor, wiper_anchor].into_iter().flatten() {
        let text = format!("size {anchor}");
        let id = format!("mix-q-sizeconflict-{qi:03}");
        let anchor_for_judge = anchor.clone();
        push_query(id, text, "size_schema_conflict", &move |p| {
            let enum_match = attr_str(p, "size").as_deref() == Some(anchor_for_judge.as_str());
            let numeric_match = anchor_for_judge.parse::<f64>().ok() == attr_numeric(p, "size");
            if enum_match || numeric_match {
                RelevanceLabel::Exact
            } else {
                RelevanceLabel::Irrelevant
            }
        });
        qi += 1;
    }

    // A clean, unambiguous, per-family control query for each family --
    // the recognized product-type phrase *alone*, with no extra residual
    // word -- so the cross-category workload's own coverage/correctness
    // numbers can be compared against a same-catalog, same-mechanism,
    // unambiguous baseline rather than only against E2/E3's separate
    // single-family runs. Deliberately does NOT prepend a family label
    // (e.g. "furniture", "apparel") to the query text: an earlier version
    // of this template did, and direct measurement (not speculation)
    // found it introduced a real confound -- see `residual_veto_probe`
    // below, which keeps that construction on purpose to document what it
    // actually revealed.
    for product_type in ["Sofas", "Sneakers", "Brake Pads"] {
        let text = product_type.to_lowercase();
        let id = format!("mix-q-control-{qi:03}");
        push_query(id, text, "same_catalog_control", &move |p| {
            if p.product_type == product_type {
                RelevanceLabel::Exact
            } else {
                RelevanceLabel::Irrelevant
            }
        });
        qi += 1;
    }

    // Deliberate probe, added after `same_catalog_control`'s original
    // "{family_label} {product_type}" construction was found (by direct
    // measurement, not code reading) to zero out two of its three queries
    // entirely: `execute_planned`'s `Hybrid`/`Punt` outcomes let the
    // lexical delegate's *residual* free-text match veto an otherwise
    // well-formed, correctly-narrowed structural query -- "furniture
    // sofas"/"automotive brake pads" compile to a real
    // `Structural(ProductType(...))` constraint plus a residual term
    // ("furniture"/"automotive") that appears in NO generated title, so
    // the delegate returns zero raw hits and the query returns nothing,
    // even though the structural constraint alone identifies hundreds of
    // correct candidates sitting right there in the index. "apparel
    // sneakers" is the third point and does NOT show this: one of this
    // crate's own apparel brand names, "Cascade Apparel", happens to
    // literally contain the word "apparel", so that query accidentally
    // recovers real hits via an unrelated brand-name coincidence -- kept
    // deliberately (not renamed away) since a brand name literally
    // containing its own category word is a realistic real-catalog
    // pattern, not an artifact, and its presence here is exactly what
    // demonstrates the effect is a real recall veto, not merely "every
    // residual term always fails." Ground truth is identical to
    // `same_catalog_control`'s (defined by product_type alone, independent
    // of the engine's own residual-text quirks) so this template's own
    // NDCG numbers are the direct, measured evidence for the finding
    // documented in this module's own top-level doc comment and in
    // `docs/experiments/ISSUE38_LOG.md`'s E3 section, not merely asserted.
    for (family_label, product_type) in [
        ("furniture", "Sofas"),
        ("apparel", "Sneakers"),
        ("automotive", "Brake Pads"),
    ] {
        let text = format!("{} {}", family_label, product_type.to_lowercase());
        let id = format!("mix-q-residualveto-{qi:03}");
        push_query(id, text, "residual_veto_probe", &move |p| {
            if p.product_type == product_type {
                RelevanceLabel::Exact
            } else {
                RelevanceLabel::Irrelevant
            }
        });
        qi += 1;
    }

    queries.shuffle(&mut rng);
    (queries, judgments)
}
