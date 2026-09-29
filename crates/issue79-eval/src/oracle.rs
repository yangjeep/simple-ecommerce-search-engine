//! Independent correctness oracle for #79's gate (section 6).
//!
//! Computes the expected `/plp` answer from the [`Catalog`] alone -- a
//! linear scan over per-variant [`effective_attributes`], never any
//! `CatalogIndex` structure, and with its own comparator (not
//! `commerce_core::index::sort::SortKey`) -- so a bug shared by an index
//! structure and the code under test cannot make both agree.
//!
//! Defined semantics (preregistered): filter before facet/sort; enum filter
//! = exact value match; facet counts every `Enum` value (and each
//! `MultiEnum` element) over the candidates with that facet's *own* filters
//! removed (disjunctive self-exclusion), never counting a missing value,
//! omitting zero counts; sort = present values first in both directions,
//! then value asc/desc, then variant ordinal ascending; NaN = missing.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use commerce_core::domain::{
    effective_attributes, AttributeMap, AttributeValue, Catalog, CategoryId, ProductId,
};

use crate::plp::{PlpDoc, PlpRequest};

struct Row {
    product_id: ProductId,
    category: CategoryId,
    attrs: AttributeMap,
}

pub struct Oracle {
    rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub num_found: usize,
    pub candidate_ordinals: Vec<u32>,
    pub facets: HashMap<String, BTreeMap<String, u64>>,
    pub docs: Vec<PlpDoc>,
}

impl Oracle {
    /// Rows in catalog encounter order: row `i` is variant ordinal `i`
    /// (the ordinal assignment `CatalogIndex::build` documents).
    #[must_use]
    pub fn new(catalog: &Catalog) -> Self {
        let mut rows = Vec::new();
        for product in &catalog.products {
            for variant in &product.variants {
                rows.push(Row {
                    product_id: product.id,
                    category: product.category,
                    attrs: effective_attributes(product, variant),
                });
            }
        }
        Oracle { rows }
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Count of rows carrying a NaN numeric value for any field -- the gate
    /// asserts this is zero for WANDS (NaN handling is then vacuous but
    /// still defined).
    #[must_use]
    pub fn nan_numeric_values(&self) -> usize {
        self.rows
            .iter()
            .flat_map(|row| row.attrs.values())
            .filter(|value| matches!(value, AttributeValue::Numeric(v) if v.is_nan()))
            .count()
    }

    fn matches(
        &self,
        row: &Row,
        req: &PlpRequest,
        category: Option<CategoryId>,
        exclude: Option<&str>,
    ) -> bool {
        if let Some(id) = category {
            if row.category != id {
                return false;
            }
        }
        for (attr, value) in &req.filters {
            if Some(attr.as_str()) == exclude {
                continue;
            }
            let ok = match row.attrs.get(attr) {
                Some(AttributeValue::Enum(v)) => v == value,
                Some(AttributeValue::MultiEnum(vs)) => vs.iter().any(|v| v == value),
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        for (attr, op, bound) in &req.ranges {
            let Some(AttributeValue::Numeric(v)) = row.attrs.get(attr) else {
                return false;
            };
            let ok = match op.as_str() {
                "eq" => v == bound,
                "lt" => v < bound,
                "lte" => v <= bound,
                "gt" => v > bound,
                "gte" => v >= bound,
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }

    pub fn expected(
        &self,
        req: &PlpRequest,
        category_id_by_leaf: &HashMap<String, CategoryId>,
        source_id_by_product: &HashMap<ProductId, String>,
    ) -> Result<Expected, String> {
        let category = match &req.category {
            Some(leaf) => Some(
                *category_id_by_leaf
                    .get(leaf)
                    .ok_or_else(|| format!("unknown category_leaf {leaf:?}"))?,
            ),
            None => None,
        };
        let candidate_ordinals: Vec<u32> = (0..self.rows.len())
            .filter(|&i| self.matches(&self.rows[i], req, category, None))
            .map(|i| i as u32)
            .collect();

        let mut facets = HashMap::new();
        for facet in &req.facets {
            let mut counts: BTreeMap<String, u64> = BTreeMap::new();
            for row in &self.rows {
                if !self.matches(row, req, category, Some(facet.as_str())) {
                    continue;
                }
                match row.attrs.get(facet) {
                    Some(AttributeValue::Enum(v)) => *counts.entry(v.clone()).or_insert(0) += 1,
                    Some(AttributeValue::MultiEnum(vs)) => {
                        for v in vs {
                            *counts.entry(v.clone()).or_insert(0) += 1;
                        }
                    }
                    _ => {}
                }
            }
            facets.insert(facet.clone(), counts);
        }

        let doc_for = |ordinal: u32, value: Option<f64>| PlpDoc {
            id: source_id_by_product
                .get(&self.rows[ordinal as usize].product_id)
                .cloned()
                .unwrap_or_else(|| self.rows[ordinal as usize].product_id.0.to_string()),
            sort_value: value,
        };
        let docs = match &req.sort {
            None => candidate_ordinals
                .iter()
                .skip(req.offset)
                .take(req.top_k)
                .map(|&ord| doc_for(ord, None))
                .collect(),
            Some((field, descending)) => {
                let mut keyed: Vec<(u32, Option<f64>)> = candidate_ordinals
                    .iter()
                    .map(|&ord| {
                        let value = match self.rows[ord as usize].attrs.get(field) {
                            Some(AttributeValue::Numeric(v)) if !v.is_nan() => Some(*v),
                            _ => None,
                        };
                        (ord, value)
                    })
                    .collect();
                keyed.sort_by(|a, b| oracle_cmp(*a, *b, *descending));
                keyed
                    .into_iter()
                    .skip(req.offset)
                    .take(req.top_k)
                    .map(|(ord, value)| doc_for(ord, value))
                    .collect()
            }
        };
        Ok(Expected {
            num_found: candidate_ordinals.len(),
            candidate_ordinals,
            facets,
            docs,
        })
    }
}

/// Present before missing (both directions); value asc/desc; ordinal asc.
fn oracle_cmp(a: (u32, Option<f64>), b: (u32, Option<f64>), descending: bool) -> Ordering {
    let by_value = match (a.1, b.1) {
        (Some(x), Some(y)) => {
            let natural = x.partial_cmp(&y).expect("NaN excluded above");
            if descending {
                natural.reverse()
            } else {
                natural
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    by_value.then(a.0.cmp(&b.0))
}
