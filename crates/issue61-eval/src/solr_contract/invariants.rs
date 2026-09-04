use super::{ContractError, SolrDataset};
use serde_json::Value;

struct ExpectedField<'a> {
    name: &'a str,
    strings: &'a [(&'a str, &'a str)],
    booleans: &'a [(&'a str, bool)],
}

pub(super) fn validate_schema_invariants(
    dataset: SolrDataset,
    schema: &Value,
) -> Result<(), ContractError> {
    require_value(
        schema.get("uniqueKey"),
        &Value::String("id".into()),
        "uniqueKey",
    )?;
    let fields = schema.get("fields").and_then(Value::as_array);
    require_field(
        fields,
        ExpectedField {
            name: "id",
            strings: &[("type", "string")],
            booleans: &[("indexed", true), ("stored", true), ("multiValued", false)],
        },
    )?;
    for name in dataset.lexical_fields() {
        require_field(
            fields,
            ExpectedField {
                name,
                strings: &[("type", "native_lexical")],
                booleans: &[("indexed", true)],
            },
        )?;
    }
    for name in dataset.companion_fields() {
        require_field(
            fields,
            ExpectedField {
                name,
                strings: &[("type", "string_lc")],
                booleans: &[("indexed", true), ("stored", false), ("multiValued", false)],
            },
        )?;
    }
    validate_native_lexical(schema)?;
    validate_string_lc(schema)?;
    let copy_fields = schema.get("copyFields").and_then(Value::as_array);
    for (source, dest) in dataset.copy_fields() {
        let exists = copy_fields.is_some_and(|items| {
            items.iter().any(|item| {
                item.get("source").and_then(Value::as_str) == Some(source)
                    && item.get("dest").and_then(Value::as_str) == Some(dest)
            })
        });
        if !exists {
            return invariant(
                format!("copyField {source}->{dest}"),
                "pair to exist",
                "missing",
            );
        }
    }
    Ok(())
}

fn validate_native_lexical(schema: &Value) -> Result<(), ContractError> {
    let root = schema
        .get("fieldTypes")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some("native_lexical"))
        })
        .ok_or_else(|| ContractError::Invariant {
            path: "fieldType native_lexical".into(),
            expected: "field type to exist".into(),
            actual: "missing".into(),
        })?;
    for (pointer, expected, path) in [
        ("/class", "solr.TextField", "native_lexical class"),
        (
            "/indexAnalyzer/tokenizer/class",
            "solr.PatternTokenizerFactory",
            "native_lexical indexAnalyzer.tokenizer.class",
        ),
        (
            "/indexAnalyzer/tokenizer/pattern",
            "[^\\p{L}\\p{N}]+",
            "native_lexical indexAnalyzer.tokenizer.pattern",
        ),
        (
            "/queryAnalyzer/tokenizer/class",
            "solr.WhitespaceTokenizerFactory",
            "native_lexical queryAnalyzer.tokenizer.class",
        ),
    ] {
        require_value(root.pointer(pointer), &Value::String(expected.into()), path)?;
    }
    let lowercase = serde_json::json!([{"class": "solr.LowerCaseFilterFactory"}]);
    require_value(
        root.pointer("/indexAnalyzer/filters"),
        &lowercase,
        "native_lexical indexAnalyzer.filters",
    )?;
    require_value(
        root.pointer("/queryAnalyzer/filters"),
        &lowercase,
        "native_lexical queryAnalyzer.filters",
    )
}

fn require_field(
    fields: Option<&Vec<Value>>,
    expected_field: ExpectedField<'_>,
) -> Result<(), ContractError> {
    let name = expected_field.name;
    let field = fields
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        })
        .ok_or_else(|| ContractError::Invariant {
            path: format!("field {name}"),
            expected: "field to exist".into(),
            actual: "missing".into(),
        })?;
    for (key, expected) in expected_field.strings {
        require_value(
            field.get(*key),
            &Value::String((*expected).into()),
            &format!("field {name}.{key}"),
        )?;
    }
    for (key, expected) in expected_field.booleans {
        require_value(
            field.get(*key),
            &Value::Bool(*expected),
            &format!("field {name}.{key}"),
        )?;
    }
    Ok(())
}

fn validate_string_lc(schema: &Value) -> Result<(), ContractError> {
    let field_type = schema
        .get("fieldTypes")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some("string_lc"))
        });
    let root = field_type.ok_or_else(|| ContractError::Invariant {
        path: "fieldType string_lc".into(),
        expected: "field type to exist".into(),
        actual: "missing".into(),
    })?;
    require_value(
        root.get("class"),
        &Value::String("solr.TextField".into()),
        "string_lc class",
    )?;
    require_value(
        root.pointer("/analyzer/tokenizer/class"),
        &Value::String("solr.KeywordTokenizerFactory".into()),
        "string_lc analyzer.tokenizer.class",
    )?;
    let expected = serde_json::json!([{"class": "solr.LowerCaseFilterFactory"}]);
    require_value(
        root.pointer("/analyzer/filters"),
        &expected,
        "string_lc analyzer.filters",
    )
}

pub(super) fn validate_config_invariants(config: &Value) -> Result<(), ContractError> {
    if let Some(value) = config.get("znodeVersion") {
        return invariant(
            "config.znodeVersion".into(),
            "omitted from frozen snapshot",
            value.to_string(),
        );
    }
    for (cache, values) in [
        ("filterCache", [4096, 4096, 0]),
        ("queryResultCache", [0, 0, 0]),
        ("documentCache", [4096, 4096, 0]),
    ] {
        for (property, expected) in ["size", "initialSize", "autowarmCount"]
            .into_iter()
            .zip(values)
        {
            let path = format!("query.{cache}.{property}");
            let value = config.pointer(&format!("/query/{cache}/{property}"));
            if value.and_then(semantic_integer) != Some(expected) {
                return invariant(path, format!("integer {expected}"), display_value(value));
            }
        }
    }
    Ok(())
}

fn semantic_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse().ok(),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

fn require_value(
    actual: Option<&Value>,
    expected: &Value,
    path: &str,
) -> Result<(), ContractError> {
    if actual == Some(expected) {
        return Ok(());
    }
    invariant(
        path.to_string(),
        expected.to_string(),
        display_value(actual),
    )
}

fn display_value(value: Option<&Value>) -> String {
    value.map_or_else(|| "missing".into(), Value::to_string)
}

fn invariant<T>(
    path: String,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> Result<T, ContractError> {
    Err(ContractError::Invariant {
        path,
        expected: expected.into(),
        actual: actual.into(),
    })
}
