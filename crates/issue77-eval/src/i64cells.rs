//! Issue #64 (Infra E4, amendment 1) workload cells: the single definition
//! shared by the native harness (`issue79-eval`) and the competitor harness
//! (`i77_measure`), so both engines are always asked the same question.
//!
//! Every value below is fixed by #64 amendment 1 section 3 (scopes chosen
//! from catalog composition only; facet order; ladder; single- and
//! multi-select disjunctive cells; k = 0 baselines).

/// A scope: a plain enum filter on one `category_depth_<d>` field (never
/// faceted, so never self-excluded), or the full catalog.
#[derive(Debug, Clone, Copy)]
pub struct Scope {
    pub label: &'static str,
    /// `(depth, value)`; `None` is the full catalog (S0).
    pub filter: Option<(u8, &'static str)>,
    /// Most frequent styles in the scope (disjunctive / multi-select cells).
    pub top_styles: [&'static str; 2],
}

pub const SCOPES: [Scope; 6] = [
    Scope {
        label: "s0",
        filter: None,
        top_styles: ["", ""],
    },
    Scope {
        label: "s1",
        filter: Some((1, "Furniture")),
        top_styles: ["", ""],
    },
    Scope {
        label: "s2",
        filter: Some((1, "Décor & Pillows")),
        top_styles: ["modern & contemporary", "traditional"],
    },
    Scope {
        label: "s3",
        filter: Some((2, "Home Improvement / Flooring, Walls & Ceiling")),
        top_styles: ["modern & contemporary", "rustic"],
    },
    Scope {
        label: "s4",
        filter: Some((3, "Furniture / Living Room Furniture / Bookcases")),
        top_styles: ["", ""],
    },
    Scope {
        label: "s5",
        filter: Some((3, "Rugs / Area Rugs / 4' x 6' Area Rugs")),
        top_styles: ["", ""],
    },
];

pub const ATTRIBUTE_FACETS: [&str; 5] = ["style", "color", "primarymaterial", "shape", "material"];
pub const DEPTH_FIELDS: [&str; 6] = [
    "category_depth_1",
    "category_depth_2",
    "category_depth_3",
    "category_depth_4",
    "category_depth_5",
    "category_depth_6",
];
pub const TOP_K: usize = 48;

/// One #64 cell: scope and active filters (single-select `filters`, and
/// multi-select `any_filters` = OR within the attribute) plus facets.
#[derive(Debug, Clone)]
pub struct I64Cell {
    pub name: &'static str,
    pub scope: &'static str,
    /// Plain AND filters: the scope filter (never faceted) and single-select
    /// active filters (self-excluded when their attribute is faceted).
    pub filters: Vec<(&'static str, &'static str)>,
    /// Multi-select active filters: `(attribute, values)`, OR within.
    pub any_filters: Vec<(&'static str, Vec<&'static str>)>,
    pub facets: Vec<&'static str>,
    /// `k` for ladder/baseline cells; `None` for the extra cell types.
    pub k: Option<usize>,
    /// ladder | baseline | color | style1 | style2
    pub kind: &'static str,
    /// In #64's realistic grid R (section 5).
    pub in_realistic_grid: bool,
}

/// The fixed facet order for a scope (section 3).
#[must_use]
pub fn facet_order(scope: &Scope) -> Vec<&'static str> {
    let depth = scope.filter.map_or(0, |(d, _)| d as usize);
    ATTRIBUTE_FACETS
        .iter()
        .copied()
        .chain(DEPTH_FIELDS.iter().copied().skip(depth))
        .collect()
}

fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// All 44 cells, in a fixed order.
#[must_use]
pub fn cells() -> Vec<I64Cell> {
    let mut out = Vec::new();
    for scope in &SCOPES {
        let order = facet_order(scope);
        let k_max = order.len();
        let scope_filter: Vec<(&'static str, &'static str)> = scope
            .filter
            .map(|(d, v)| vec![(DEPTH_FIELDS[d as usize - 1], v)])
            .unwrap_or_default();
        let realistic = scope.filter.is_some();
        let mut ladder: Vec<usize> = [1, 3, 5, 8, k_max]
            .into_iter()
            .filter(|&k| k <= k_max)
            .collect();
        ladder.dedup();
        for k in std::iter::once(0).chain(ladder) {
            out.push(I64Cell {
                name: leak(format!("i64_{}_k{k}", scope.label)),
                scope: scope.label,
                filters: scope_filter.clone(),
                any_filters: Vec::new(),
                facets: order[..k].to_vec(),
                k: Some(k),
                kind: if k == 0 { "baseline" } else { "ladder" },
                in_realistic_grid: realistic && k > 0,
            });
        }
        out.push(I64Cell {
            name: leak(format!("i64_{}_color", scope.label)),
            scope: scope.label,
            filters: scope_filter.clone(),
            any_filters: Vec::new(),
            facets: vec!["color"],
            k: None,
            kind: "color",
            in_realistic_grid: realistic,
        });
        if matches!(scope.label, "s2" | "s3") {
            let mut single = scope_filter.clone();
            single.push(("style", scope.top_styles[0]));
            out.push(I64Cell {
                name: leak(format!("i64_{}_k5_style1", scope.label)),
                scope: scope.label,
                filters: single,
                any_filters: Vec::new(),
                facets: order[..5].to_vec(),
                k: None,
                kind: "style1",
                in_realistic_grid: true,
            });
            out.push(I64Cell {
                name: leak(format!("i64_{}_k5_style2", scope.label)),
                scope: scope.label,
                filters: scope_filter.clone(),
                any_filters: vec![("style", scope.top_styles.to_vec())],
                facets: order[..5].to_vec(),
                k: None,
                kind: "style2",
                in_realistic_grid: true,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_preregistered_grid() {
        let cells = cells();
        assert_eq!(cells.len(), 44);
        assert_eq!(cells.iter().filter(|c| c.in_realistic_grid).count(), 32);
        assert_eq!(cells.iter().filter(|c| c.kind == "baseline").count(), 6);
        let ks = |scope: &str| -> Vec<usize> {
            cells
                .iter()
                .filter(|c| c.scope == scope && c.kind == "ladder")
                .filter_map(|c| c.k)
                .collect()
        };
        assert_eq!(ks("s0"), [1, 3, 5, 8, 11]);
        assert_eq!(ks("s1"), [1, 3, 5, 8, 10]);
        assert_eq!(ks("s3"), [1, 3, 5, 8, 9]);
        assert_eq!(ks("s5"), [1, 3, 5, 8]);
        // A scope's own and shallower depth fields are never faceted.
        for c in &cells {
            for (attr, _) in &c.filters {
                if attr.starts_with("category_depth") {
                    assert!(!c.facets.contains(attr), "{}", c.name);
                }
            }
        }
        let s3 = cells.iter().find(|c| c.name == "i64_s3_k8").unwrap();
        assert_eq!(
            s3.facets,
            [
                "style",
                "color",
                "primarymaterial",
                "shape",
                "material",
                "category_depth_3",
                "category_depth_4",
                "category_depth_5"
            ]
        );
        // scripts/issue64/cell_names.py prints exactly this list.
        let joined: Vec<&str> = cells.iter().map(|c| c.name).collect();
        assert_eq!(joined.join(","), "i64_s0_k0,i64_s0_k1,i64_s0_k3,i64_s0_k5,i64_s0_k8,i64_s0_k11,i64_s0_color,i64_s1_k0,i64_s1_k1,i64_s1_k3,i64_s1_k5,i64_s1_k8,i64_s1_k10,i64_s1_color,i64_s2_k0,i64_s2_k1,i64_s2_k3,i64_s2_k5,i64_s2_k8,i64_s2_k10,i64_s2_color,i64_s2_k5_style1,i64_s2_k5_style2,i64_s3_k0,i64_s3_k1,i64_s3_k3,i64_s3_k5,i64_s3_k8,i64_s3_k9,i64_s3_color,i64_s3_k5_style1,i64_s3_k5_style2,i64_s4_k0,i64_s4_k1,i64_s4_k3,i64_s4_k5,i64_s4_k8,i64_s4_color,i64_s5_k0,i64_s5_k1,i64_s5_k3,i64_s5_k5,i64_s5_k8,i64_s5_color");
        let multi = cells.iter().find(|c| c.name == "i64_s2_k5_style2").unwrap();
        assert_eq!(
            multi.any_filters,
            [("style", vec!["modern & contemporary", "traditional"])]
        );
    }
}
