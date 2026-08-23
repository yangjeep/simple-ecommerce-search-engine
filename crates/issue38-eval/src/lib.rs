//! Issue #38 E1: architectural falsification gate.
//!
//! The primary research question (Issue #38): can a dynamically discovered
//! merchant feature model compile into a serving artifact whose hot-path
//! cost remains close to a hand-specialized native implementation, and
//! materially below a generic-document-search baseline? E1 isolates this
//! to one concrete, already-real question in this codebase:
//! `commerce_core::index::CatalogIndex` already draws exactly this line
//! for two different field kinds --
//! `product_type`/`brand`/`category` are hard-coded Rust struct fields on
//! [`commerce_core::domain::Product`], each backed by a dedicated
//! `HashMap<TypedId, RoaringBitmap>` (path A); arbitrary catalog attributes
//! (color, material, ...) are looked up by *string* name through a generic
//! `HashMap<(String, String), RoaringBitmap>` (`enum_bitmaps`), the same
//! physical primitive (`RoaringBitmap` intersection) reached through a
//! schema-flexible, string-keyed path instead of a typed one. This module
//! reuses that exact generic mechanism -- not a new invention -- as path
//! B's "compiled schema" representation: the *same catalog field*
//! (`product_type`) is compiled into it via [`CompiledEnumIndex`],
//! discovered the same catalog-agnostic way
//! `commerce_core::cold_start::profile` already discovers real attribute
//! vocabulary (no hand-authored per-vertical mapping).
//!
//! Path C is a deliberate strawman this project explicitly does NOT want
//! to build for production (`CLAUDE.md`: "do not solve schema flexibility
//! by recreating Solr/Elasticsearch abstractions in Rust") -- a
//! runtime-generic document store ([`GenericStore`]) with no precomputed
//! index at all: every query is a full linear scan with dynamic
//! type-dispatch per document. It exists only to quantify, once, the tax
//! of true runtime genericity (no compilation, dispatch AND
//! representation both deferred to query time), so E1's B-vs-A question
//! (does compiling flexibility away preserve the physical advantage?) has
//! a concrete, measured contrast against "not compiling it away at all."
//!
//! Path D reuses the same fresh, same-run Apache Solr baseline established
//! in Phase 9 (`wands_bench` core), queried via `fq=product_class:"..."`
//! (WANDS's raw column name, preserved verbatim in the Solr schema).

use std::collections::{BTreeMap, HashMap};

use commerce_core::domain::{Catalog, ProductId, VariantId};
use roaring::RoaringBitmap;

/// A dynamically-typed field value, for path C's deliberately generic
/// document representation. Only the variants this experiment's workload
/// needs (an equality filter on a string-valued field); a real generic
/// document store would need more, which is precisely the point -- every
/// additional type/operator is more runtime dispatch, not less.
#[derive(Debug, Clone, PartialEq)]
pub enum GenericValue {
    Str(String),
}

/// Path C: the "accidental Rust-Solr" this project's architecture
/// deliberately avoids building for real. One flat, per-document field map
/// with no precomputed index of any kind -- every query is a full linear
/// scan, and every per-document field access dispatches on `GenericValue`'s
/// runtime type before comparing.
#[derive(Debug, Default)]
pub struct GenericStore {
    pub docs: Vec<(ProductId, VariantId, BTreeMap<String, GenericValue>)>,
}

impl GenericStore {
    /// Builds the fully generic representation directly from a real
    /// [`Catalog`] snapshot, `extract`ing whatever fields the caller wants
    /// represented genuinely dynamically (e.g. `product_type`) rather than
    /// through `Product`'s hard-coded struct fields.
    pub fn build(
        catalog: &Catalog,
        extract: impl Fn(&commerce_core::domain::Product) -> Vec<(String, GenericValue)>,
    ) -> Self {
        let mut docs = Vec::new();
        for product in &catalog.products {
            let fields: BTreeMap<String, GenericValue> = extract(product).into_iter().collect();
            for variant in &product.variants {
                docs.push((product.id, variant.id, fields.clone()));
            }
        }
        GenericStore { docs }
    }

    /// Runtime-generic equality query: for every document, dispatch on the
    /// stored value's type, then compare. No bitmap, no dictionary, no
    /// precomputed anything -- the cost this experiment isolates.
    pub fn query_eq(&self, field: &str, want: &GenericValue) -> Vec<(ProductId, VariantId)> {
        let mut hits = Vec::new();
        for (pid, vid, fields) in &self.docs {
            if let Some(have) = fields.get(field) {
                let matches = match (have, want) {
                    (GenericValue::Str(a), GenericValue::Str(b)) => a == b,
                };
                if matches {
                    hits.push((*pid, *vid));
                }
            }
        }
        hits
    }

