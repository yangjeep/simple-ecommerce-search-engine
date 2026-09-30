//! Normalizes a native (N1) or Solr response into one [`Observed`] form and
//! compares it with a request's frozen [`Expect`]ation.
//!
//! - Pre-pass validation ([`check_full`]) compares everything the class
//!   defines: ids (A, F), `num_found` + hit count (B, D), the sort-value
//!   sequence (C), complete facet maps (E).
//! - In-load checks ([`check_fast`]) compare `num_found` (and, for A, the
//!   id), so every measured response is verified without dominating the
//!   generator's CPU.

use std::collections::{BTreeMap, HashMap};

use crate::workload::Expect;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observed {
    pub num_found: Option<u64>,
    pub ids: Vec<String>,
    pub sort_values: Vec<Option<f64>>,
    pub facets: HashMap<String, BTreeMap<String, u64>>,
}

fn as_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::Array(a) => a.first().and_then(serde_json::Value::as_f64),
        _ => None,
    }
}

/// Parses a native `/plp` or `/lookup` response, or a Solr JSON response.
/// `sort_field` names the Solr field holding the sort value (class C).
pub fn parse(body: &str, solr: bool, sort_field: Option<&str>) -> Result<Observed, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("bad json: {e}"))?;
    if v.get("error").is_some() {
        return Err(format!("error response: {}", v["error"]));
    }
    let mut out = Observed::default();
    if solr {
        let resp = v.get("response").ok_or("solr: no response")?;
        out.num_found = resp["numFound"].as_u64();
        for d in resp["docs"].as_array().into_iter().flatten() {
            out.ids
                .push(d["id"].as_str().unwrap_or_default().to_owned());
            if let Some(f) = sort_field {
                out.sort_values.push(d.get(f).and_then(as_f64));
            }
        }
        if let Some(obj) = v.get("facets").and_then(|f| f.as_object()) {
            for (field, value) in obj.iter().filter(|(k, _)| *k != "count") {
                let mut counts = BTreeMap::new();
                for b in value["buckets"].as_array().into_iter().flatten() {
                    let key = match &b["val"] {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    counts.insert(key, b["count"].as_u64().unwrap_or(0));
                }
                out.facets.insert(field.clone(), counts);
            }
        }
    } else {
        out.num_found = v["num_found"].as_u64();
        for d in v["docs"].as_array().into_iter().flatten() {
            out.ids
                .push(d["id"].as_str().unwrap_or_default().to_owned());
            out.sort_values.push(d.get("sort_value").and_then(as_f64));
        }
        if let Some(obj) = v.get("facets").and_then(|f| f.as_object()) {
            for (field, counts) in obj {
                let map = counts
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(k, c)| (k.clone(), c.as_u64().unwrap_or(0)))
                            .collect()
                    })
                    .unwrap_or_default();
                out.facets.insert(field.clone(), map);
            }
        }
    }
    Ok(out)
}

/// Full comparison (validation pre-pass). Returns the first difference.
pub fn check_full(expect: &Expect, got: &Observed) -> Result<(), String> {
    if let Some(n) = expect.num_found {
        if got.num_found != Some(n) {
            return Err(format!("num_found {:?} != {n}", got.num_found));
        }
    }
    if let Some(h) = expect.hit_count {
        if got.ids.len() as u64 != h {
            return Err(format!("hit_count {} != {h}", got.ids.len()));
        }
    }
    if let Some(ids) = &expect.ids {
        if &got.ids != ids {
            return Err(format!(
                "ids differ (first {:?} vs {:?})",
                got.ids.first(),
                ids.first()
            ));
        }
    }
    if let Some(values) = &expect.sort_values {
        // Numeric equality (-0.0 == 0.0); tie order is engine-defined.
        let same = values.len() == got.sort_values.len()
            && values
                .iter()
                .zip(&got.sort_values)
                .all(|(a, b)| match (a, b) {
                    (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * x.abs().max(1.0),
                    (None, None) => true,
                    _ => false,
                });
        if !same {
            return Err("sort-value sequence differs".to_owned());
        }
    }
    if let Some(facets) = &expect.facets {
        let empty = BTreeMap::new();
        for (field, want) in facets {
            let have = got.facets.get(field).unwrap_or(&empty);
            if have != want {
                return Err(format!(
                    "facet {field} differs ({} vs {} values)",
                    have.len(),
                    want.len()
                ));
            }
        }
        if got.facets.keys().any(|k| !facets.contains_key(k)) {
            return Err("unexpected facet field".to_owned());
        }
    }
    Ok(())
}

/// In-load check: `num_found`, plus the first id for exact lookup (A).
pub fn check_fast(class: &str, expect: &Expect, body: &str, solr: bool) -> bool {
    let Ok(got) = parse(body, solr, None) else {
        return false;
    };
    if expect.num_found.is_some() && got.num_found != expect.num_found {
        return false;
    }
    if class == "A" {
        return expect.ids.as_ref().and_then(|i| i.first()) == got.ids.first();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_solr_normalize_to_the_same_observation() {
        let native = r#"{"num_found":3,"docs":[{"id":"a","sort_value":4.5},{"id":"b","sort_value":4.0}],
                         "facets":{"color":{"black":2,"white":1}},"backend_requests":1,"diag":{}}"#;
        let solr = r#"{"response":{"numFound":3,"docs":[{"id":"a","average_rating":4.5},{"id":"b","average_rating":4.0}]},
                       "facets":{"count":3,"color":{"buckets":[{"val":"black","count":2},{"val":"white","count":1}]}}}"#;
        let n = parse(native, false, None).unwrap();
        let s = parse(solr, true, Some("average_rating")).unwrap();
        assert_eq!(n, s);
        let expect = Expect {
            num_found: Some(3),
            hit_count: Some(2),
            ids: None,
            sort_values: Some(vec![Some(4.5), Some(4.0)]),
            facets: Some(HashMap::from([(
                "color".to_owned(),
                BTreeMap::from([("black".to_owned(), 2), ("white".to_owned(), 1)]),
            )])),
        };
        assert!(check_full(&expect, &n).is_ok());
        assert!(check_full(&expect, &s).is_ok());
        let mut wrong = n.clone();
        wrong
            .facets
            .get_mut("color")
            .unwrap()
            .insert("red".into(), 1);
        assert!(check_full(&expect, &wrong).is_err());
        let mut short = n;
        short.ids.pop();
        assert!(check_full(&expect, &short).is_err());
    }

    #[test]
    fn fast_check_catches_count_and_lookup_errors() {
        let e = Expect {
            num_found: Some(1),
            ids: Some(vec!["x".into()]),
            ..Expect::default()
        };
        assert!(check_fast(
            "A",
            &e,
            r#"{"num_found":1,"docs":[{"id":"x"}]}"#,
            false
        ));
        assert!(!check_fast(
            "A",
            &e,
            r#"{"num_found":1,"docs":[{"id":"y"}]}"#,
            false
        ));
        assert!(!check_fast(
            "A",
            &e,
            r#"{"response":{"numFound":0,"docs":[]}}"#,
            true
        ));
        assert!(!check_fast("B", &e, "not json", false));
        assert!(!check_fast("B", &e, r#"{"error":"boom"}"#, false));
    }
}
