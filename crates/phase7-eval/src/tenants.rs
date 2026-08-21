//! Phase 7 (Issue #21 Phase 7) tenant model for the first packing-density
//! pass: each real WANDS `category_depth_1` value becomes one
//! independent tenant's full catalog. See `docs/experiments/
//! PHASE7_LOG.md`'s "Tenant model" section for why this is a realistic
//! SMB partition (a specialty single-category retailer) rather than an
//! arbitrary split, and why it inherits a real, non-fabricated
//! heterogeneous size distribution from Phase 6A/6B's own findings.

use std::collections::BTreeMap;

use commerce_core::domain::{AttributeValue, Catalog, Product};
use commerce_core::index::CatalogIndex;
use phase6a_eval::{catalog as catalog_ingest, data};

pub struct Tenant {
    pub name: String,
    pub catalog: Catalog,
    pub index: CatalogIndex,
}

/// Load the real WANDS catalog and partition it by `category_depth_1`
/// into one `(name, Catalog)` pair per distinct real value,
/// largest-catalog-first, WITHOUT building any `CatalogIndex` yet --
/// callers that need to measure incremental per-tenant build cost
/// (e.g. the H1 RSS-amortization curve) must build each tenant's index
/// themselves, one at a time, to get a real incremental measurement
/// rather than one that has already fully materialized every tenant
/// before any RSS snapshot is taken. `limit` caps how many of the
/// largest real tenants to include.
pub fn partition_depth1(catalog_path: &std::path::Path, limit: usize) -> Vec<(String, Catalog)> {
    let products = data::load_catalog(catalog_path);
    let ingested = catalog_ingest::build_catalog(&products);

    let mut by_tenant: BTreeMap<String, Vec<Product>> = BTreeMap::new();
    for product in ingested.catalog.products {
        let depth1 = match product.attributes.get("category_depth_1") {
            Some(AttributeValue::Enum(v)) => v.clone(),
            _ => continue,
        };
        by_tenant.entry(depth1).or_default().push(product);
    }

    let mut tenants: Vec<(String, Vec<Product>)> = by_tenant.into_iter().collect();
    tenants.sort_by_key(|(_, products)| std::cmp::Reverse(products.len()));
    tenants.truncate(limit);

    tenants
        .into_iter()
        .map(|(name, products)| (name, Catalog { products }))
        .collect()
}

/// Convenience: partition and eagerly build every tenant's `CatalogIndex`
/// up front. Use this when the incremental build cost itself is not
/// what's being measured (e.g. the H2 isolation check, which only cares
/// about two already-built tenants' query latency).
pub fn load_depth1_tenants(catalog_path: &std::path::Path, limit: usize) -> Vec<Tenant> {
    partition_depth1(catalog_path, limit)
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
