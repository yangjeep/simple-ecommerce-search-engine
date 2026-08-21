//! Phase 7 (Issue #21 Phase 7) tenant model for the first packing-density
//! pass: each real WANDS `category_depth_1` value becomes one
//! independent tenant's full catalog. See `docs/experiments/
//! PHASE7_LOG.md`'s "Tenant model" section for why this is a realistic
//! SMB partition (a specialty single-category retailer) rather than an
//! arbitrary split, and why it inherits a real, non-fabricated
//! heterogeneous size distribution from Phase 6A/6B's own findings.
//!
//! **Independent per-tenant schemas** (fixed after adversarial review):
//! raw WANDS records are grouped by `category_depth_1` BEFORE calling
//! `catalog_ingest::build_catalog`, so each tenant gets its own
//! independently-interned `CategoryId`/`ProductTypeId`/`BrandId` space,
//! rather than all tenants sharing one canonicalized ID space from a
//! single whole-catalog ingestion pass. This matches how real,
//! independent SaaS tenants would each bootstrap their own schema.

use std::collections::BTreeMap;

use commerce_core::domain::Catalog;
use commerce_core::index::CatalogIndex;
use phase6a_eval::{catalog as catalog_ingest, data};

pub struct Tenant {
    pub name: String,
    pub catalog: Catalog,
    pub index: CatalogIndex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// Largest real tenant first (the ladder's default order).
    LargestFirst,
    /// Smallest real tenant first -- an order-confound control: if a
    /// claimed per-tenant fixed cost is real (not an artifact of always
    /// building the biggest catalog first and inheriting its allocator
    /// warm-up), it should look similar whichever end of the size
    /// distribution is built first.
    SmallestFirst,
}

/// Partition the real WANDS catalog by `category_depth_1` into one
/// `(name, Catalog)` pair per distinct real value, WITHOUT building any
/// `CatalogIndex` yet -- callers that need to measure incremental
/// per-tenant build cost (e.g. the H1 RSS-amortization curve) must build
/// each tenant's index themselves, one at a time, to get a real
/// incremental measurement rather than one that has already fully
/// materialized every tenant before any snapshot is taken. `limit` caps
/// how many real tenants to include, ordered by `order`.
pub fn partition_depth1(
    catalog_path: &std::path::Path,
    limit: usize,
    order: Order,
) -> Vec<(String, Catalog)> {
    let products = data::load_catalog(catalog_path);

    // Group RAW records first (before build_catalog), so each tenant's
    // eventual build_catalog call gets an independent ID-interning space.
    let mut by_tenant: BTreeMap<String, Vec<data::WandsProduct>> = BTreeMap::new();
    for product in products {
        let Some(depth1) = product.category_depth_1.clone() else {
            continue;
        };
        by_tenant.entry(depth1).or_default().push(product);
    }

    let mut tenants: Vec<(String, Vec<data::WandsProduct>)> = by_tenant.into_iter().collect();
    match order {
        Order::LargestFirst => tenants.sort_by_key(|(_, p)| std::cmp::Reverse(p.len())),
        Order::SmallestFirst => tenants.sort_by_key(|(_, p)| p.len()),
    }
    tenants.truncate(limit);

    tenants
        .into_iter()
        .map(|(name, raw_products)| {
            let ingested = catalog_ingest::build_catalog(&raw_products);
            (name, ingested.catalog)
        })
        .collect()
}

/// Convenience: partition (largest-first) and eagerly build every
/// tenant's `CatalogIndex` up front. Use this when the incremental
/// build cost itself is not what's being measured (e.g. the H2
/// isolation check, which only cares about two already-built tenants'
/// query latency).
pub fn load_depth1_tenants(catalog_path: &std::path::Path, limit: usize) -> Vec<Tenant> {
    partition_depth1(catalog_path, limit, Order::LargestFirst)
        .into_iter()
        .map(|(name, catalog)| {
            let index = CatalogIndex::build(&catalog);
            Tenant {
                name,
                catalog,
                index,
            }
        })
        .collect()
}
