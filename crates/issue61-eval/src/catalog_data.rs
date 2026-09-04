use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::{
    attributes, AttributeValue, Brand, BrandId, Catalog, CategoryId, Inventory, Price, Product,
    ProductId, ProductTypeId, Variant, VariantId,
};
use commerce_core::ir::SemanticLexicon;
use comparator_eval::translate::StructuralNames;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dataset {
    Wands,
    EsciElectronics,
}

impl Dataset {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "wands" => Ok(Self::Wands),
            "esci_electronics" => Ok(Self::EsciElectronics),
            other => Err(format!("unsupported dataset {other:?}")),
        }
    }
}

pub struct LoadedDataset {
    pub catalog: Catalog,
    pub lexicon: SemanticLexicon,
    pub source_id_by_product: HashMap<ProductId, String>,
    brand_name_by_id: HashMap<BrandId, String>,
    category_name_by_id: HashMap<CategoryId, String>,
    product_type_name_by_id: HashMap<ProductTypeId, String>,
}

impl StructuralNames for LoadedDataset {
    fn brand_name(&self, id: BrandId) -> Option<&str> {
        self.brand_name_by_id.get(&id).map(String::as_str)
    }

    fn product_type_name(&self, id: ProductTypeId) -> Option<&str> {
        self.product_type_name_by_id.get(&id).map(String::as_str)
    }

    fn category_name(&self, id: CategoryId) -> Option<&str> {
        self.category_name_by_id.get(&id).map(String::as_str)
    }
}

pub fn load_dataset(path: &Path, dataset: Dataset) -> Result<LoadedDataset, String> {
    match dataset {
        Dataset::Wands => load_wands(path),
        Dataset::EsciElectronics => load_esci(path),
    }
}

fn load_wands(path: &Path) -> Result<LoadedDataset, String> {
    let products = phase6a_eval::data::load_catalog(path);
    let ingested = phase6a_eval::catalog::build_catalog(&products);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );
    let source_id_by_product = ingested
        .wands_id_to_product_id
        .iter()
        .map(|(source, product)| (*product, source.clone()))
        .collect();
    Ok(LoadedDataset {
        lexicon: compile_lexicon(&profile, 1),
        catalog: ingested.catalog,
        source_id_by_product,
        brand_name_by_id: HashMap::new(),
        category_name_by_id: ingested
            .categories
            .into_iter()
            .map(|category| (category.id, category.name))
            .collect(),
        product_type_name_by_id: ingested
            .product_types
            .into_iter()
            .map(|product_type| (product_type.id, product_type.name))
            .collect(),
    })
}

#[derive(Deserialize)]
struct EsciProduct {
    product_id: String,
    title: String,
    description: String,
    bullet_point: String,
    brand: String,
    color: String,
}

fn load_esci(path: &Path) -> Result<LoadedDataset, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let products: Vec<EsciProduct> = content
        .lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .map_err(|error| format!("parse {} line {}: {error}", path.display(), index + 1))
        })
        .collect::<Result<_, _>>()?;
    let mut brands = Vec::new();
    let mut brand_ids = HashMap::new();
    let mut catalog_products = Vec::with_capacity(products.len());
    let mut source_id_by_product = HashMap::with_capacity(products.len());
    for (index, source) in products.into_iter().enumerate() {
        let brand = if source.brand.trim().is_empty() {
            BrandId(0)
        } else {
            let next = BrandId(u32::try_from(brands.len() + 1).map_err(|error| error.to_string())?);
            *brand_ids.entry(source.brand.clone()).or_insert_with(|| {
                brands.push(Brand {
                    id: next,
                    name: source.brand.clone(),
                });
                next
            })
        };
        let id_value = u64::try_from(index + 1).map_err(|error| error.to_string())?;
        let product_id = ProductId(id_value);
        let mut product_attributes = vec![
            ("description", AttributeValue::Text(source.description)),
            ("bullet_point", AttributeValue::Text(source.bullet_point)),
        ];
        if !source.color.trim().is_empty() {
            product_attributes.push(("color", AttributeValue::Enum(source.color)));
        }
        catalog_products.push(Product {
            id: product_id,
            product_type: ProductTypeId(0),
            brand,
            category: CategoryId(0),
            title: source.title,
            attributes: attributes(product_attributes),
            variants: vec![Variant {
                id: VariantId(id_value),
                attributes: attributes([]),
                price: Price::usd(0),
                inventory: Inventory::in_stock(1),
            }],
        });
        source_id_by_product.insert(product_id, source.product_id);
    }
    let catalog = Catalog {
        products: catalog_products,
    };
    let profile = CatalogProfile::build(&catalog, &brands, &[], &[]);
    let brand_name_by_id = brands
        .into_iter()
        .map(|brand| (brand.id, brand.name))
        .collect();
    Ok(LoadedDataset {
        lexicon: compile_lexicon(&profile, 1),
        catalog,
        source_id_by_product,
        brand_name_by_id,
        category_name_by_id: HashMap::new(),
        product_type_name_by_id: HashMap::new(),
    })
}
