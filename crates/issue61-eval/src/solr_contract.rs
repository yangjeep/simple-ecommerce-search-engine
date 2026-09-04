use serde_json::Value;
use std::error::Error;
use std::fmt;

mod invariants;

use invariants::{validate_config_invariants, validate_schema_invariants};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolrDataset {
    Wands,
    EsciElectronics,
}

impl SolrDataset {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "wands" => Ok(Self::Wands),
            "esci_electronics" => Ok(Self::EsciElectronics),
            other => Err(ContractError::UnknownDataset(other.to_string())),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Wands => "wands",
            Self::EsciElectronics => "esci_electronics",
        }
    }

    pub const fn core_name(self) -> &'static str {
        match self {
            Self::Wands => "i61_wands",
            Self::EsciElectronics => "i61_esci_electronics",
        }
    }

    pub const fn schema_snapshot(self) -> &'static str {
        match self {
            Self::Wands => "solr_wands_schema.json",
            Self::EsciElectronics => "solr_esci_electronics_schema.json",
        }
    }

    pub const fn config_snapshot(self) -> &'static str {
        match self {
            Self::Wands => "solr_wands_config.json",
            Self::EsciElectronics => "solr_esci_electronics_config.json",
        }
    }

    const fn lexical_fields(self) -> &'static [&'static str] {
        match self {
            Self::Wands => &["title", "description"],
            Self::EsciElectronics => &["title", "description", "bullet_point"],
        }
    }

    const fn companion_fields(self) -> &'static [&'static str] {
        match self {
            Self::Wands => &["product_class_lc", "category_leaf_lc"],
            Self::EsciElectronics => &["brand_lc", "color_lc"],
        }
    }

    const fn copy_fields(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Wands => &[
                ("product_class", "product_class_lc"),
                ("category_leaf", "category_leaf_lc"),
            ],
            Self::EsciElectronics => &[("brand", "brand_lc"), ("color", "color_lc")],
        }
    }
}

pub struct ContractDocuments<'a> {
    pub live_schema_envelope: &'a Value,
    pub live_config_envelope: &'a Value,
    pub expected_schema: &'a Value,
    pub expected_config: &'a Value,
}

struct SnapshotTarget {
    kind: &'static str,
    filename: &'static str,
}

#[derive(Debug)]
pub enum ContractError {
    UnknownDataset(String),
    MissingEnvelopeObject(&'static str),
    Invariant {
        path: String,
        expected: String,
        actual: String,
    },
    Drift {
        kind: &'static str,
        snapshot: &'static str,
        first_difference: String,
    },
}

impl fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDataset(value) => write!(
                formatter,
                "unknown dataset {value:?} (expected: wands | esci_electronics)"
            ),
            Self::MissingEnvelopeObject(endpoint) => {
                write!(
                    formatter,
                    "live {endpoint} response missing .{} object",
                    &endpoint[1..]
                )
            }
            Self::Invariant {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "E1 invariant failed at {path}: expected {expected}, got {actual}"
            ),
            Self::Drift {
                kind,
                snapshot,
                first_difference,
            } => write!(
                formatter,
                "Solr {kind} drift from {snapshot}; first difference at {first_difference}"
            ),
        }
    }
}

impl Error for ContractError {}

pub fn validate_contract(
    dataset: SolrDataset,
    documents: ContractDocuments<'_>,
) -> Result<(), ContractError> {
    validate_schema_invariants(dataset, documents.expected_schema)?;
    validate_config_invariants(documents.expected_config)?;
    let live_schema = envelope_object(documents.live_schema_envelope, "schema", "/schema")?;
    let live_config = envelope_object(documents.live_config_envelope, "config", "/config")?;
    let normalized_live_config = live_config_without_znode_version(live_config)?;
    require_exact(
        SnapshotTarget {
            kind: "schema",
            filename: dataset.schema_snapshot(),
        },
        documents.expected_schema,
        live_schema,
    )?;
    require_exact(
        SnapshotTarget {
            kind: "config",
            filename: dataset.config_snapshot(),
        },
        documents.expected_config,
        &normalized_live_config,
    )
}

fn live_config_without_znode_version(config: &Value) -> Result<Value, ContractError> {
    let mut normalized = config.clone();
    let Value::Object(properties) = &mut normalized else {
        return Err(ContractError::MissingEnvelopeObject("/config"));
    };
    properties.remove("znodeVersion");
    Ok(normalized)
}

fn envelope_object<'a>(
    envelope: &'a Value,
    key: &str,
    endpoint: &'static str,
) -> Result<&'a Value, ContractError> {
    envelope
        .get(key)
        .filter(|value| value.is_object())
        .ok_or(ContractError::MissingEnvelopeObject(endpoint))
}

fn require_exact(
    target: SnapshotTarget,
    expected: &Value,
    actual: &Value,
) -> Result<(), ContractError> {
    if expected == actual {
        return Ok(());
    }
    Err(ContractError::Drift {
        kind: target.kind,
        snapshot: target.filename,
        first_difference: first_difference(expected, actual, "$"),
    })
}

fn first_difference(expected: &Value, actual: &Value, path: &str) -> String {
    match (expected, actual) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, value) in left {
                let child = format!("{path}.{key}");
                match right.get(key) {
                    Some(other) if value != other => return first_difference(value, other, &child),
                    None => return format!("{child} (missing live key)"),
                    Some(_) => {}
                }
            }
            right
                .keys()
                .find(|key| !left.contains_key(*key))
                .map_or_else(
                    || path.to_string(),
                    |key| format!("{path}.{key} (extra live key)"),
                )
        }
        (Value::Array(left), Value::Array(right)) => {
            for (index, (value, other)) in left.iter().zip(right).enumerate() {
                if value != other {
                    return first_difference(value, other, &format!("{path}[{index}]"));
                }
            }
            format!(
                "{path}.length (expected {}, got {})",
                left.len(),
                right.len()
            )
        }
        _ => format!("{path} (expected {expected}, got {actual})"),
    }
}

#[cfg(test)]
#[path = "solr_contract/tests.rs"]
mod tests;