    pub fn approximate_size_bytes(&self) -> usize {
        self.docs
            .iter()
            .map(|(_, _, fields)| {
                std::mem::size_of::<(ProductId, VariantId)>()
                    + fields
                        .iter()
                        .map(|(k, v)| {
                            k.len()
                                + match v {
                                    GenericValue::Str(s) => s.len(),
                                }
                        })
                        .sum::<usize>()
            })
            .sum()
    }
}

/// Path B: a dynamically-discovered feature compiled offline into the
/// *same physical primitive* `commerce_core::index::CatalogIndex` already
/// uses for its own generic `enum_bitmaps` attribute path -- a
/// `(field, value) -> RoaringBitmap` map, keyed by owned `String`s exactly
/// the way that production code path is (including its one real, already-
/// disclosed inefficiency: a per-query key clone/allocation, since
/// `HashMap::get` needs an owned `(String, String)` to match production's
/// exact behavior rather than a friendlier variant invented for this
/// experiment). This is not a new design -- it is the literal mechanism
/// `cold_start::profile::compile_lexicon` + `CatalogIndex::attribute_bitmap`
/// already provide for arbitrary catalog attributes, applied here to the
/// one field (`product_type`) path A represents with a dedicated typed
/// bitmap instead, so the two can be measured against each other on
/// identical data.
#[derive(Debug, Default)]
pub struct CompiledEnumIndex {
    bitmaps: HashMap<(String, String), RoaringBitmap>,
    ordinals: Vec<(ProductId, VariantId)>,
}

impl CompiledEnumIndex {
    /// The offline compilation step: one pass over the catalog, assigning
    /// dense ordinals identically to `CatalogIndex::build`'s own encounter
    /// order (so a size/behavior comparison against path A is apples to
    /// apples), and populating the generic bitmap map via `extract` --
    /// the same "discover it, then compile it into a bitmap" step
    /// `compile_lexicon` already performs for real attribute vocabulary.
    pub fn compile(
        catalog: &Catalog,
        field: &str,
        extract: impl Fn(&commerce_core::domain::Product) -> String,
    ) -> Self {
        let mut idx = CompiledEnumIndex::default();
        for product in &catalog.products {
            let value = extract(product);
            for variant in &product.variants {
                let ord = idx.ordinals.len() as u32;
                idx.ordinals.push((product.id, variant.id));
                idx.bitmaps
                    .entry((field.to_string(), value.clone()))
                    .or_default()
                    .insert(ord);
            }
        }
        idx
    }

