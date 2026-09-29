//! The E3b `/plp` executor: one function serving every preregistered
//! variant (#79 section 4), so variants differ only in the code path taken,
//! never in build, request parsing or response encoding.
//!
//! `FacetMode::Legacy` + `SortMode::Legacy` is #77's
//! `i77_native_plp_server::handle_plp`, copied verbatim (N0'); the unchanged
//! #77 binary itself is the true N0 and is measured separately.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use commerce_core::domain::{
    effective_attributes, AttributeValue, Catalog, CategoryId, Constraint, NumericOp, ProductId,
};
use commerce_core::index::sort::{
    choose_facet_path, choose_sort_path, top_k_presorted, top_k_presorted_all, top_k_scan,
    Direction, FacetPath, NumericSortColumn, PresenceBitmap, SortOutcome, SortPath,
};
use commerce_core::index::{CandidateSet, CatalogIndex};
use commerce_core::ir::{ResolvedConstraint, StructuralConstraint};
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};

pub const MAX_ROWS: usize = 200;
pub const DEFAULT_ROWS: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacetMode {
    /// N0: `facet_counts` + per-facet candidate recomputation.
    Legacy,
    /// F1: `facet_counts_ordinal` (dense ordinal column scan).
    Ordinal,
    /// F2: `facet_counts_bitmap` (value-bitmap `intersection_len`).
    Bitmap,
    /// F3: per-facet `choose_facet_path` with the calibrated `tau`.
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortMode {
    /// N0: full assembly of every candidate + full stable sort.
    Legacy,
    /// S1: dense value column + bounded top-K heap.
    Topk,
    /// S2: precomputed value order + candidate membership test.
    Presorted,
    /// S3: `choose_sort_path` with the calibrated `rho`.
    Hybrid,
}

/// Issue #63 (amendment 1, section 3 B1 and clarification C1): how a
/// candidate set with no indexable constraint (match-all) is represented.
/// Every non-match-all candidate set is the same explicit bitmap in every
/// mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CandidateMode {
    /// P0 (#79 FINAL, the default): `indexed_candidates`, i.e. per-element
    /// `all_ordinals()` materialization.
    #[default]
    P0,
    /// P0r: materialized with one `insert_range`.
    P0r,
    /// P1: borrow the prebuilt all-ordinals bitmap (server `--prebuilt-all`).
    P1,
    /// P2: logical match-all. Facets over it use the dense paths, chosen by
    /// the frozen tau rule; unsorted paging reads `0..limit`; presorted sort
    /// skips the membership test; candidate top-K borrows the P1 bitmap.
    P2,
    /// P2b: P2, except a hybrid facet over match-all always takes the
    /// value-bitmap `len()` path.
    P2b,
}

impl CandidateMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "p0" => Ok(Self::P0),
            "p0r" => Ok(Self::P0r),
            "p1" => Ok(Self::P1),
            "p2" => Ok(Self::P2),
            "p2b" => Ok(Self::P2b),
            other => Err(format!("unknown cand_mode {other:?}")),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "p0",
            Self::P0r => "p0r",
            Self::P1 => "p1",
            Self::P2 => "p2",
            Self::P2b => "p2b",
        }
    }
}

impl FacetMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "ordinal" => Ok(Self::Ordinal),
            "bitmap" => Ok(Self::Bitmap),
            "hybrid" => Ok(Self::Hybrid),
            other => Err(format!("unknown facet_mode {other:?}")),
        }
    }
}

