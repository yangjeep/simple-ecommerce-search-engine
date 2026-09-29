//! Equal-work verification (amendment 1, section 2): an engine's captured
//! response for a cell is EQUIVALENT only if `num_found` and every
//! requested facet's complete `{value: count}` map equal the independent
//! oracle's exactly, and hits carry identifiers only. Anything else is
//! `NOT_EQUIVALENT_WORK` for that (engine, cell), with the differences
//! itemized -- never silently accepted or coerced.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// Keys a hit may carry and still be "IDs only": the identifier, plus
/// native's sort key (null on unsorted cells).
pub const ID_ONLY_KEYS: [&str; 2] = ["id", "sort_value"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FieldDiff {
    pub expected_values: usize,
    pub returned_values: usize,
    /// Values present on both sides with different counts.
    pub count_mismatches: usize,
    /// Expected values the engine did not return.
    pub missing_values: usize,
    /// Returned values the oracle does not have.
    pub extra_values: usize,
    /// Up to 5 example differences, `value: expected vs returned`.
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Verdict {
    pub engine: String,
    pub cell: String,
    pub equivalent: bool,
    pub num_found_expected: u64,
    pub num_found_returned: Option<u64>,
    pub facets_exact: bool,
    pub hits_id_only: bool,
    pub hit_keys: Vec<String>,
    pub backend_requests: Option<u64>,
    pub fields: BTreeMap<String, FieldDiff>,
}

/// Parses a `{field: {value: count}}` object into maps; counts must be
/// non-negative integers (a string or float count is a mismatch, not
/// coerced).
fn parse_facets(
    value: &serde_json::Value,
) -> Result<BTreeMap<String, BTreeMap<String, u64>>, String> {
    let mut out = BTreeMap::new();
    let Some(object) = value.as_object() else {
        return if value.is_null() {
            Ok(out)
        } else {
            Err(format!("facets is not an object: {value}"))
        };
    };
    for (field, counts) in object {
        let mut map = BTreeMap::new();
        for (v, count) in counts
            .as_object()
            .ok_or_else(|| format!("facet {field} is not an object"))?
        {
            let count = count
                .as_u64()
                .ok_or_else(|| format!("facet {field}/{v}: count {count} is not an integer"))?;
            map.insert(v.clone(), count);
        }
        out.insert(field.clone(), map);
    }
    Ok(out)
}

/// Compares one engine dump (the JSON written by `I63_DUMP_DIR`) with the
/// oracle's expected `num_found` and facet maps for the same cell.
#[must_use]
pub fn compare(
    engine: &str,
    cell: &str,
    dump: &serde_json::Value,
    expected_num_found: u64,
    expected_facets: &HashMap<String, BTreeMap<String, u64>>,
) -> Verdict {
    let num_found_returned = dump["num_found"].as_u64();
    let hit_keys: Vec<String> = dump["hit_keys"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|k| k.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let hits_id_only = hit_keys.iter().any(|k| k == "id")
        && hit_keys.iter().all(|k| ID_ONLY_KEYS.contains(&k.as_str()));
    let mut fields = BTreeMap::new();
    let facets_exact = match parse_facets(&dump["facets"]) {
        Err(error) => {
            fields.insert(
                "<parse>".to_owned(),
                FieldDiff {
                    examples: vec![error],
                    ..FieldDiff::default()
                },
            );
            false
        }
        Ok(returned) => {
            let mut exact = true;
            let empty = BTreeMap::new();
            let mut names: Vec<&String> = expected_facets.keys().collect();
            names.extend(
                returned
                    .keys()
                    .filter(|k| !expected_facets.contains_key(*k)),
            );
            names.sort();
            names.dedup();
            for field in names {
                let want = expected_facets.get(field).unwrap_or(&empty);
                let got = returned.get(field).unwrap_or(&empty);
                let mut diff = FieldDiff {
                    expected_values: want.len(),
                    returned_values: got.len(),
                    ..FieldDiff::default()
                };
                for (value, count) in want {
                    match got.get(value) {
                        None => {
                            diff.missing_values += 1;
                            if diff.examples.len() < 5 {
                                diff.examples.push(format!("{value}: {count} vs missing"));
                            }
                        }
                        Some(c) if c != count => {
                            diff.count_mismatches += 1;
                            if diff.examples.len() < 5 {
                                diff.examples.push(format!("{value}: {count} vs {c}"));
                            }
                        }
                        Some(_) => {}
                    }
                }
                for (value, c) in got {
                    if !want.contains_key(value) {
                        diff.extra_values += 1;
                        if diff.examples.len() < 5 {
                            diff.examples.push(format!("{value}: missing vs {c}"));
                        }
                    }
                }
                exact &= diff.count_mismatches == 0
                    && diff.missing_values == 0
                    && diff.extra_values == 0;
                fields.insert(field.clone(), diff);
            }
            exact
        }
    };
    let num_found_ok = num_found_returned == Some(expected_num_found);
    Verdict {
        engine: engine.to_owned(),
        cell: cell.to_owned(),
        equivalent: num_found_ok && facets_exact && hits_id_only,
        num_found_expected: expected_num_found,
        num_found_returned,
        facets_exact,
        hits_id_only,
        hit_keys,
        backend_requests: dump["backend_requests"].as_u64(),
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expected() -> HashMap<String, BTreeMap<String, u64>> {
        let mut color = BTreeMap::new();
        color.insert("black".to_owned(), 3);
        color.insert("white".to_owned(), 1);
        HashMap::from([("color".to_owned(), color)])
    }

    #[test]
    fn exact_complete_facets_and_id_only_hits_are_equivalent() {
        let dump = json!({"num_found": 4, "facets": {"color": {"white": 1, "black": 3}},
                          "hit_keys": ["id"], "backend_requests": 1});
        let v = compare("solr", "fh", &dump, 4, &expected());
        assert!(v.equivalent, "{v:?}");
    }

    #[test]
    fn a_truncated_distribution_is_not_equivalent_work() {
        // What Meilisearch's default maxValuesPerFacet=100 does to V=2825.
        let dump = json!({"num_found": 4, "facets": {"color": {"black": 3}}, "hit_keys": ["id"]});
        let v = compare("meilisearch", "fh", &dump, 4, &expected());
        assert!(!v.equivalent);
        assert_eq!(v.fields["color"].missing_values, 1);
    }

    #[test]
    fn wrong_counts_extra_values_documents_and_num_found_all_fail() {
        let base = |facets: serde_json::Value, keys: serde_json::Value, n: u64| json!({"num_found": n, "facets": facets, "hit_keys": keys});
        let ok_facets = json!({"color": {"white": 1, "black": 3}});
        let cases = [
            base(json!({"color": {"white": 1, "black": 2}}), json!(["id"]), 4),
            base(
                json!({"color": {"white": 1, "black": 3, "red": 1}}),
                json!(["id"]),
                4,
            ),
            base(ok_facets.clone(), json!(["id", "title"]), 4),
            base(ok_facets.clone(), json!([]), 4),
            base(ok_facets.clone(), json!(["id"]), 5),
            base(
                json!({"color": {"white": "1", "black": 3}}),
                json!(["id"]),
                4,
            ),
            base(
                json!({"color": {"white": 1, "black": 3}, "style": {"x": 1}}),
                json!(["id"]),
                4,
            ),
        ];
        for dump in cases {
            assert!(
                !compare("e", "c", &dump, 4, &expected()).equivalent,
                "{dump}"
            );
        }
        // Native's sort key is allowed alongside the id.
        let native = base(ok_facets, json!(["id", "sort_value"]), 4);
        assert!(compare("native", "c", &native, 4, &expected()).equivalent);
    }
}
