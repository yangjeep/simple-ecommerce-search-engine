use std::collections::BTreeMap;

/// Typed attribute value. Every commerce-specific fact that is not already a
/// first-class domain field (id, price, inventory, ...) is expressed as one
/// of these five kinds rather than as an untyped string or JSON blob.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    Enum(String),
    MultiEnum(Vec<String>),
    Boolean(bool),
    Numeric(f64),
    Text(String),
}

/// Attribute name -> value, keyed by a `BTreeMap` so iteration order (and
/// therefore any derived hashing/serialization) is deterministic across runs.
pub type AttributeMap = BTreeMap<String, AttributeValue>;

pub fn attributes(pairs: impl IntoIterator<Item = (&'static str, AttributeValue)>) -> AttributeMap {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
