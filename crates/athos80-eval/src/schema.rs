use commerce_core::domain::*;
use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogInput {
    pub products: Vec<ProductInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductInput {
    pub product_id: u64,
    pub title: String,
    pub brand_id: u32,
    pub category_id: u32,
    pub product_type_id: u32,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
    pub variants: Vec<VariantInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantInput {
    pub variant_id: u64,
    pub price_cents: i64,
    pub available_units: u32,
    pub in_stock: bool,
    #[serde(default)]
    pub attributes: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Enum(String),
    MultiEnum(Vec<String>),
    Boolean(bool),
    Numeric(f64),
    Text(String),
}
impl Value {
    pub(crate) fn native(&self) -> AttributeValue {
        match self {
            Self::Enum(v) => AttributeValue::Enum(v.clone()),
            Self::MultiEnum(v) => AttributeValue::MultiEnum(v.clone()),
            Self::Boolean(v) => AttributeValue::Boolean(*v),
            Self::Numeric(v) => AttributeValue::Numeric(*v),
            Self::Text(v) => AttributeValue::Text(v.clone()),
        }
    }
}
impl CatalogInput {
    pub(crate) fn native(&self) -> Catalog {
        let attrs =
            |m: &BTreeMap<String, Value>| m.iter().map(|(k, v)| (k.clone(), v.native())).collect();
        Catalog {
            products: self
                .products
                .iter()
                .map(|p| Product {
                    id: ProductId(p.product_id),
                    title: p.title.clone(),
                    brand: BrandId(p.brand_id),
                    category: CategoryId(p.category_id),
                    product_type: ProductTypeId(p.product_type_id),
                    attributes: attrs(&p.attributes),
                    variants: p
                        .variants
                        .iter()
                        .map(|v| Variant {
                            id: VariantId(v.variant_id),
                            price: Price::usd(v.price_cents),
                            inventory: Inventory {
                                available_units: v.available_units,
                                status: if v.in_stock {
                                    Availability::InStock
                                } else {
                                    Availability::OutOfStock
                                },
                            },
                            attributes: attrs(&v.attributes),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Filter {
    Enum {
        attribute: String,
        value: String,
    },
    MultiEnumContains {
        attribute: String,
        value: String,
    },
    Boolean {
        attribute: String,
        value: bool,
    },
    Numeric {
        attribute: String,
        op: NumberOp,
        value: f64,
    },
    TextContains {
        attribute: String,
        value: String,
    },
    Brand {
        value: u32,
    },
    Category {
        value: u32,
    },
    ProductType {
        value: u32,
    },
    PriceUnderCents {
        value: i64,
    },
    PriceOverCents {
        value: i64,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberOp {
    Eq,
    Lt,
    Lte,
    Gt,
    Gte,
}
impl Filter {
    pub(crate) fn native(&self) -> ResolvedConstraint {
        use ResolvedConstraint::{Attribute as A, Structural as S};
        match self {
            Self::Enum { attribute, value } => A(Constraint::Enum {
                attribute: attribute.clone(),
                value: value.clone(),
            }),
            Self::MultiEnumContains { attribute, value } => A(Constraint::MultiEnumContains {
                attribute: attribute.clone(),
                value: value.clone(),
            }),
            Self::Boolean { attribute, value } => A(Constraint::Boolean {
                attribute: attribute.clone(),
                value: *value,
            }),
            Self::Numeric {
                attribute,
                op,
                value,
            } => A(Constraint::Numeric {
                attribute: attribute.clone(),
                op: match op {
                    NumberOp::Eq => NumericOp::Eq,
                    NumberOp::Lt => NumericOp::Lt,
                    NumberOp::Lte => NumericOp::Lte,
                    NumberOp::Gt => NumericOp::Gt,
                    NumberOp::Gte => NumericOp::Gte,
                },
                value: *value,
            }),
            Self::TextContains { attribute, value } => A(Constraint::Text {
                attribute: attribute.clone(),
                contains: value.clone(),
            }),
            Self::Brand { value } => S(StructuralConstraint::Brand(BrandId(*value))),
            Self::Category { value } => S(StructuralConstraint::Category(CategoryId(*value))),
            Self::ProductType { value } => {
                S(StructuralConstraint::ProductType(ProductTypeId(*value)))
            }
            Self::PriceUnderCents { value } => S(StructuralConstraint::PriceUnderCents(*value)),
            Self::PriceOverCents { value } => S(StructuralConstraint::PriceOverCents(*value)),
        }
    }
    pub(crate) fn field(&self) -> FacetField {
        match self {
            Self::Enum { attribute, .. }
            | Self::MultiEnumContains { attribute, .. }
            | Self::Boolean { attribute, .. }
            | Self::Numeric { attribute, .. }
            | Self::TextContains { attribute, .. } => FacetField::Attribute {
                name: attribute.clone(),
            },
            Self::Brand { .. } => FacetField::Brand,
            Self::Category { .. } => FacetField::Category,
            Self::ProductType { .. } => FacetField::ProductType,
            Self::PriceUnderCents { .. } | Self::PriceOverCents { .. } => FacetField::PriceCents,
        }
    }
}
pub(crate) fn query(filters: &[Filter]) -> CommerceQuery {
    CommerceQuery {
        constraints: filters.iter().map(Filter::native).collect(),
        ..CommerceQuery::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultMode {
    #[default]
    Products,
    Variants,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "profile", rename_all = "snake_case", deny_unknown_fields)]
pub enum LexicalInput {
    #[default]
    ExactTokenAnd,
    /// Optional feature; strict Tantivy parser, AND by default; bucket 0.
    Bm25,
    /// Raw storefront text: same analyzer, unique tokens AND; never query DSL.
    Bm25Literal,
    Supplied {
        hits: Vec<LexicalHit>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalHit {
    pub product_id: u64,
    pub variant_id: u64,
    pub score: f64,
    pub relevance_bucket: i32,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub category_id: Option<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    #[default]
    Global,
    Category {
        category_id: u32,
    },
    Query {
        query: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    QueryEquals { query: String },
    QueryContainsAll { tokens: Vec<String> },
    ContextCategory { category_id: u32 },
    ContextTag { tag: String },
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    #[default]
    All,
    Products {
        ids: Vec<u64>,
    },
    Variants {
        ids: Vec<u64>,
    },
    Filters {
        filters: Vec<Filter>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Hide,
    Group {
        #[serde(default)]
        order: u32,
        rank: i32,
    },
    /// Zero-based absolute position, applied before page slicing.
    Pin {
        position: usize,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub target: Target,
    pub action: Action,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonalSignal {
    pub target: Target,
    /// Each factor and the final product are bounded to [0.25, 4].
    /// Final factor is quantized to 12 decimal places after compensated log sum.
    pub factor: f64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Relevance,
    PriceAsc,
    PriceDesc,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortPolicy {
    /// Price sort replaces ranking and pins, but never hard filters/hides/facets.
    #[default]
    ShopperOverrides,
    /// Relevance bucket/group/factor remain first; price is a subsort. Pins apply.
    MerchOverrides,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FacetField {
    Attribute { name: String },
    Brand,
    Category,
    ProductType,
    PriceCents,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacetRequest {
    pub name: String,
    pub field: FacetField,
}
fn yes() -> bool {
    true
}
fn limit_default() -> usize {
    20
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Request {
    pub q: String,
    pub filters: Vec<Filter>,
    #[serde(default = "yes")]
    pub in_stock_only: bool,
    pub result_mode: ResultMode,
    /// Null means follow result_mode; explicit products/variants can differ.
    pub facet_count_mode: Option<ResultMode>,
    pub facets: Vec<FacetRequest>,
    pub lexical: LexicalInput,
    pub context: Context,
    pub rules: Vec<Rule>,
    pub personal_signals: Vec<PersonalSignal>,
    pub sort: Sort,
    pub sort_policy: SortPolicy,
    pub offset: usize,
    #[serde(default = "limit_default")]
    pub limit: usize,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            q: String::new(),
            filters: vec![],
            in_stock_only: true,
            result_mode: ResultMode::Products,
            facet_count_mode: None,
            facets: vec![],
            lexical: LexicalInput::default(),
            context: Context::default(),
            rules: vec![],
            personal_signals: vec![],
            sort: Sort::Relevance,
            sort_policy: SortPolicy::ShopperOverrides,
            offset: 0,
            limit: 20,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub product_id: u64,
    pub selected_variant_id: u64,
    pub matching_variant_count: usize,
    pub lexical_score: f64,
    pub relevance_bucket: i32,
    pub merch_groups: Vec<i32>,
    pub personal_factor: f64,
    pub composed_score: f64,
    pub price_cents: i64,
    pub group_rule_ids: Vec<Option<String>>,
    pub pin_rule_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetResult {
    pub name: String,
    pub counts: BTreeMap<String, usize>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub catalog_variants: usize,
    pub lexical_candidates: usize,
    pub structural_candidates: usize,
    pub verified_variants: usize,
    pub eligible_variants: usize,
    pub eligible_products: usize,
    pub facet_verified_variants: usize,
    pub ranked_variants: usize,
    pub elapsed_micros: u128,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub contract: String,
    pub lexical_profile: String,
    pub result_mode: ResultMode,
    pub facet_count_mode: ResultMode,
    pub num_found: usize,
    pub hits: Vec<Hit>,
    pub facets: Vec<FacetResult>,
    pub active_rule_ids: Vec<String>,
    pub diagnostics: Vec<String>,
    pub metrics: Metrics,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayInput {
    pub catalog: CatalogInput,
    pub requests: Vec<Request>,
}
