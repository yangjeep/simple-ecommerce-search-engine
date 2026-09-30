//! Issue #65 workload (amendment 1 section 8): the frozen request pools for
//! classes A-F, their oracle expectations, the native (N1) path and the Solr
//! request for each, the frozen mixes, and deterministic request sequences.

use std::collections::{BTreeMap, HashMap};

use commerce_core::domain::{AttributeValue, Catalog, CategoryId, ProductId};
use issue77_eval::i64cells::{facet_order, SCOPES};
use issue79_eval::cells::urlencode;
use issue79_eval::oracle::Oracle;
use issue79_eval::plp::{CandidateMode, FacetMode, PlpRequest, SortMode};
use serde::{Deserialize, Serialize};

pub const SEED: u64 = 65;
pub const TOP_K: usize = 48;
pub const CLASSES: [&str; 6] = ["A", "B", "C", "D", "E", "F"];

/// Frozen mixes: request-share weights over A..F (amendment section 8), plus
/// the conditional decomposition control / native-heavy diagnostic mix.
pub const MIXES: [(&str, [f64; 6]); 5] = [
    ("primary", [0.10, 0.15, 0.10, 0.20, 0.20, 0.25]),
    ("structural", [0.10, 0.20, 0.10, 0.25, 0.25, 0.10]),
    ("lexical", [0.05, 0.10, 0.05, 0.15, 0.15, 0.50]),
    // Primary restricted to A-E, renormalized.
    (
        "no_lexical",
        [
            0.10 / 0.75,
            0.15 / 0.75,
            0.10 / 0.75,
            0.20 / 0.75,
            0.20 / 0.75,
            0.0,
        ],
    ),
    // The native scaling diagnostic (section 14): B-E of the primary mix,
    // renormalized. N0 (#79's server) has no exact-lookup endpoint, so this
    // same A-free mix is used for N0 and every N1 worker count.
    (
        "native_plp",
        [0.0, 0.15 / 0.65, 0.10 / 0.65, 0.20 / 0.65, 0.20 / 0.65, 0.0],
    ),
];

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Expect {
    pub num_found: Option<u64>,
    pub hit_count: Option<u64>,
    /// Class A: exact ids; class F: the B0-recorded ranked top-48.
    pub ids: Option<Vec<String>>,
    /// Class C: the top-48 sort-value sequence (tie order is engine-defined).
    pub sort_values: Option<Vec<Option<f64>>>,
    /// Class E: complete facet maps.
    pub facets: Option<HashMap<String, BTreeMap<String, u64>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Req {
    pub id: String,
    pub class: String,
    /// GET path for N1 (None for class F, which H1 routes to Solr).
    pub native_path: Option<String>,
    /// POST body for `/solr/i77_wands/select` (JSON Request API).
    pub solr_body: serde_json::Value,
    pub expect: Expect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pools {
    pub seed: u64,
    pub catalog: String,
    pub requests: Vec<Req>,
    /// Request ids removed as NOT_EQUIVALENT_WORK (from both treatments).
    #[serde(default)]
    pub excluded: Vec<String>,
}

impl Pools {
    #[must_use]
    pub fn hash(&self) -> String {
        // Canonical JSON: `serde_json::Value` objects are key-ordered, so the
        // HashMap-backed facet expectations hash deterministically. (Before
        // this fix the hash depended on HashMap iteration order; the pools
        // file's own sha256 is the authoritative identity of every run.)
        let text = serde_json::to_value(&self.requests)
            .expect("json")
            .to_string();
        issue61_eval::sha256_hex(text.as_bytes())
    }

    #[must_use]
    pub fn by_class(&self) -> BTreeMap<String, Vec<usize>> {
        let mut out: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, r) in self.requests.iter().enumerate() {
            if !self.excluded.contains(&r.id) {
                out.entry(r.class.clone()).or_default().push(i);
            }
        }
        out
    }
}

/// A structural request (classes B-E).
#[derive(Debug, Clone, Default)]
pub struct Plp {
    pub filters: Vec<(String, String)>,
    pub ranges: Vec<(String, String, f64)>,
    pub facets: Vec<String>,
    pub sort: Option<(String, bool)>,
}

impl Plp {
    #[must_use]
    pub fn to_request(&self) -> PlpRequest {
        PlpRequest {
            category: None,
            filters: self.filters.clone(),
            any_filters: Vec::new(),
            ranges: self.ranges.clone(),
            facets: self.facets.clone(),
            sort: self.sort.clone(),
            top_k: TOP_K,
            offset: 0,
            facet_mode: FacetMode::Hybrid,
            sort_mode: SortMode::Hybrid,
            cand_mode: CandidateMode::P0r,
        }
    }

    /// The N1 path: #79/#63 executor, N+ = hybrid:hybrid:p0r.
    #[must_use]
    pub fn native_path(&self) -> String {
        let mut params = Vec::new();
        for (a, v) in &self.filters {
            params.push(format!("filter={a}:{}", urlencode(v)));
        }
        if !self.facets.is_empty() {
            params.push(format!("facets={}", self.facets.join(",")));
        }
        for (a, op, v) in &self.ranges {
            params.push(format!("range={a}:{op}:{v}"));
        }
        if let Some((a, desc)) = &self.sort {
            params.push(format!("sort={a}:{}", if *desc { "desc" } else { "asc" }));
        }
        params.push(format!("topk={TOP_K}"));
        params.push("facet_mode=hybrid&sort_mode=hybrid&cand_mode=p0r".to_owned());
        format!("/plp?{}", params.join("&"))
    }

    /// The equal-work Solr request (#63/#64 conventions: tagged filters and
    /// `excludeTags` for a faceted attribute's own selection, `limit:-1`).
    #[must_use]
    pub fn solr_body(&self) -> serde_json::Value {
        let faceted = |a: &str| self.facets.iter().any(|f| f == a);
        let mut fq: Vec<String> = Vec::new();
        for (a, v) in &self.filters {
            let clause = format!("{a}:{}", solr_quote(v));
            fq.push(if faceted(a) {
                format!("{{!tag={a}}}{clause}")
            } else {
                clause
            });
        }
        for (a, op, v) in &self.ranges {
            let range = match op.as_str() {
                "gte" => format!("[{v} TO *]"),
                "gt" => format!("{{{v} TO *]"),
                "lte" => format!("[* TO {v}]"),
                "lt" => format!("[* TO {v}}}"),
                _ => format!("[{v} TO {v}]"),
            };
            fq.push(format!("{a}:{range}"));
        }
        let mut body = serde_json::json!({
            "query": "*:*", "filter": fq, "limit": TOP_K, "fields": "id",
        });
        if !self.facets.is_empty() {
            let mut facets = serde_json::Map::new();
            for f in &self.facets {
                let mut def = serde_json::json!({"type": "terms", "field": f, "limit": -1});
                if self.filters.iter().any(|(a, _)| a == f) {
                    def["domain"] = serde_json::json!({"excludeTags": [f]});
                }
                facets.insert(f.clone(), def);
            }
            body["facet"] = serde_json::Value::Object(facets);
        }
        if let Some((a, desc)) = &self.sort {
            body["sort"] = serde_json::json!(format!("{a} {}", if *desc { "desc" } else { "asc" }));
            // Return the sort value too, as native's `sort_value` does.
            body["fields"] = serde_json::json!(format!("id,{a}"));
        }
        body
    }
}

#[must_use]
pub fn solr_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The frozen #57 Solr lexical request (class F).
#[must_use]
pub fn lexical_body(text: &str) -> serde_json::Value {
    serde_json::json!({
        "query": text,
        "params": {"defType": "edismax", "qf": "title description"},
        "limit": TOP_K,
        "fields": "id",
    })
}

struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn top_values(catalog: &Catalog, attr: &str, n: usize) -> Vec<String> {
    let mut counts: HashMap<String, u64> = HashMap::new();
    for p in &catalog.products {
        for v in &p.variants {
            if let Some(AttributeValue::Enum(x)) =
                commerce_core::domain::effective_attributes(p, v).get(attr)
            {
                *counts.entry(x.clone()).or_insert(0) += 1;
            }
        }
    }
    let mut sorted: Vec<(String, u64)> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    sorted.into_iter().take(n).map(|(v, _)| v).collect()
}

fn depth_sizes(catalog: &Catalog) -> Vec<(u8, String, u64)> {
    let mut counts: HashMap<(u8, String), u64> = HashMap::new();
    for p in &catalog.products {
        for v in &p.variants {
            let attrs = commerce_core::domain::effective_attributes(p, v);
            for d in 1..=3u8 {
                if let Some(AttributeValue::Enum(x)) = attrs.get(&format!("category_depth_{d}")) {
                    *counts.entry((d, x.clone())).or_insert(0) += 1;
                }
            }
        }
    }
    let mut out: Vec<(u8, String, u64)> = counts.into_iter().map(|((d, v), c)| (d, v, c)).collect();
    out.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    out
}

fn plp_expect(
    oracle: &Oracle,
    plp: &Plp,
    category_id_by_leaf: &HashMap<String, CategoryId>,
    source_ids: &HashMap<ProductId, String>,
    class: &str,
) -> Expect {
    let e = oracle
        .expected(&plp.to_request(), category_id_by_leaf, source_ids)
        .expect("oracle");
    let hits = e.docs.len() as u64;
    Expect {
        num_found: Some(e.num_found as u64),
        hit_count: Some(hits),
        ids: None,
        sort_values: (class == "C").then(|| e.docs.iter().map(|d| d.sort_value).collect()),
        facets: (class == "E").then(|| e.facets.clone()),
    }
}

/// Builds every pool (section 8). `queries` are WANDS `query.csv` texts.
#[must_use]
pub fn build(
    catalog: &Catalog,
    catalog_label: &str,
    category_id_by_leaf: &HashMap<String, CategoryId>,
    source_ids: &HashMap<ProductId, String>,
    queries: &[String],
) -> Pools {
    let oracle = Oracle::new(catalog);
    let mut rng = XorShift(SEED.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut requests = Vec::new();
    let push_plp = |class: &str, n: usize, plp: Plp, requests: &mut Vec<Req>| {
        requests.push(Req {
            id: format!("{class}-{n:04}"),
            class: class.to_owned(),
            native_path: Some(plp.native_path()),
            solr_body: plp.solr_body(),
            expect: plp_expect(&oracle, &plp, category_id_by_leaf, source_ids, class),
        });
    };

    // A: 200 product ids, uniform by seed over catalog order.
    let mut chosen = std::collections::BTreeSet::new();
    while chosen.len() < 200.min(catalog.products.len()) {
        chosen.insert((rng.next() % catalog.products.len() as u64) as usize);
    }
    for (n, i) in chosen.into_iter().enumerate() {
        let sid = source_ids
            .get(&catalog.products[i].id)
            .cloned()
            .unwrap_or_default();
        requests.push(Req {
            id: format!("A-{n:04}"),
            class: "A".to_owned(),
            native_path: Some(format!("/lookup?id={}", urlencode(&sid))),
            solr_body: serde_json::json!({
                "query": "*:*", "filter": [format!("id:{}", solr_quote(&sid))], "limit": 1, "fields": "id",
            }),
            expect: Expect { num_found: Some(1), hit_count: Some(1), ids: Some(vec![sid]), ..Expect::default() },
        });
    }

    // B: top-20 color and style single filters; depth-3/4/5 x top-6 colors.
    let mut n = 0;
    for attr in ["color", "style"] {
        for v in top_values(catalog, attr, 20) {
            push_plp(
                "B",
                n,
                Plp {
                    filters: vec![(attr.to_owned(), v)],
                    ..Plp::default()
                },
                &mut requests,
            );
            n += 1;
        }
    }
    for c in top_values(catalog, "color", 6) {
        let depth3 = vec![
            ("color".to_owned(), c.clone()),
            ("style".to_owned(), "modern & contemporary".to_owned()),
            ("primarymaterial".to_owned(), "metal".to_owned()),
        ];
        let mut depth4 = depth3.clone();
        depth4.push(("shape".to_owned(), "square".to_owned()));
        for plp in [
            Plp {
                filters: depth3.clone(),
                ..Plp::default()
            },
            Plp {
                filters: depth4.clone(),
                ..Plp::default()
            },
            Plp {
                filters: depth4.clone(),
                ranges: vec![("average_rating".into(), "gte".into(), 4.0)],
                ..Plp::default()
            },
        ] {
            push_plp("B", n, plp, &mut requests);
            n += 1;
        }
    }

    // C: rating range x sort field, desc.
    let mut n = 0;
    for t in [3.0, 3.5, 4.0, 4.5] {
        for f in ["average_rating", "review_count", "rating_count"] {
            push_plp(
                "C",
                n,
                Plp {
                    ranges: vec![("average_rating".into(), "gte".into(), t)],
                    sort: Some((f.to_owned(), true)),
                    ..Plp::default()
                },
                &mut requests,
            );
            n += 1;
        }
    }

    // D: #64 S1-S5 + the 15 next-largest depth-1..3 values with >= 1000 docs.
    let mut scopes: Vec<(u8, String)> = SCOPES
        .iter()
        .filter_map(|s| s.filter.map(|(d, v)| (d, v.to_owned())))
        .collect();
    for (d, v, count) in depth_sizes(catalog) {
        if scopes.len() >= 20 {
            break;
        }
        if count >= 1000 && !scopes.iter().any(|(sd, sv)| *sd == d && *sv == v) {
            scopes.push((d, v));
        }
    }
    for (n, (d, v)) in scopes.iter().enumerate() {
        push_plp(
            "D",
            n,
            Plp {
                filters: vec![(format!("category_depth_{d}"), v.clone())],
                ..Plp::default()
            },
            &mut requests,
        );
    }

    // E: S1/S2/S3 x k in {1,5}, + S2/S3 k=5 single-select style.
    let mut n = 0;
    for label in ["s1", "s2", "s3"] {
        let scope = SCOPES.iter().find(|s| s.label == label).expect("scope");
        let (d, v) = scope.filter.expect("scoped");
        let order = facet_order(scope);
        let base = vec![(format!("category_depth_{d}"), v.to_owned())];
        for k in [1usize, 5] {
            let plp = Plp {
                filters: base.clone(),
                facets: order[..k].iter().map(|s| (*s).to_owned()).collect(),
                ..Plp::default()
            };
            push_plp("E", n, plp, &mut requests);
            n += 1;
        }
        if matches!(label, "s2" | "s3") {
            let mut filters = base.clone();
            filters.push(("style".to_owned(), scope.top_styles[0].to_owned()));
            let plp = Plp {
                filters,
                facets: order[..5].iter().map(|s| (*s).to_owned()).collect(),
                ..Plp::default()
            };
            push_plp("E", n, plp, &mut requests);
            n += 1;
        }
    }

    // F: every WANDS judged query; expectations recorded from B0 later.
    for (n, q) in queries.iter().enumerate() {
        requests.push(Req {
            id: format!("F-{n:04}"),
            class: "F".to_owned(),
            native_path: None,
            solr_body: lexical_body(q),
            expect: Expect::default(),
        });
    }
    Pools {
        seed: SEED,
        catalog: catalog_label.to_owned(),
        requests,
        excluded: Vec::new(),
    }
}

/// One scheduled logical request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Scheduled {
    /// Offset from the run start, microseconds.
    pub at_us: u64,
    /// Index into `Pools::requests`.
    pub req: usize,
}

/// The pre-generated sequence for (mix, rate): Poisson arrivals and
/// class-weighted, within-class-uniform request choice, both from one seed
/// derived from (SEED, mix, rate). Identical for B0 and H1.
#[must_use]
pub fn sequence(pools: &Pools, mix: &str, rate_qps: f64, total_s: f64) -> Vec<Scheduled> {
    let weights = MIXES.iter().find(|(m, _)| *m == mix).expect("mix").1;
    let by_class = pools.by_class();
    let mut seed = SEED ^ (rate_qps.to_bits().rotate_left(17));
    for b in mix.bytes() {
        seed = seed.wrapping_mul(0x100_0000_01b3) ^ u64::from(b);
    }
    let mut rng = XorShift(seed | 1);
    let total_w: f64 = weights.iter().sum();
    let mut out = Vec::new();
    let mut t = 0.0f64;
    loop {
        t += -(1.0 - rng.unit()).ln() / rate_qps;
        if t >= total_s {
            break;
        }
        let mut pick = rng.unit() * total_w;
        // Fallback for rounding: the last class with positive weight (a
        // zero-weight class must never be scheduled).
        let mut class = weights
            .iter()
            .rposition(|w| *w > 0.0)
            .expect("a positive weight");
        for (i, w) in weights.iter().enumerate() {
            if *w > 0.0 && pick < *w {
                class = i;
                break;
            }
            pick -= w;
        }
        let pool = &by_class[CLASSES[class]];
        let req = pool[(rng.next() % pool.len() as u64) as usize];
        out.push(Scheduled {
            at_us: (t * 1e6) as u64,
            req,
        });
    }
    out
}

#[must_use]
pub fn sequence_hash(seq: &[Scheduled]) -> String {
    let mut bytes = Vec::with_capacity(seq.len() * 16);
    for s in seq {
        bytes.extend_from_slice(&s.at_us.to_le_bytes());
        bytes.extend_from_slice(&(s.req as u64).to_le_bytes());
    }
    issue61_eval::sha256_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_pools() -> Pools {
        let mut requests = Vec::new();
        for (i, c) in CLASSES.iter().enumerate() {
            for j in 0..3 {
                requests.push(Req {
                    id: format!("{c}-{j}"),
                    class: (*c).to_owned(),
                    native_path: None,
                    solr_body: serde_json::json!({"i": i}),
                    expect: Expect::default(),
                });
            }
        }
        Pools {
            seed: SEED,
            catalog: "t".into(),
            requests,
            excluded: Vec::new(),
        }
    }

    #[test]
    fn sequences_are_deterministic_poisson_and_follow_the_mix() {
        let pools = tiny_pools();
        let a = sequence(&pools, "primary", 200.0, 50.0);
        let b = sequence(&pools, "primary", 200.0, 50.0);
        assert_eq!(a, b);
        assert_eq!(sequence_hash(&a), sequence_hash(&b));
        assert!((a.len() as f64 - 10_000.0).abs() < 400.0, "{}", a.len());
        let f = a
            .iter()
            .filter(|s| pools.requests[s.req].class == "F")
            .count() as f64
            / a.len() as f64;
        assert!((f - 0.25).abs() < 0.02, "{f}");
        let none = sequence(&pools, "no_lexical", 200.0, 50.0);
        assert!(none.iter().all(|s| pools.requests[s.req].class != "F"));
        let plp = sequence(&pools, "native_plp", 500.0, 50.0);
        assert!(plp
            .iter()
            .all(|s| !matches!(pools.requests[s.req].class.as_str(), "A" | "F")));
        assert_ne!(
            sequence_hash(&a),
            sequence_hash(&sequence(&pools, "primary", 300.0, 50.0))
        );
        assert!(a.windows(2).all(|w| w[0].at_us <= w[1].at_us));
    }

    #[test]
    fn pools_hash_is_deterministic_with_facet_maps() {
        let mut pools = tiny_pools();
        let facets: HashMap<String, BTreeMap<String, u64>> = (0..20)
            .map(|i| (format!("f{i}"), BTreeMap::from([(format!("v{i}"), i)])))
            .collect();
        pools.requests[0].expect.facets = Some(facets);
        let a = pools.hash();
        let reparsed: Pools =
            serde_json::from_str(&serde_json::to_string(&pools).unwrap()).unwrap();
        assert_eq!(reparsed.hash(), a);
    }

    #[test]
    fn excluded_requests_never_scheduled() {
        let mut pools = tiny_pools();
        pools.excluded.push("B-0".into());
        let seq = sequence(&pools, "primary", 500.0, 20.0);
        assert!(seq.iter().all(|s| pools.requests[s.req].id != "B-0"));
    }

    #[test]
    fn solr_body_tags_only_faceted_selections() {
        let plp = Plp {
            filters: vec![
                ("category_depth_1".into(), "D\u{e9}cor & Pillows".into()),
                ("style".into(), "modern".into()),
            ],
            ranges: vec![("average_rating".into(), "gte".into(), 4.0)],
            facets: vec!["style".into(), "color".into()],
            sort: Some(("review_count".into(), true)),
        };
        let b = plp.solr_body();
        assert_eq!(b["filter"][0], "category_depth_1:\"D\u{e9}cor & Pillows\"");
        assert_eq!(b["filter"][1], "{!tag=style}style:\"modern\"");
        assert_eq!(b["filter"][2], "average_rating:[4 TO *]");
        assert_eq!(b["facet"]["style"]["domain"]["excludeTags"][0], "style");
        assert!(b["facet"]["color"].get("domain").is_none());
        assert_eq!(b["sort"], "review_count desc");
        let p = plp.native_path();
        assert!(p.starts_with("/plp?filter=category_depth_1:D%C3%A9cor%20%26%20Pillows&filter=style:modern&facets=style,color&range=average_rating:gte:4&sort=review_count:desc&topk=48"), "{p}");
        assert!(p.ends_with("cand_mode=p0r"));
    }
}
