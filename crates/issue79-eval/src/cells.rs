//! The E3b workload cells, exactly as preregistered in #79 section 5.
//!
//! Held-out headline cells reuse #77's cell names and query parameters
//! verbatim (so the native request is byte-identical to E3's
//! `native_query_string`); calibration cells are new and native-only. All
//! values are real WANDS values; counts in comments are at the 500k tier
//! (`catalog_12x.jsonl`, N = 515,928).

/// Whether a cell may be used to fit the F3/S3 planner constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Held-out: never used to choose tau/rho or the no-crossover winner.
    Headline,
    /// Used only to decide crossover existence and fit tau/rho.
    Calibration,
}

impl Role {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Headline => "headline",
            Self::Calibration => "calibration",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Facet,
    Sort,
}

impl Family {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Facet => "facet",
            Self::Sort => "sort",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub name: &'static str,
    pub role: Role,
    pub family: Family,
    pub category: Option<&'static str>,
    pub filters: Vec<(&'static str, &'static str)>,
    /// Issue #64 multi-select filters (OR within the attribute).
    pub any_filters: Vec<(&'static str, Vec<&'static str>)>,
    pub ranges: Vec<(&'static str, &'static str, f64)>,
    pub facets: Vec<&'static str>,
    /// `(field, descending)`.
    pub sort: Option<(&'static str, bool)>,
    pub top_k: usize,
}

pub const TOP_K: usize = 48;

pub const CATEGORY_BROAD: &str =
    "Furniture / Living Room Furniture / Chairs & Seating / Accent Chairs";
pub const CATEGORY_MEDIUM: &str = "Outdoor / Outdoor Shades / Pergolas";
pub const MODERN: &str = "modern & contemporary";

const DEPTH3: [(&str, &str); 3] = [
    ("color", "white"),
    ("style", MODERN),
    ("primarymaterial", "metal"),
];

fn cell(name: &'static str, role: Role, family: Family) -> Cell {
    Cell {
        name,
        role,
        family,
        category: None,
        filters: Vec::new(),
        any_filters: Vec::new(),
        ranges: Vec::new(),
        facets: Vec::new(),
        sort: None,
        top_k: TOP_K,
    }
}

/// All E3b cells, headline first. See #79 section 5.
#[must_use]
pub fn all_cells() -> Vec<Cell> {
    use Family::{Facet, Sort};
    use Role::{Calibration, Headline};
    let five = vec!["color", "style", "primarymaterial", "material", "shape"];
    vec![
        // --- facet, held-out headline (E3 cells) ---
        Cell {
            facets: vec!["style"],
            ..cell("facet_low_cardinality_style", Headline, Facet)
        },
        Cell {
            facets: vec!["primarymaterial"],
            ..cell("facet_medium_cardinality_primarymaterial", Headline, Facet)
        },
        Cell {
            facets: vec!["color"],
            ..cell("facet_high_cardinality_color", Headline, Facet)
        },
        Cell {
            filters: vec![("color", "black")],
            facets: five,
            ..cell("facet_disjunctive_multi_dim", Headline, Facet)
        },
        // --- facet, calibration ---
        Cell {
            facets: vec!["material"],
            ..cell("fc1_full_material", Calibration, Facet)
        },
        Cell {
            facets: vec!["shape"],
            ..cell("fc2_full_shape", Calibration, Facet)
        },
        Cell {
            filters: vec![("style", MODERN)],
            facets: vec!["color"],
            ..cell("fc3_style_modern_x_color", Calibration, Facet)
        },
        Cell {
            filters: vec![("color", "white")],
            facets: vec!["style"],
            ..cell("fc4_color_white_x_style", Calibration, Facet)
        },
        Cell {
            filters: vec![("color", "white")],
            facets: vec!["primarymaterial"],
            ..cell("fc5_color_white_x_primarymaterial", Calibration, Facet)
        },
        Cell {
            filters: DEPTH3.to_vec(),
            facets: vec!["shape"],
            ..cell("fc6_depth3_x_shape", Calibration, Facet)
        },
        Cell {
            category: Some(CATEGORY_MEDIUM),
            facets: vec!["color"],
            ..cell("fc7_pergolas_x_color", Calibration, Facet)
        },
        // --- sort, held-out headline ---
        Cell {
            ranges: vec![("average_rating", "gte", 4.0)],
            sort: Some(("average_rating", true)),
            ..cell("numeric_range_sort", Headline, Sort)
        },
        Cell {
            filters: vec![("color", "white")],
            sort: Some(("review_count", true)),
            ..cell("sh2_color_white_review_count_desc", Headline, Sort)
        },
        Cell {
            filters: DEPTH3.to_vec(),
            sort: Some(("review_count", true)),
            ..cell("sh3_depth3_review_count_desc", Headline, Sort)
        },
        // --- sort, calibration ---
        Cell {
            sort: Some(("rating_count", true)),
            ..cell("sc1_full_rating_count_desc", Calibration, Sort)
        },
        Cell {
            filters: vec![("style", MODERN)],
            sort: Some(("average_rating", false)),
            ..cell("sc2_style_modern_average_rating_asc", Calibration, Sort)
        },
        Cell {
            category: Some(CATEGORY_BROAD),
            sort: Some(("review_count", true)),
            ..cell("sc3_accent_chairs_review_count_desc", Calibration, Sort)
        },
        Cell {
            filters: vec![("color", "black")],
            sort: Some(("rating_count", false)),
            ..cell("sc4_color_black_rating_count_asc", Calibration, Sort)
        },
        Cell {
            category: Some(CATEGORY_MEDIUM),
            sort: Some(("average_rating", true)),
            ..cell("sc5_pergolas_average_rating_desc", Calibration, Sort)
        },
        Cell {
            filters: vec![
                ("color", "white"),
                ("style", MODERN),
                ("primarymaterial", "metal"),
                ("shape", "square"),
            ],
            sort: Some(("review_count", true)),
            ..cell("sc6_depth5_review_count_desc", Calibration, Sort)
        },
    ]
}

/// Issue #63 (amendment 1, section 2): #77's unsorted filter-depth cells,
/// measured as same-window structural references in #63's Part A. Kept
/// out of [`all_cells`] so every #79 selection (and #79's gate) is
/// unchanged. Values are #77's `I77_FILTER_DEPTH*` (`resource_envelope.env`).
#[must_use]
pub fn reference_cells() -> Vec<Cell> {
    use Family::Facet;
    use Role::Headline;
    let depth1 = vec![("color", "white")];
    let depth3 = DEPTH3.to_vec();
    let mut depth5 = DEPTH3.to_vec();
    depth5.push(("shape", "square"));
    vec![
        Cell {
            filters: depth1,
            ..cell("filter_depth_1", Headline, Facet)
        },
        Cell {
            filters: depth3,
            ..cell("filter_depth_3", Headline, Facet)
        },
        Cell {
            filters: depth5,
            ..cell("filter_depth_5", Headline, Facet)
        },
    ]
}

/// Issue #64 (amendment 1): the 44 facet-economics cells, mapped from the
/// single shared definition in `issue77_eval::i64cells`. Kept out of
/// [`all_cells`] and [`reference_cells`].
#[must_use]
pub fn i64_cells() -> Vec<Cell> {
    issue77_eval::i64cells::cells()
        .into_iter()
        .map(|c| Cell {
            filters: c.filters,
            any_filters: c.any_filters,
            facets: c.facets,
            top_k: issue77_eval::i64cells::TOP_K,
            ..cell(c.name, Role::Headline, Family::Facet)
        })
        .collect()
}

/// The sort fields E3b builds sort structures for.
pub const SORT_FIELDS: [&str; 3] = ["average_rating", "review_count", "rating_count"];

/// Percent-encoding identical to #77's `i77_measure::urlencode`.
#[must_use]
pub fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The `/plp` query string for a cell, in #77's parameter order (category,
/// filters, facets, range, sort, topk), plus optional E3b mode parameters.
/// Unknown parameters are ignored by #77's unchanged server, so the N0
/// binary receives the exact E3 request for E3 cells.
#[must_use]
pub fn query_string(cell: &Cell, modes: Option<(&str, &str)>) -> String {
    query_string_with_cand(cell, modes, None)
}

/// [`query_string`] plus Issue #63's `cand_mode` parameter (omitted when
/// `None`, so every #79 request string is unchanged).
#[must_use]
pub fn query_string_with_cand(
    cell: &Cell,
    modes: Option<(&str, &str)>,
    cand_mode: Option<&str>,
) -> String {
    let mut params: Vec<String> = Vec::new();
    if let Some(category) = cell.category {
        params.push(format!("category={}", urlencode(category)));
    }
    for (attr, value) in &cell.filters {
        params.push(format!("filter={attr}:{}", urlencode(value)));
    }
    for (attr, values) in &cell.any_filters {
        let encoded: Vec<String> = values.iter().map(|v| urlencode(v)).collect();
        params.push(format!("anyfilter={attr}:{}", encoded.join("|")));
    }
    if !cell.facets.is_empty() {
        params.push(format!("facets={}", cell.facets.join(",")));
    }
    for (attr, op, value) in &cell.ranges {
        params.push(format!("range={attr}:{op}:{value}"));
    }
    if let Some((attr, descending)) = cell.sort {
        params.push(format!(
            "sort={attr}:{}",
            if descending { "desc" } else { "asc" }
        ));
    }
    params.push(format!("topk={}", cell.top_k));
    if let Some((facet_mode, sort_mode)) = modes {
        params.push(format!("facet_mode={facet_mode}"));
        params.push(format!("sort_mode={sort_mode}"));
    }
    if let Some(cand_mode) = cand_mode {
        params.push(format!("cand_mode={cand_mode}"));
    }
    format!("/plp?{}", params.join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_cells_match_e3_native_requests_exactly() {
        let cells = reference_cells();
        let by_name = |name: &str| cells.iter().find(|c| c.name == name).unwrap();
        // #77's `native_query_string` path part for the filter-depth cells.
        assert_eq!(
            query_string(by_name("filter_depth_1"), None),
            "/plp?filter=color:white&topk=48"
        );
        assert_eq!(
            query_string(by_name("filter_depth_5"), None),
            "/plp?filter=color:white&filter=style:modern%20%26%20contemporary\
             &filter=primarymaterial:metal&filter=shape:square&topk=48"
        );
        assert!(all_cells()
            .iter()
            .all(|c| !c.name.starts_with("filter_depth")));
    }

    #[test]
    fn headline_cells_match_e3_native_requests_exactly() {
        let cells = all_cells();
        let by_name = |name: &str| cells.iter().find(|c| c.name == name).unwrap();
        // Byte-for-byte what #77's `native_query_string` produced (path part).
        assert_eq!(
            query_string(by_name("facet_low_cardinality_style"), None),
            "/plp?facets=style&topk=48"
        );
        assert_eq!(
            query_string(by_name("facet_disjunctive_multi_dim"), None),
            "/plp?filter=color:black&facets=color,style,primarymaterial,material,shape&topk=48"
        );
        assert_eq!(
            query_string(by_name("numeric_range_sort"), None),
            "/plp?range=average_rating:gte:4&sort=average_rating:desc&topk=48"
        );
    }

    #[test]
    fn e3_probe_values_match_the_frozen_e3_envelope() {
        let env = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../benchmarks/configs/issue77/resource_envelope.env"
        ))
        .unwrap();
        for line in [
            "I77_FACET_COLOR_PROBE=black",
            "I77_FILTER_DEPTH1_COLOR=white",
            "I77_FILTER_DEPTH3_STYLE=\"modern & contemporary\"",
            "I77_FILTER_DEPTH3_PRIMARYMATERIAL=metal",
            "I77_FILTER_DEPTH5_SHAPE=square",
            "I77_FILTER_DEPTH5_RATING_GTE=4",
        ] {
            assert!(env.lines().any(|l| l == line), "missing {line}");
        }
        assert!(env.contains(&format!("I77_CATEGORY_BROAD=\"{CATEGORY_BROAD}\"")));
        assert!(env.contains(&format!("I77_CATEGORY_MEDIUM=\"{CATEGORY_MEDIUM}\"")));
    }

    #[test]
    fn cell_names_are_unique_and_roles_split() {
        let cells = all_cells();
        let mut names: Vec<_> = cells.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), cells.len());
        assert_eq!(cells.iter().filter(|c| c.role == Role::Headline).count(), 7);
        assert_eq!(
            cells.iter().filter(|c| c.role == Role::Calibration).count(),
            13
        );
    }
}
