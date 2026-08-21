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

/// Load and build ONLY the named tenant's catalog -- unlike
/// `partition_depth1`, which materializes all 55 tenants' fully-built
/// `Catalog`s in one `Vec` before any caller can select a subset (a real
/// bug P7-E03's first draft hit: every "single tenant" child process was
/// actually paying the memory cost of building all 55 tenants, since
/// `.into_iter().find()` over an already-fully-built `Vec` doesn't avoid
/// constructing the other 54, it just discards them after the fact).
/// This filters raw records to the one target tenant BEFORE calling
/// `build_catalog`, so only that tenant's data is ever constructed.
/// Callers should also pass a catalog_path that ALREADY contains only
/// this tenant's raw lines (see `write_single_tenant_jsonl` below) --
/// otherwise `data::load_catalog` itself pays the cost of parsing the
/// entire shared multi-tenant file before this filter even runs, a
/// second real confound P7-E03's first draft also hit (every "single
/// tenant" child showed ~37 MB regardless of tenant size, dominated by
/// parsing all 42,994 raw records, not by that one tenant's real data).
pub fn load_single_tenant(catalog_path: &std::path::Path, target_name: &str) -> Catalog {
    let products = data::load_catalog(catalog_path);
    let raw: Vec<_> = products
        .into_iter()
        .filter(|p| p.category_depth_1.as_deref() == Some(target_name))
        .collect();
    assert!(!raw.is_empty(), "tenant {target_name} not found");
    catalog_ingest::build_catalog(&raw).catalog
}

/// Write a temporary JSONL file containing ONLY the named tenant's raw
/// lines from the shared multi-tenant catalog, so a child process
/// pointed at it never pays the cost of parsing every other tenant's
/// data -- the realistic analogue of a real single-tenant deployment
/// that would hold only its own tenant's data file in the first place.
/// `subdir` namespaces the temp files per experiment (e.g.
/// `p7_e03_single_tenant_tmp`) so concurrent experiments never collide.
pub fn write_single_tenant_jsonl(
    catalog_path: &std::path::Path,
    target_name: &str,
    subdir: &str,
) -> std::path::PathBuf {
    let raw_text = std::fs::read_to_string(catalog_path).expect("read catalog.jsonl");
    let mut out = String::new();
    for line in raw_text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("parse catalog line");
        if value.get("category_depth_1").and_then(|v| v.as_str()) == Some(target_name) {
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(!out.is_empty(), "tenant {target_name} not found");
    let dir = std::path::PathBuf::from("dataset_cache").join(subdir);
    std::fs::create_dir_all(&dir).ok();
    let safe_name: String = target_name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let path = dir.join(format!("{safe_name}.jsonl"));
    std::fs::write(&path, out).expect("write single-tenant jsonl");
    path
}