impl SortMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "topk" => Ok(Self::Topk),
            "presorted" => Ok(Self::Presorted),
            "hybrid" => Ok(Self::Hybrid),
            other => Err(format!("unknown sort_mode {other:?}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlpRequest {
    pub category: Option<String>,
    pub filters: Vec<(String, String)>,
    /// Issue #64 multi-select: `(attribute, values)`, OR within the
    /// attribute, AND with everything else; self-excluded like `filters`.
    pub any_filters: Vec<(String, Vec<String>)>,
    pub ranges: Vec<(String, String, f64)>,
    pub facets: Vec<String>,
    pub sort: Option<(String, bool)>, // (attribute, descending)
    pub top_k: usize,
    pub offset: usize,
    pub facet_mode: FacetMode,
    pub sort_mode: SortMode,
    pub cand_mode: CandidateMode,
}

pub fn percent_decode(value: &str) -> Result<String, String> {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => output.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                let encoded = std::str::from_utf8(&bytes[index + 1..index + 3])
                    .map_err(|error| format!("invalid percent escape: {error}"))?;
                output.push(
                    u8::from_str_radix(encoded, 16)
                        .map_err(|_| format!("invalid percent escape %{encoded}"))?,
                );
                index += 2;
            }
            b'%' => return Err("truncated percent escape".to_string()),
            byte => output.push(byte),
        }
        index += 1;
    }
    String::from_utf8(output).map_err(|error| format!("query is not UTF-8: {error}"))
}