    /// Mirrors `CatalogIndex::attribute_bitmap`'s exact lookup shape for
    /// `Constraint::Enum` (see `crates/commerce-core/src/index/mod.rs`):
    /// callers pass borrowed field/value strings the way a compiled query
    /// constraint does, and this allocates a fresh tuple key for the
    /// lookup on every call -- production's real behavior, not an
    /// idealized one.
    pub fn query_eq(&self, field: &str, value: &str) -> RoaringBitmap {
        self.bitmaps
            .get(&(field.to_string(), value.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    pub fn candidate_product_variant_ids(
        &self,
        bitmap: &RoaringBitmap,
    ) -> Vec<(ProductId, VariantId)> {
        bitmap
            .iter()
            .map(|ord| self.ordinals[ord as usize])
            .collect()
    }

    pub fn approximate_size_bytes(&self) -> usize {
        self.bitmaps
            .iter()
            .map(|((f, v), bm)| f.len() + v.len() + bm.serialized_size())
            .sum::<usize>()
            + self.ordinals.len() * std::mem::size_of::<(ProductId, VariantId)>()
    }
}

/// Path B2: the same compiled-schema idea as [`CompiledEnumIndex`], but
/// with the one real, avoidable inefficiency E1's own first measurement
/// found in it removed -- a per-query `(String, String)` tuple-key
/// allocation, costing exactly 2 extra heap allocations per query versus
/// path A's integer-keyed lookup (confirmed directly: A allocates 2/query,
/// B allocates 4/query, for the identical query). This is not a new
/// invention either: it is `commerce_core::index::CatalogIndex`'s own
/// Issue #21 Phase 6D precedent (`enum_dictionary`/`enum_columns`, and the
/// dedicated `brand_dictionary`/`category_dictionary`/
/// `product_type_dictionary`), which that phase built for exactly this
/// reason -- a per-candidate `String` hash/clone was already found to cost
/// real, measurable time there too. One dedicated `HashMap<String,
/// RoaringBitmap>` *per compiled field* (rather than one shared map keyed
/// by `(field, value)`) needs only a borrowed `&str` to look up -- no
/// allocation at all -- while still being reached through a schema
/// discovered at compile time, not a hard-coded Rust struct field. If this
/// closes most of B's overhead against A, the original inefficiency was
/// implementation-specific, not a fundamental cost of "compiled but
/// schema-flexible"; Issue #38 explicitly asks for exactly this
/// redesign-and-retest before concluding the architecture itself fails E1.
#[derive(Debug, Default)]
pub struct CompiledOrdinalIndex {
    bitmaps: HashMap<String, RoaringBitmap>,
    ordinals: Vec<(ProductId, VariantId)>,
}

impl CompiledOrdinalIndex {
    pub fn compile(
        catalog: &Catalog,
        extract: impl Fn(&commerce_core::domain::Product) -> String,
    ) -> Self {
        let mut idx = CompiledOrdinalIndex::default();
        for product in &catalog.products {
            let value = extract(product);
            for variant in &product.variants {
                let ord = idx.ordinals.len() as u32;
                idx.ordinals.push((product.id, variant.id));
                idx.bitmaps.entry(value.clone()).or_default().insert(ord);
            }
        }
        idx
    }

    /// No allocation: `HashMap<String, _>::get` accepts a borrowed `&str`
    /// directly (via `Borrow<str>`), unlike `CompiledEnumIndex::query_eq`'s
    /// tuple key, which cannot borrow a `(String, String)` from two `&str`s.
    pub fn query_eq(&self, value: &str) -> RoaringBitmap {
        self.bitmaps.get(value).cloned().unwrap_or_default()
    }

    pub fn candidate_product_variant_ids(
        &self,
        bitmap: &RoaringBitmap,
    ) -> Vec<(ProductId, VariantId)> {
        bitmap
            .iter()
            .map(|ord| self.ordinals[ord as usize])
            .collect()
    }

    pub fn approximate_size_bytes(&self) -> usize {
        self.bitmaps
            .iter()
            .map(|(v, bm)| v.len() + bm.serialized_size())
            .sum::<usize>()
            + self.ordinals.len() * std::mem::size_of::<(ProductId, VariantId)>()
    }
}

/// A tiny counting global allocator, wrapping the system allocator, so
/// binaries in this crate can measure real allocation *count* and *bytes*
/// per query path -- this environment has no `perf`/`perf_event_open`
/// access (checked directly: no `perf` binary, `/proc/sys/kernel/perf_event_paranoid`
/// unreadable), so cycles/branch-misses/cache-misses (Issue #38's "where
/// practical" metrics) are not measurable here and are disclosed as such
/// rather than fabricated; allocation counting is a real, measurable
/// proxy this environment does support.
pub mod counting_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
    pub static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);

    pub struct CountingAllocator;

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            System.alloc(layout)
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout)
        }
    }

    /// Snapshot-and-diff helper: reset the counters, run `f`, return
    /// `(alloc_count, alloc_bytes)` attributable to `f` alone. Not
    /// thread-safe against concurrent allocation from other threads --
    /// every binary in this crate runs its measured sections
    /// single-threaded for exactly this reason.
    pub fn count_allocs<T>(f: impl FnOnce() -> T) -> (usize, usize, T) {
        ALLOC_COUNT.store(0, Ordering::Relaxed);
        ALLOC_BYTES.store(0, Ordering::Relaxed);
        let result = f();
        (
            ALLOC_COUNT.load(Ordering::Relaxed),
            ALLOC_BYTES.load(Ordering::Relaxed),
            result,
        )
    }
}

/// Current process resident set size in KB, read from `/proc/self/status`
/// -- the same mechanism `phase7_eval::resident::current_rss_kb` already
/// established (Issue #21 Phase 7), reimplemented here rather than taking
/// a cross-crate dependency on an eval crate for one function.
pub fn current_rss_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            if let Some(kb) = rest.split_whitespace().next() {
                return kb.parse().unwrap_or(0);
            }
        }
    }
    0
}
