//! Issue #55 H3: does `commerce_core`'s variant-scoped-conjunction
//! correctness guarantee (`docs/decisions/...`, `commerce_core`'s hard
//! rule "Product/Variant correctness is non-negotiable") hold on a real
//! catalog with genuine Product/Variant structure? `crates/commerce-core/tests/variant_safety.rs`
//! already proves this on a 1-product, 2-variant synthetic fixture; H3
//! itself and `ISSUE47_DECISION.md`'s own "external validity... NOT
//! ESTABLISHED" note both flag that this has never been tested against
//! real data, since neither WANDS nor ESCI has real variant structure.
//!
//! This binary ingests `dataset_cache/magento_configurable/catalog.jsonl`
//! (real Magento apparel configurable-product data, checkerboard-
//! sparsified per `scripts/datasets/prepare_magento_configurable.py`'s own
//! disclosed methodology so real cross-variant trap opportunities exist),
//! builds the real `commerce_core::index::CatalogIndex` production
//! serving structure from it, and for every product, exhaustively queries
//! every (color, size) combination drawn from that product's own real
//! color/size vocabulary -- both combinations that are a real kept
//! variant (true positives) and combinations that were sparsified away
//! (traps: color exists on some variant, size exists on some *other*
//! variant, but no single variant has both). Ground truth for each query
//! is computed directly from the parsed JSONL, independent of both
//! `Catalog::search` (the naive per-variant reference) and
//! `plan::execute_planned` (the production FastPath route through
//! `CatalogIndex`) -- so this is a real, non-circular oracle comparison
//! against two independent implementations.
//!
//! A query with a resolved `color` + `size` Enum conjunction and no
//! free-text residual always routes to `ExecutionOutcome::FastPath`
//! (`plan::plan`'s own first check: empty `residual_lexical` ->
//! `FastPath`), which calls `CatalogIndex::execute_ranked` -- the actual
//! compiled bitmap/ordinal execution path real queries use, not a
//! shortcut.

use std::collections::BTreeMap;
use std::fs;

use commerce_core::domain::{
    attributes, AttributeMap, AttributeValue, Brand, BrandId, Catalog, Category, CategoryId,
    Constraint, Inventory, Price, Product, ProductId, ProductType, ProductTypeId, Variant,
    VariantId,
};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint};
use commerce_core::plan::{execute_planned, ExecutionOutcome, PlannerPolicy};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RawVariant {
    color: String,
    size: String,
}

#[derive(Debug, Deserialize)]
struct RawProduct {
    sku: String,
    name: String,
    price_cents: i64,
    category_top: String,
    material: String,
    colors: Vec<String>,
    sizes: Vec<String>,
    variants: Vec<RawVariant>,
}

fn load_raw(path: &str) -> Vec<RawProduct> {
    let content = fs::read_to_string(path).expect("read catalog.jsonl");
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("parse product json"))
        .collect()
}

/// Build the real `commerce_core::domain::Catalog` from the parsed real
/// (sparsified) data. One `ProductTypeId`/`CategoryId`/`BrandId` per
/// distinct `category_top` value (`Men`/`Women`) -- a coarse but real
/// grouping present in the source data, not invented.
fn build_catalog(raw: &[RawProduct]) -> (Catalog, Vec<(Brand, ProductType, Category)>) {
    let mut category_ids: BTreeMap<String, u32> = BTreeMap::new();
    let mut next_cat = 1u32;
    let mut products = Vec::new();

    for (p_idx, rp) in raw.iter().enumerate() {
        let cat_id = *category_ids
            .entry(rp.category_top.clone())
            .or_insert_with(|| {
                let id = next_cat;
                next_cat += 1;
                id
            });
        let product_attrs: AttributeMap = attributes([
            ("material", AttributeValue::Text(rp.material.clone())),
            ("sku", AttributeValue::Text(rp.sku.clone())),
        ]);
        let variants: Vec<Variant> = rp
            .variants
            .iter()
            .enumerate()
            .map(|(v_idx, rv)| {
                let mut attrs: AttributeMap = AttributeMap::new();
                attrs.insert("color".to_string(), AttributeValue::Enum(rv.color.clone()));
                attrs.insert("size".to_string(), AttributeValue::Enum(rv.size.clone()));
                Variant {
                    id: VariantId((p_idx as u64) * 1000 + v_idx as u64),
                    attributes: attrs,
                    price: Price::usd(rp.price_cents),
                    inventory: Inventory::in_stock(1),
                }
            })
            .collect();
        products.push(Product {
            id: ProductId(p_idx as u64),
            product_type: ProductTypeId(cat_id),
            brand: BrandId(1),
            category: CategoryId(cat_id),
            title: rp.name.clone(),
            attributes: product_attrs,
            variants,
        });
    }

    let meta = category_ids
        .into_iter()
        .map(|(name, id)| {
            (
                Brand {
                    id: BrandId(1),
                    name: "Magento Sample Data".to_string(),
                },
                ProductType {
                    id: ProductTypeId(id),
                    name: name.clone(),
                },
                Category {
                    id: CategoryId(id),
                    name,
                },
            )
        })
        .collect();

    (Catalog { products }, meta)
}

