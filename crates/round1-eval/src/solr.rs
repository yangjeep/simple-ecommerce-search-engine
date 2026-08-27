//! Shared Solr query-construction/HTTP helpers, factored out of Phase 2's
//! `phase2-eval::p1d_physical_advantage_eval` (P2-E13/P2-E16,
//! `docs/experiments/PHASE2_LOG.md`) so later phases reuse the *same*,
//! already-adversarially-reviewed query construction rather than
//! re-deriving (and potentially re-breaking) it. Two real bugs were found
//! and fixed in this exact code during Phase 2 -- an unpopulated
//! `brand_lower` field (P2-E13) and a latency measurement that silently
//! bypassed this construction entirely (P2-E16) -- so this module is the
//! validated, fair baseline construction, not a fresh implementation to
//! re-audit from scratch.

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Builds a Lucene `RegexpQuery` pattern that matches `s` case-
/// insensitively, anchored to the *entire* field value (Lucene's regex
/// queries on a `StrField`/`strings` field are implicitly whole-term-
/// anchored, confirmed directly against a live Solr instance:
/// `brand:/[Nn][Ii][Kk][Ee]/` returns exactly the ASCII-case variants of
/// "Nike," not substring matches like "Nike Inc").
///
/// **Why this exists** (P2-E13): Solr's real `brand`/`color` fields hold
/// the *raw*, per-record casing (`solr.StrField`, no case-folding
/// analyzer) -- a regex built from the already-lowercased
/// `commerce_core::domain::Brand::name` reproduces `commerce_core`'s own
/// case-insensitive brand-identity grouping exactly, which neither an
/// unpopulated lowercased copy-field nor a single exact-case
/// `brand:"Nike"` query would (the latter misses real "NIKE"/"nike"
/// casing variants `commerce_core` correctly treats as the same brand).
///
/// Issue #55 A3: this is now a re-export of
/// `comparator_eval::solr::case_insensitive_field_regex`, the same
/// implementation moved to the new centralized comparator crate, rather
/// than a second copy maintained here. Kept as a `pub use` (not removed)
/// so every existing `round1_eval::solr::case_insensitive_field_regex`
/// call site in this workspace keeps compiling unchanged.
pub use comparator_eval::solr::case_insensitive_field_regex;

pub struct SolrResult {
    /// Server-side-only `QTime` from `responseHeader` (ms) -- excludes
    /// HTTP/JSON/network overhead. A caller measuring full round-trip
    /// latency should time its own `solr_search` call with `Instant::now()`
    /// rather than rely on this field for that purpose.
    pub qtime_ms: f64,
    pub num_found: usize,
    pub ids: Vec<String>,
}

pub fn solr_search(base_url: &str, q: &str, fq: &[String], rows: usize) -> Option<SolrResult> {
    let mut url = format!(
        "{base_url}/select?q={}&rows={rows}&fl=id",
        percent_encode(q)
    );
    for f in fq {
        url.push_str(&format!("&fq={}", percent_encode(f)));
    }
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .ok()?;
    let body: serde_json::Value = resp.into_json().ok()?;
    let qtime_ms = body["responseHeader"]["QTime"].as_f64().unwrap_or(0.0);
    let num_found = body["response"]["numFound"].as_u64().unwrap_or(0) as usize;
    let ids: Vec<String> = body["response"]["docs"]
        .as_array()?
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_string))
        .collect();
    Some(SolrResult {
        qtime_ms,
        num_found,
        ids,
    })
}

/// Builds `(q, fq)` for a query, matching the *shape* a real commerce Solr
/// deployment would use: structural signal (brand/color, both via a
/// case-insensitive whole-field regex against Solr's raw `brand`/`color`
/// fields) goes to `fq` (filter query, matching commerce-native's hard-
/// constraint semantics), free text goes to `q` via `edismax` over
/// `all_text`.
pub fn solr_query_for(
    query_text: &str,
    residual_lexical: &[String],
    brand: Option<&str>,
    color: Option<&str>,
) -> (String, Vec<String>) {
    let mut fq = Vec::new();
    if let Some(b) = brand {
        fq.push(format!("brand:/{}/", case_insensitive_field_regex(b)));
    }
    if let Some(c) = color {
        fq.push(format!("color:/{}/", case_insensitive_field_regex(c)));
    }
    let text = if residual_lexical.is_empty() {
        query_text.to_string()
    } else {
        residual_lexical.join(" ")
    };
    let q = if text.trim().is_empty() {
        "*:*".to_string()
    } else {
        format!("{{!edismax qf=all_text}}{}", text)
    };
    (q, fq)
}

/// Extracts the (brand, color) structural signal a compiled query would
/// hand to Solr's `fq`, mirroring `commerce_core::ir::CommerceQuery`'s own
/// resolved constraints -- factored out of Phase 2's inline
/// per-call-site logic (P2-E16) so every caller builds the identical
/// Solr request for the identical compiled query.
pub fn extract_brand_color(
    constraints: &[commerce_core::ir::ResolvedConstraint],
    brand_name_by_id: &std::collections::HashMap<commerce_core::domain::BrandId, String>,
) -> (Option<String>, Option<String>) {
    let brand = constraints.iter().find_map(|c| match c {
        commerce_core::ir::ResolvedConstraint::Structural(
            commerce_core::ir::StructuralConstraint::Brand(id),
        ) => brand_name_by_id.get(id).cloned(),
        commerce_core::ir::ResolvedConstraint::Structural(
            commerce_core::ir::StructuralConstraint::BrandAny(ids),
        ) => ids.first().and_then(|id| brand_name_by_id.get(id)).cloned(),
        _ => None,
    });
    let color = constraints.iter().find_map(|c| match c {
        commerce_core::ir::ResolvedConstraint::Attribute(
            commerce_core::domain::Constraint::Enum { attribute, value },
        ) if attribute == "color" => Some(value.clone()),
        _ => None,
    });
    (brand, color)
}
