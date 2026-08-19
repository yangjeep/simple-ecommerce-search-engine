use super::profile::CatalogProfile;

/// Generate shopper-like query strings from a [`CatalogProfile`] using a
/// fixed set of templates and the profile's own deduplicated vocabulary —
/// no randomness, no model call, one call for the whole profile rather
/// than per SKU. Iteration is over sorted (`BTreeMap`/`BTreeSet`-backed)
/// profile data, so the output order is deterministic across runs.
pub fn generate_shopper_queries(profile: &CatalogProfile) -> Vec<String> {
    let mut queries = Vec::new();
    let sizes = profile.numeric_values("size").to_vec();
    let price_threshold_dollars = profile.max_price_cents().map(|cents| (cents + 100) / 100);

    for product_type in profile.product_type_names() {
        for brand in profile.brand_names() {
            queries.push(format!("{brand} {product_type}"));
        }
        for value in profile.enum_value_keys() {
            queries.push(format!("{value} {product_type}"));
        }
        for attribute in profile.boolean_attributes() {
            queries.push(format!("{attribute} {product_type}"));
        }
        for size in &sizes {
            queries.push(format!("{product_type} size {size}"));
        }
        if let Some(dollars) = price_threshold_dollars {
            queries.push(format!("{product_type} under ${dollars}"));
        }
    }
    queries
}