fn color_size_query(color: &str, size: &str) -> CommerceQuery {
    CommerceQuery {
        constraints: vec![
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "color".to_string(),
                value: color.to_string(),
            }),
            ResolvedConstraint::Attribute(Constraint::Enum {
                attribute: "size".to_string(),
                value: size.to_string(),
            }),
        ],
        preferences: vec![],
        ambiguous: vec![],
        residual_lexical: vec![],
    }
}

fn main() {
    let raw = load_raw("dataset_cache/magento_configurable/catalog.jsonl");
    let total_kept_variants: usize = raw.iter().map(|p| p.variants.len()).sum();
    let total_full_cartesian: usize = raw.iter().map(|p| p.colors.len() * p.sizes.len()).sum();
    println!(
        "loaded {} parent products, {} kept (sparsified) variants of {} full-cartesian combinations",
        raw.len(),
        total_kept_variants,
        total_full_cartesian
    );

    let (catalog, _meta) = build_catalog(&raw);
    let index = CatalogIndex::build(&catalog);
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };
    let k = total_kept_variants + 10; // no truncation: every real hit must surface

    let mut total_queries = 0usize;
    let mut true_positive_queries = 0usize;
    let mut trap_queries = 0usize;
    let mut fastpath_count = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for (p_idx, rp) in raw.iter().enumerate() {
        let product_id = ProductId(p_idx as u64);
        // Ground truth: which of THIS product's kept variants have exactly
        // this (color, size) pair -- computed directly from the parsed
        // JSONL, not from any commerce_core API.
        for size in &rp.sizes {
            for color in &rp.colors {
                total_queries += 1;
                let mut ground_truth: Vec<(ProductId, VariantId)> = Vec::new();
                for (v_idx, rv) in rp.variants.iter().enumerate() {
                    if &rv.color == color && &rv.size == size {
                        ground_truth
                            .push((product_id, VariantId((p_idx as u64) * 1000 + v_idx as u64)));
                    }
                }
                // Also check: does this (color,size) combo appear as a real
                // kept variant on ANY OTHER product? (legitimate cross-
                // product true positives the query should also surface.)
                for (other_idx, other) in raw.iter().enumerate() {
                    if other_idx == p_idx {
                        continue;
                    }
                    for (v_idx, rv) in other.variants.iter().enumerate() {
                        if &rv.color == color && &rv.size == size {
                            ground_truth.push((
                                ProductId(other_idx as u64),
                                VariantId((other_idx as u64) * 1000 + v_idx as u64),
                            ));
                        }
                    }
                }
                ground_truth.sort();

                if rp
                    .variants
                    .iter()
                    .any(|v| &v.color == color && &v.size == size)
                {
                    true_positive_queries += 1;
                } else {
                    trap_queries += 1;
                }

                let query = color_size_query(color, size);
                let mut reference_hits = catalog.search(&query_to_constraints(&query));
                reference_hits.sort();

                let (planned, planned_hits) =
                    execute_planned(&query, &catalog, &index, None, k, &policy, None);
                if planned.outcome == ExecutionOutcome::FastPath {
                    fastpath_count += 1;
                } else {
                    mismatches.push(format!(
                        "UNEXPECTED ROUTING: product={} color={} size={} routed to {:?}, expected FastPath (pure structural conjunction, no residual)",
                        rp.sku, color, size, planned.outcome
                    ));
                }
                let mut production_hits: Vec<(ProductId, VariantId)> = planned_hits
                    .iter()
                    .map(|h| (h.product, h.variant))
                    .collect();
                production_hits.sort();

                if reference_hits != ground_truth {
                    mismatches.push(format!(
                        "Catalog::search MISMATCH: product={} color={} size={} ground_truth={:?} reference={:?}",
                        rp.sku, color, size, ground_truth, reference_hits
                    ));
                }
                if production_hits != ground_truth {
                    mismatches.push(format!(
                        "execute_planned MISMATCH: product={} color={} size={} ground_truth={:?} production={:?}",
                        rp.sku, color, size, ground_truth, production_hits
                    ));
                }
            }
        }
    }

    println!("total exhaustive (color,size) queries: {total_queries}");
    println!("  true-positive queries (combo is a real kept variant of that product): {true_positive_queries}");
    println!("  trap queries (color and size each real, but never co-occur on one variant of that product): {trap_queries}");
    println!("queries routed to FastPath: {fastpath_count} / {total_queries}");
    println!("mismatches found: {}", mismatches.len());
    for m in &mismatches {
        println!("  MISMATCH: {m}");
    }
    if mismatches.is_empty() {
        println!(
            "=== VERDICT: CONFIRMED -- every one of {total_queries} exhaustive real-data \
            variant-scoped conjunction queries ({trap_queries} of them genuine cross-variant \
            traps) matched the independently-computed ground truth exactly, on both \
            Catalog::search and the production execute_planned/CatalogIndex path ==="
        );
    } else {
        println!(
            "=== VERDICT: FALSIFIED -- {} of {total_queries} queries mismatched ground truth; \
            see mismatches above ===",
            mismatches.len()
        );
        std::process::exit(1);
    }
}

fn query_to_constraints(query: &CommerceQuery) -> Vec<Constraint> {
    query
        .constraints
        .iter()
        .filter_map(|c| match c {
            ResolvedConstraint::Attribute(constraint) => Some(constraint.clone()),
            ResolvedConstraint::Structural(_) => None,
        })
        .collect()
}
