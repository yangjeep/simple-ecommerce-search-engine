//! Converts a `&[SynthProduct]` into a real `commerce_core::domain::Catalog`
//! -- the same registry-assignment pattern
//! `phase6a_eval::catalog::build_catalog` already uses for real WANDS
//! records (deterministic encounter-order `ProductId`/`VariantId`
//! assignment, a `HashMap`-backed name->id registry per typed field).

use std::collections::HashMap;

use commerce_core::domain::{
    Brand, BrandId, Catalog, Category, CategoryId, Inventory, Price, Product, ProductId,
    ProductType, ProductTypeId, Variant, VariantId,
};

use crate::SynthProduct;

pub struct IngestedSynthCatalog {
    pub catalog: Catalog,
    pub external_product_id: HashMap<String, ProductId>,
    pub external_variant_id: HashMap<String, (ProductId, VariantId)>,
    pub product_types: Vec<ProductType>,
    pub categories: Vec<Category>,
    pub brands: Vec<Brand>,
}

pub fn build_catalog(products: &[SynthProduct]) -> IngestedSynthCatalog {
    let mut product_type_ids: HashMap<String, ProductTypeId> = HashMap::new();
    let mut product_types: Vec<ProductType> = Vec::new();
    let mut category_ids: HashMap<String, CategoryId> = HashMap::new();
    let mut categories: Vec<Category> = Vec::new();
    let mut brand_ids: HashMap<String, BrandId> = HashMap::new();
    let mut brands: Vec<Brand> = Vec::new();

    let mut external_product_id = HashMap::with_capacity(products.len());
    let mut external_variant_id = HashMap::new();
    let mut catalog_products = Vec::with_capacity(products.len());

    for (p_ord, p) in products.iter().enumerate() {
        let product_id = ProductId(p_ord as u64);
        external_product_id.insert(p.external_id.clone(), product_id);

        let product_type = *product_type_ids
            .entry(p.product_type.clone())
            .or_insert_with(|| {
                let id = ProductTypeId(product_types.len() as u32 + 1);
                product_types.push(ProductType {
                    id,
                    name: p.product_type.clone(),
                });
                id
            });
        let category = *category_ids
            .entry(p.category_path.clone())
            .or_insert_with(|| {
                let id = CategoryId(categories.len() as u32 + 1);
                categories.push(Category {
                    id,
                    name: p.category_path.clone(),
                });
                id
            });
        let brand = *brand_ids.entry(p.brand.clone()).or_insert_with(|| {
            let id = BrandId(brands.len() as u32 + 1);
            brands.push(Brand {
                id,
                name: p.brand.clone(),
            });
            id
        });

        let mut variants = Vec::with_capacity(p.variants.len());
        for (v_ord, v) in p.variants.iter().enumerate() {
            let variant_id = VariantId((p_ord as u64) * 1000 + v_ord as u64);
            external_variant_id.insert(v.external_id.clone(), (product_id, variant_id));
            variants.push(Variant {
                id: variant_id,
                attributes: attributes_from_pairs(&v.variant_attributes),
                price: Price::usd(v.price_cents),
                inventory: Inventory::in_stock(10),
            });
        }

        catalog_products.push(Product {
            id: product_id,
            product_type,
            brand,
            category,
            title: p.title.clone(),
            attributes: attributes_from_pairs(&p.product_attributes),
            variants,
        });
    }

    IngestedSynthCatalog {
        catalog: Catalog {
            products: catalog_products,
        },
        external_product_id,
        external_variant_id,
        product_types,
        categories,
        brands,
    }
}

fn attributes_from_pairs(
    pairs: &[(String, commerce_core::domain::AttributeValue)],
) -> commerce_core::domain::AttributeMap {
    pairs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}
