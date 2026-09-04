use crate::{FrozenQuery, LoadedDataset};
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;

pub fn frozen_native_query(query: &FrozenQuery) -> Result<&str, String> {
    query
        .native
        .as_ref()
        .map(|request| request.q.as_str())
        .ok_or_else(|| {
            format!(
                "query {} is missing its native request block",
                query.query_id
            )
        })
}

/// Executes the complete native candidate path used by Issue #61.
///
/// # Errors
/// Returns an error when a native product has no source-dataset identifier.
pub fn native_candidate_ids(
    data: &LoadedDataset,
    index: &CatalogIndex,
    query_text: &str,
) -> Result<Vec<String>, String> {
    let query = compile(query_text, &data.lexicon);
    let lexical = index.lexical_and_candidates(&query.residual_lexical);
    let hits = if query.residual_lexical.is_empty() {
        index.execute(&query, &data.catalog)
    } else {
        index
            .execute_ranked_narrowed_by(
                &query,
                &lexical,
                &data.catalog,
                data.catalog.products.len(),
            )
            .into_iter()
            .map(|hit| (hit.product, hit.variant))
            .collect()
    };
    hits.into_iter()
        .map(|(product, _)| {
            data.source_id_by_product
                .get(&product)
                .cloned()
                .ok_or_else(|| format!("missing source id for product {}", product.0))
        })
        .collect()
}