/// Parses `/plp?...` exactly like #77's server, plus `facet_mode`/
/// `sort_mode` (default `legacy`) and Issue #63's `cand_mode` (default
/// `p0`).
pub fn parse_plp_request(target: &str) -> Result<PlpRequest, String> {
    let (path, query_string) = target.split_once('?').unwrap_or((target, ""));
    if path != "/plp" {
        return Err(format!("unsupported path {path}"));
    }
    let mut req = PlpRequest {
        category: None,
        filters: Vec::new(),
        any_filters: Vec::new(),
        ranges: Vec::new(),
        facets: Vec::new(),
        sort: None,
        top_k: DEFAULT_ROWS,
        offset: 0,
        facet_mode: FacetMode::Legacy,
        sort_mode: SortMode::Legacy,
        cand_mode: CandidateMode::P0,
    };
    for pair in query_string.split('&').filter(|p| !p.is_empty()) {
        let (name, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(raw_value)?;
        match name {
            "category" => req.category = Some(value),
            "filter" => {
                let (attr, val) = value
                    .split_once(':')
                    .ok_or_else(|| format!("malformed filter {value:?}, want attr:value"))?;
                req.filters.push((attr.to_owned(), val.to_owned()));
            }
            "anyfilter" => {
                let (attr, vals) = value
                    .split_once(':')
                    .ok_or_else(|| format!("malformed anyfilter {value:?}, want attr:v1|v2"))?;
                let values: Vec<String> = vals.split('|').map(str::to_owned).collect();
                if attr.is_empty() || values.iter().any(String::is_empty) {
                    return Err(format!("malformed anyfilter {value:?}, want attr:v1|v2"));
                }
                req.any_filters.push((attr.to_owned(), values));
            }
            "range" => {
                let mut parts = value.splitn(3, ':');
                let attr = parts.next().ok_or("malformed range")?;
                let op = parts.next().ok_or("malformed range")?;
                let val: f64 = parts
                    .next()
                    .ok_or("malformed range")?
                    .parse()
                    .map_err(|_| "malformed range value".to_string())?;
                req.ranges.push((attr.to_owned(), op.to_owned(), val));
            }
            "facets" => req.facets = value.split(',').map(|s| s.to_owned()).collect(),
            "sort" => {
                let (attr, dir) = value.split_once(':').unwrap_or((value.as_str(), "desc"));
                req.sort = Some((attr.to_owned(), dir != "asc"));
            }
            "topk" => {
                req.top_k = value
                    .parse::<usize>()
                    .map_err(|_| "invalid topk".to_string())?
                    .min(MAX_ROWS);
            }
            "offset" => {
                req.offset = value
                    .parse::<usize>()
                    .map_err(|_| "invalid offset".to_string())?;
            }
            "facet_mode" => req.facet_mode = FacetMode::parse(&value)?,
            "sort_mode" => req.sort_mode = SortMode::parse(&value)?,
            "cand_mode" => req.cand_mode = CandidateMode::parse(&value)?,
            _ => {}
        }
    }
    Ok(req)
}

/// Optional E3b physical structures, built at startup only when requested
/// so their memory/build cost can be measured per launch configuration.
#[derive(Debug, Default)]
pub struct SortStructures {
    pub columns: HashMap<String, NumericSortColumn>,
    pub presence: HashMap<String, PresenceBitmap>,
    /// Issue #63 P1: the prebuilt all-ordinals bitmap (`--prebuilt-all`).
    pub all_ordinals: Option<RoaringBitmap>,
}

impl SortStructures {
    #[must_use]
    pub fn build(index: &CatalogIndex, columns: &[&str], presence: &[&str]) -> Self {
        let mut out = SortStructures::default();
        for field in columns {
            if let Some(column) = NumericSortColumn::build(index, field) {
                out.columns.insert((*field).to_owned(), column);
            }
        }
        for field in presence {
            if let Some(bitmap) = PresenceBitmap::build(index, field) {
                out.presence.insert((*field).to_owned(), bitmap);
            }
        }
        out
    }

    /// Deterministic on-heap estimate of owned bytes, per structure kind.
    #[must_use]
    pub fn owned_bytes(&self) -> (usize, usize) {
        (
            self.columns
                .values()
                .map(NumericSortColumn::owned_bytes)
                .sum(),
            self.presence
                .values()
                .map(PresenceBitmap::owned_bytes)
                .sum(),
        )
    }

    /// Issue #63: serialized size of the prebuilt P1 bitmap (0 if absent).
    #[must_use]
    pub fn all_ordinals_bytes(&self) -> usize {
        self.all_ordinals
            .as_ref()
            .map_or(0, RoaringBitmap::serialized_size)
    }
}

/// Everything `execute` reads. Borrowed, so the server and the correctness
/// gate share one code path.
pub struct PlpContext<'a> {
    pub catalog: &'a Catalog,
    pub index: &'a CatalogIndex,
    pub category_id_by_leaf: &'a HashMap<String, CategoryId>,
    pub source_id_by_product: &'a HashMap<ProductId, String>,
    pub structures: &'a SortStructures,
    /// F3 constant (None until calibrated; `hybrid` then errors).
    pub tau_f: Option<f64>,
    /// S3 constant (None until calibrated; `hybrid` then errors).
    pub rho_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlpDoc {
    pub id: String,
    pub sort_value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FacetDiag {
    pub field: String,
    pub candidates: u64,
    pub cardinality: usize,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Diag {
    pub num_candidates: u64,
    pub candidates_us: f64,
    pub facets_us: f64,
    pub sort_us: f64,
    pub total_us: f64,
    pub facet_diag: Vec<FacetDiag>,
    pub sort_path: String,
    pub ids_inspected: u64,
    /// Issue #63: `cand_mode` and whether the base set was match-all. Both
    /// are omitted for the default P0 path, so #79 FINAL's response is
    /// byte-identical to #79's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cand_mode: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub base_match_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlpResponse {
    pub num_found: usize,
    pub docs: Vec<PlpDoc>,
    pub facets: HashMap<String, BTreeMap<String, u64>>,
    /// Always 1 for native (one in-process call), as in #77.
    pub backend_requests: u32,
    pub diag: Diag,
}

fn numeric_op(op: &str) -> Result<NumericOp, String> {
    match op {
        "eq" => Ok(NumericOp::Eq),
        "lt" => Ok(NumericOp::Lt),
        "lte" => Ok(NumericOp::Lte),
        "gt" => Ok(NumericOp::Gt),
        "gte" => Ok(NumericOp::Gte),
        other => Err(format!("unknown numeric op {other:?}")),
    }
}

/// #77's `build_constraints`, verbatim: optionally excludes one attribute's
/// own filter (the disjunctive-facet mechanism).
fn build_constraints(
    ctx: &PlpContext<'_>,
    req: &PlpRequest,
    exclude_attribute: Option<&str>,
) -> Result<Vec<ResolvedConstraint>, String> {
    let mut constraints = Vec::new();
    if let Some(leaf) = &req.category {
        let id = ctx
            .category_id_by_leaf
            .get(leaf)
            .copied()
            .ok_or_else(|| format!("unknown category_leaf {leaf:?}"))?;
        constraints.push(ResolvedConstraint::Structural(
            StructuralConstraint::Category(id),
        ));
    }
    for (attr, val) in &req.filters {
        if Some(attr.as_str()) == exclude_attribute {
            continue;
        }
        constraints.push(ResolvedConstraint::Attribute(Constraint::Enum {
            attribute: attr.clone(),
            value: val.clone(),
        }));
    }
    for (attr, op, val) in &req.ranges {
        constraints.push(ResolvedConstraint::Attribute(Constraint::Numeric {
            attribute: attr.clone(),
            op: numeric_op(op)?,
            value: *val,
        }));
    }
    Ok(constraints)
}

fn doc_id_for(ctx: &PlpContext<'_>, product_id: ProductId) -> String {
    ctx.source_id_by_product
        .get(&product_id)
        .cloned()
        .unwrap_or_else(|| product_id.0.to_string())
}

fn micros(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1e6
}

/// Executes one `/plp` request under `req.facet_mode` / `req.sort_mode`.
pub fn execute(ctx: &PlpContext<'_>, req: &PlpRequest) -> Result<PlpResponse, String> {
    if req.cand_mode != CandidateMode::P0 {
        return execute_with_candidate_mode(ctx, req);
    }
    let started = Instant::now();
    let base_constraints = build_constraints(ctx, req, None)?;
    let mut candidates = ctx.index.indexed_candidates(&base_constraints);
    apply_any_filters(ctx, req, None, &mut candidates);
    let num_found = candidates.len() as usize;
    let mut diag = Diag {
        num_candidates: candidates.len(),
        candidates_us: micros(started),
        ..Diag::default()
    };

    let facets_started = Instant::now();
    let facets = compute_facets(ctx, req, &candidates, &mut diag)?;
    diag.facets_us = micros(facets_started);

    let sort_started = Instant::now();
    let docs = if req.sort_mode == SortMode::Legacy {
        diag.sort_path = "legacy_full".to_owned();
        diag.ids_inspected = candidates.len();
        legacy_assembly(ctx, req, &candidates)
    } else {
        bounded_assembly(ctx, req, &candidates, &mut diag)?
    };
    diag.sort_us = micros(sort_started);
    diag.total_us = micros(started);

    Ok(PlpResponse {
        num_found,
        docs,
        facets,
        backend_requests: 1,
        diag,
    })
}

fn compute_facets(
    ctx: &PlpContext<'_>,
    req: &PlpRequest,
    candidates: &RoaringBitmap,
    diag: &mut Diag,
) -> Result<HashMap<String, BTreeMap<String, u64>>, String> {
    let mut facets = HashMap::new();
    for facet_attr in &req.facets {
        if req.facet_mode == FacetMode::Legacy {
            // #77 verbatim: recompute candidates for every facet.
            let constraints_without_this_facet =
                build_constraints(ctx, req, Some(facet_attr.as_str()))?;
            let mut candidates_without_this_facet = ctx
                .index
                .indexed_candidates(&constraints_without_this_facet);
            apply_any_filters(
                ctx,
                req,
                Some(facet_attr.as_str()),
                &mut candidates_without_this_facet,
            );
            let counts = ctx
                .index
                .facet_counts(facet_attr, &candidates_without_this_facet);
            diag.facet_diag.push(FacetDiag {
                field: facet_attr.clone(),
                candidates: candidates_without_this_facet.len(),
                cardinality: ctx.index.enum_cardinality(facet_attr),
                path: "legacy".to_owned(),
            });
            facets.insert(facet_attr.clone(), counts);
            continue;
        }
        // Disjunctive self-exclusion only changes the candidate set when this
        // facet has its own active filter; otherwise it is the base set.
        let has_own_filter = has_own_filter(req, facet_attr);
        let own;
        let facet_candidates = if has_own_filter {
            let constraints = build_constraints(ctx, req, Some(facet_attr.as_str()))?;
            let mut bitmap = ctx.index.indexed_candidates(&constraints);
            apply_any_filters(ctx, req, Some(facet_attr.as_str()), &mut bitmap);
            own = bitmap;
            &own
        } else {
            candidates
        };
        let cardinality = ctx.index.enum_cardinality(facet_attr);
        let ordinal_exact = ctx.index.attribute_is_single_valued_enum(facet_attr);
        let path = match req.facet_mode {
            FacetMode::Ordinal if ordinal_exact => FacetPath::OrdinalScan,
            // An attribute the ordinal column cannot answer exactly (absent,
            // or ever MultiEnum) falls back to exact bitmap counting.
            FacetMode::Ordinal | FacetMode::Bitmap => FacetPath::BitmapCount,
            FacetMode::Hybrid => {
                let tau = ctx
                    .tau_f
                    .ok_or("facet_mode=hybrid requested but tau_f is not configured")?;
                choose_facet_path(facet_candidates.len(), cardinality, tau, ordinal_exact)
            }
            FacetMode::Legacy => unreachable!("handled above"),
        };
        let counts = match path {
            FacetPath::OrdinalScan => ctx.index.facet_counts_ordinal(facet_candidates, facet_attr),
            FacetPath::BitmapCount => ctx.index.facet_counts_bitmap(facet_candidates, facet_attr),
        };
        diag.facet_diag.push(FacetDiag {
            field: facet_attr.clone(),
            candidates: facet_candidates.len(),
            cardinality,
            path: match path {
                FacetPath::OrdinalScan => "ordinal",
                FacetPath::BitmapCount => "bitmap",
            }
            .to_owned(),
        });
        facets.insert(facet_attr.clone(), counts);
    }
    Ok(facets)
}

/// #77 verbatim: materialize every candidate (and its sort value via a full
/// `effective_attributes` clone), full stable sort, then page.
fn legacy_assembly(
    ctx: &PlpContext<'_>,
    req: &PlpRequest,
    candidates: &RoaringBitmap,
) -> Vec<PlpDoc> {
    let mut scored: Vec<(ProductId, Option<f64>)> = Vec::new();
    for ord in candidates.iter() {
        let Some(variant_id) = ctx.index.variant_id_at(ord) else {
            continue;
        };
        let Some((product, variant)) = ctx.index.lookup_variant(ctx.catalog, variant_id) else {
            continue;
        };
        let sort_value = req.sort.as_ref().and_then(|(attr, _)| {
            let attrs = effective_attributes(product, variant);
            match attrs.get(attr) {
                Some(AttributeValue::Numeric(v)) => Some(*v),
                _ => None,
            }
        });
        scored.push((product.id, sort_value));
    }
    if let Some((_, descending)) = req.sort {
        scored.sort_by(|a, b| {
            let ordering = a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    scored
        .into_iter()
        .skip(req.offset)
        .take(req.top_k)
        .map(|(product_id, sort_value)| PlpDoc {
            id: doc_id_for(ctx, product_id),
            sort_value,
        })
        .collect()
}

fn ordinal_doc(ctx: &PlpContext<'_>, ordinal: u32, sort_value: Option<f64>) -> Option<PlpDoc> {
    let variant_id = ctx.index.variant_id_at(ordinal)?;
    let (product, _) = ctx.index.lookup_variant(ctx.catalog, variant_id)?;
    Some(PlpDoc {
        id: doc_id_for(ctx, product.id),
        sort_value,
    })
}

/// S-family result path. Unsorted: the first `offset + top_k` candidate
/// ordinals (the same ordinal order #77 returns), nothing else touched.
/// Sorted: S1/S2/S3 bounded top-(offset + top_k).
fn bounded_assembly(
    ctx: &PlpContext<'_>,
    req: &PlpRequest,
    candidates: &RoaringBitmap,
    diag: &mut Diag,
) -> Result<Vec<PlpDoc>, String> {
    let limit = req.offset + req.top_k;
    let Some((field, descending)) = &req.sort else {
        diag.sort_path = "bounded_unsorted".to_owned();
        let docs: Vec<PlpDoc> = candidates
            .iter()
            .take(limit)
            .filter_map(|ord| ordinal_doc(ctx, ord, None))
            .skip(req.offset)
            .collect();
        diag.ids_inspected = (limit as u64).min(candidates.len());
        return Ok(docs);
    };
    let direction = if *descending {
        Direction::Descending
    } else {
        Direction::Ascending
    };
    let path = match req.sort_mode {
        SortMode::Topk => SortPath::CandidateTopK,
        SortMode::Presorted => SortPath::Presorted,
        SortMode::Hybrid => {
            let rho = ctx
                .rho_s
                .ok_or("sort_mode=hybrid requested but rho_s is not configured")?;
            choose_sort_path(candidates.len(), limit, ctx.index.ordinal_count(), rho)
        }
        SortMode::Legacy => unreachable!("handled by caller"),
    };
    let outcome: SortOutcome = match path {
        SortPath::CandidateTopK => {
            let column = ctx.structures.columns.get(field).ok_or_else(|| {
                format!(
                    "sort_mode requires a value column for {field:?} (start with --sort-columns)"
                )
            })?;
            diag.sort_path = "topk".to_owned();
            top_k_scan(candidates, column, direction, limit)
        }
        SortPath::Presorted => {
            let sorted = ctx
                .index
                .numeric_sorted(field)
                .ok_or_else(|| format!("{field:?} is not a numeric attribute"))?;
            let presence = ctx.structures.presence.get(field).ok_or_else(|| {
                format!(
                    "sort_mode requires a presence bitmap for {field:?} (start with --presence)"
                )
            })?;
            diag.sort_path = "presorted".to_owned();
            top_k_presorted(sorted, presence, candidates, direction, limit)
        }
    };
    diag.ids_inspected = outcome.inspected;
    Ok(outcome
        .hits
        .into_iter()
        .skip(req.offset)
        .filter_map(|hit| ordinal_doc(ctx, hit.ordinal, hit.value))
        .collect())
}

/// Issue #63: a candidate set as one of the B1 representations produces it.
enum Candidates<'a> {
    Owned(RoaringBitmap),
    Borrowed(&'a RoaringBitmap),
    All(u32),
}

impl Candidates<'_> {
    fn len(&self) -> u64 {
        match self {
            Self::Owned(bitmap) => bitmap.len(),
            Self::Borrowed(bitmap) => bitmap.len(),
            Self::All(count) => u64::from(*count),
        }
    }

    fn bitmap(&self) -> Option<&RoaringBitmap> {
        match self {
            Self::Owned(bitmap) => Some(bitmap),
            Self::Borrowed(bitmap) => Some(bitmap),
            Self::All(_) => None,
        }
    }

    fn is_all(&self) -> bool {
        matches!(self, Self::All(_))
    }
}

fn prebuilt_all<'a>(ctx: &PlpContext<'a>) -> Result<&'a RoaringBitmap, String> {
    ctx.structures.all_ordinals.as_ref().ok_or_else(|| {
        "cand_mode requires the prebuilt all-ordinals bitmap (start with --prebuilt-all)".to_owned()
    })
}

/// The candidate set for `constraints` under `mode` (never P0, which keeps
/// #79's `indexed_candidates` call verbatim in [`execute`]).
fn candidates_for<'a>(
    ctx: &PlpContext<'a>,
    constraints: &[ResolvedConstraint],
    mode: CandidateMode,
) -> Result<Candidates<'a>, String> {
    Ok(match ctx.index.candidate_set(constraints) {
        CandidateSet::Set(bitmap) => Candidates::Owned(bitmap),
        CandidateSet::All { count } => match mode {
            CandidateMode::P0 => Candidates::Owned(ctx.index.all_ordinals_bitmap()),
            CandidateMode::P0r => Candidates::Owned(ctx.index.all_ordinals_bitmap_by_range()),
            CandidateMode::P1 => Candidates::Borrowed(prebuilt_all(ctx)?),
            CandidateMode::P2 | CandidateMode::P2b => Candidates::All(count),
        },
    })
}

/// [`execute`] for `cand_mode != p0`: the FINAL facet/sort algorithms over
/// the B1 candidate representation. Legacy facet/sort modes are #77's code
/// and only exist over P0, so they are rejected here.
fn execute_with_candidate_mode(
    ctx: &PlpContext<'_>,
    req: &PlpRequest,
) -> Result<PlpResponse, String> {
    if req.facet_mode == FacetMode::Legacy || req.sort_mode == SortMode::Legacy {
        return Err("cand_mode other than p0 requires non-legacy facet_mode and sort_mode".into());
    }
    let started = Instant::now();
    let base_constraints = build_constraints(ctx, req, None)?;
    let candidates = restrict_any(
        ctx,
        req,
        None,
        candidates_for(ctx, &base_constraints, req.cand_mode)?,
    );
    let num_found = candidates.len() as usize;
    let mut diag = Diag {
        num_candidates: candidates.len(),
        candidates_us: micros(started),
        cand_mode: req.cand_mode.as_str().to_owned(),
        base_match_all: candidates.is_all(),
        ..Diag::default()
    };

    let facets_started = Instant::now();
    let mut facets = HashMap::new();
    for facet_attr in &req.facets {
        let has_own_filter = has_own_filter(req, facet_attr);
        let own;
        let facet_candidates = if has_own_filter {
            let constraints = build_constraints(ctx, req, Some(facet_attr.as_str()))?;
            own = restrict_any(
                ctx,
                req,
                Some(facet_attr.as_str()),
                candidates_for(ctx, &constraints, req.cand_mode)?,
            );
            &own
        } else {
            &candidates
        };
        let cardinality = ctx.index.enum_cardinality(facet_attr);
        let ordinal_exact = ctx.index.attribute_is_single_valued_enum(facet_attr);
        let path = match req.facet_mode {
            FacetMode::Ordinal if ordinal_exact => FacetPath::OrdinalScan,
            FacetMode::Ordinal | FacetMode::Bitmap => FacetPath::BitmapCount,
            FacetMode::Hybrid
                if facet_candidates.is_all() && req.cand_mode == CandidateMode::P2b =>
            {
                FacetPath::BitmapCount
            }
            FacetMode::Hybrid => {
                let tau = ctx
                    .tau_f
                    .ok_or("facet_mode=hybrid requested but tau_f is not configured")?;
                choose_facet_path(facet_candidates.len(), cardinality, tau, ordinal_exact)
            }
            FacetMode::Legacy => unreachable!("rejected above"),
        };
        let (counts, path_name) = match (facet_candidates.bitmap(), path) {
            (Some(bitmap), FacetPath::OrdinalScan) => (
                ctx.index.facet_counts_ordinal(bitmap, facet_attr),
                "ordinal",
            ),
            (Some(bitmap), FacetPath::BitmapCount) => {
                (ctx.index.facet_counts_bitmap(bitmap, facet_attr), "bitmap")
            }
            (None, FacetPath::OrdinalScan) => (
                ctx.index.facet_counts_ordinal_all(facet_attr),
                "ordinal_all",
            ),
            (None, FacetPath::BitmapCount) => {
                (ctx.index.facet_counts_bitmap_all(facet_attr), "bitmap_all")
            }
        };
        diag.facet_diag.push(FacetDiag {
            field: facet_attr.clone(),
            candidates: facet_candidates.len(),
            cardinality,
            path: path_name.to_owned(),
        });
        facets.insert(facet_attr.clone(), counts);
    }
    diag.facets_us = micros(facets_started);

    let sort_started = Instant::now();
    let docs = bounded_assembly_with(ctx, req, &candidates, &mut diag)?;
    diag.sort_us = micros(sort_started);
    diag.total_us = micros(started);

    Ok(PlpResponse {
        num_found,
        docs,
        facets,
        backend_requests: 1,
        diag,
    })
}

/// [`bounded_assembly`] over a B1 candidate representation. A match-all
/// set pages `0..limit` unsorted and walks the presorted order without a
/// membership test; candidate top-K has no dense specialization, so it
/// borrows the P1 bitmap.
fn bounded_assembly_with(
    ctx: &PlpContext<'_>,
    req: &PlpRequest,
    candidates: &Candidates<'_>,
    diag: &mut Diag,
) -> Result<Vec<PlpDoc>, String> {
    let Candidates::All(count) = candidates else {
        let bitmap = candidates.bitmap().expect("explicit candidate set");
        return bounded_assembly(ctx, req, bitmap, diag);
    };
    let limit = req.offset + req.top_k;
    let Some((field, descending)) = &req.sort else {
        diag.sort_path = "bounded_unsorted_all".to_owned();
        let docs: Vec<PlpDoc> = (0..*count)
            .take(limit)
            .filter_map(|ord| ordinal_doc(ctx, ord, None))
            .skip(req.offset)
            .collect();
        diag.ids_inspected = (limit as u64).min(u64::from(*count));
        return Ok(docs);
    };
    let direction = if *descending {
        Direction::Descending
    } else {
        Direction::Ascending
    };
    let path = match req.sort_mode {
        SortMode::Topk => SortPath::CandidateTopK,
        SortMode::Presorted => SortPath::Presorted,
        SortMode::Hybrid => {
            let rho = ctx
                .rho_s
                .ok_or("sort_mode=hybrid requested but rho_s is not configured")?;
            choose_sort_path(u64::from(*count), limit, ctx.index.ordinal_count(), rho)
        }
        SortMode::Legacy => unreachable!("rejected by the caller"),
    };
    let outcome: SortOutcome = match path {
        SortPath::CandidateTopK => {
            let bitmap = prebuilt_all(ctx)?;
            return bounded_assembly(ctx, req, bitmap, diag);
        }
        SortPath::Presorted => {
            let sorted = ctx
                .index
                .numeric_sorted(field)
                .ok_or_else(|| format!("{field:?} is not a numeric attribute"))?;
            let presence = ctx.structures.presence.get(field).ok_or_else(|| {
                format!(
                    "sort_mode requires a presence bitmap for {field:?} (start with --presence)"
                )
            })?;
            diag.sort_path = "presorted_all".to_owned();
            top_k_presorted_all(sorted, presence, *count, direction, limit)
        }
    };
    diag.ids_inspected = outcome.inspected;
    Ok(outcome
        .hits
        .into_iter()
        .skip(req.offset)
        .filter_map(|hit| ordinal_doc(ctx, hit.ordinal, hit.value))
        .collect())
}

/// Issue #64: whether `facet_attr` has its own active selection (single- or
/// multi-select), i.e. whether disjunctive self-exclusion applies.
fn has_own_filter(req: &PlpRequest, facet_attr: &str) -> bool {
    req.filters.iter().any(|(attr, _)| attr == facet_attr)
        || req.any_filters.iter().any(|(attr, _)| attr == facet_attr)
}

/// Issue #64 multi-select: intersects `bitmap` with, for every
/// `any_filters` entry except `exclude`, the union of that attribute's value
/// bitmaps (an unknown value contributes nothing).
fn apply_any_filters(
    ctx: &PlpContext<'_>,
    req: &PlpRequest,
    exclude: Option<&str>,
    bitmap: &mut RoaringBitmap,
) {
    for (attr, values) in &req.any_filters {
        if Some(attr.as_str()) == exclude {
            continue;
        }
        let mut union = RoaringBitmap::new();
        for value in values {
            if let Some(b) = ctx.index.enum_value_bitmap(attr, value) {
                union |= b;
            }
        }
        *bitmap &= union;
    }
}

/// [`apply_any_filters`] over a B1 candidate representation: a set
/// restricted by a multi-select is explicit, never match-all.
fn restrict_any<'a>(
    ctx: &PlpContext<'a>,
    req: &PlpRequest,
    exclude: Option<&str>,
    candidates: Candidates<'a>,
) -> Candidates<'a> {
    if !req
        .any_filters
        .iter()
        .any(|(attr, _)| Some(attr.as_str()) != exclude)
    {
        return candidates;
    }
    let mut bitmap = match candidates {
        Candidates::Owned(b) => b,
        Candidates::Borrowed(b) => b.clone(),
        Candidates::All(_) => ctx.index.all_ordinals_bitmap_by_range(),
    };
    apply_any_filters(ctx, req, exclude, &mut bitmap);
    Candidates::Owned(bitmap)
}
