//! The deterministic multi-variant expansion for the same-variant
//! conjunction primitive (amendment 1, section 4). WANDS has one variant
//! per product, so a same-variant constraint cannot be exercised on it
//! directly. Each input product keeps its product-level fields and gets
//! exactly [`VARIANTS_PER_PRODUCT`] variants whose variant-level
//! `variant_color`/`variant_size` are placed so that many products carry the
//! queried color on one variant and the queried size on another, but never
//! both on the same variant: a product-level (flattened) match would return
//! them, and a correct same-variant conjunction must not.

use commerce_core::domain::{
    attributes, AttributeValue, Catalog, Inventory, Price, Product, Variant, VariantId,
};

pub const VARIANTS_PER_PRODUCT: u64 = 4;
pub const COLORS: [&str; 8] = [
    "red", "blue", "green", "black", "white", "grey", "brown", "beige",
];
pub const SIZES: [&str; 4] = ["S", "M", "L", "XL"];
/// The queried pair.
pub const QUERY_COLOR: &str = "red";
pub const QUERY_SIZE: &str = "S";

/// Variant `i` of product index `p`: color `COLORS[(p + i) % 8]`, size
/// `SIZES[(p + 2i + p/8) % 4]` -- two sizes per product, alternating, so
/// color and size co-occur on the same variant for only some products.
#[must_use]
pub fn variant_attrs(p: u64, i: u64) -> (&'static str, &'static str) {
    (
        COLORS[((p + i) % 8) as usize],
        SIZES[((p + 2 * i + p / 8) % 4) as usize],
    )
}

#[must_use]
pub fn expand(catalog: &Catalog) -> Catalog {
    let products = catalog
        .products
        .iter()
        .enumerate()
        .map(|(p, product)| {
            let template = product.variants.first();
            let variants = (0..VARIANTS_PER_PRODUCT)
                .map(|i| {
                    let (color, size) = variant_attrs(p as u64, i);
                    Variant {
                        id: VariantId(p as u64 * VARIANTS_PER_PRODUCT + i),
                        attributes: attributes([
                            ("variant_color", AttributeValue::Enum(color.to_owned())),
                            ("variant_size", AttributeValue::Enum(size.to_owned())),
                        ]),
                        price: template.map_or(Price::usd(0), |v| v.price),
                        inventory: template.map_or(Inventory::in_stock(1), |v| v.inventory),
                    }
                })
                .collect();
            Product {
                id: product.id,
                product_type: product.product_type,
                brand: product.brand,
                category: product.category,
                title: product.title.clone(),
                attributes: product.attributes.clone(),
                variants,
            }
        })
        .collect();
    Catalog { products }
}

/// `(same-variant matches, trap products)` by linear scan: variants that
/// carry both values, and products that carry the color on one variant and
/// the size on another but on no single variant.
#[must_use]
pub fn oracle(catalog: &Catalog) -> (Vec<VariantId>, usize) {
    let is = |v: &Variant, attr: &str, want: &str| {
        matches!(v.attributes.get(attr), Some(AttributeValue::Enum(x)) if x == want)
    };
    let mut matches = Vec::new();
    let mut traps = 0;
    for product in &catalog.products {
        let mut any_color = false;
        let mut any_size = false;
        let mut both = false;
        for v in &product.variants {
            let c = is(v, "variant_color", QUERY_COLOR);
            let s = is(v, "variant_size", QUERY_SIZE);
            any_color |= c;
            any_size |= s;
            if c && s {
                both = true;
                matches.push(v.id);
            }
        }
        if any_color && any_size && !both {
            traps += 1;
        }
    }
    (matches, traps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{BrandId, CategoryId, ProductId, ProductTypeId};
    use commerce_core::index::CatalogIndex;
    use commerce_core::ir::ResolvedConstraint;

    fn base(n: u64) -> Catalog {
        Catalog {
            products: (0..n)
                .map(|p| Product {
                    id: ProductId(p),
                    product_type: ProductTypeId(1),
                    brand: BrandId(1),
                    category: CategoryId(1),
                    title: format!("p{p}"),
                    attributes: attributes([]),
                    variants: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn expansion_has_traps_and_the_index_returns_only_same_variant_matches() {
        let catalog = expand(&base(64));
        let (matches, traps) = oracle(&catalog);
        assert!(!matches.is_empty(), "the query must have real matches");
        assert!(traps > 0, "the fixture must contain cross-variant traps");
        let index = CatalogIndex::build(&catalog);
        let constraints = [("variant_color", QUERY_COLOR), ("variant_size", QUERY_SIZE)]
            .map(|(a, v)| {
                ResolvedConstraint::Attribute(commerce_core::domain::Constraint::Enum {
                    attribute: a.to_owned(),
                    value: v.to_owned(),
                })
            });
        let hits: Vec<VariantId> = index
            .indexed_candidates(&constraints)
            .iter()
            .map(|o| index.variant_id_at(o).unwrap())
            .collect();
        assert_eq!(hits, matches);
    }
}
