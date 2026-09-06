use super::error::PostRunError;
use serde::de::{DeserializeOwned, DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use std::collections::BTreeSet;
use std::fmt;

const RAW_NULLABLE_FIELDS: [&str; 7] = [
    "native_cgroup_host_pid",
    "native_pid_namespace",
    "process_cpu_user_usec",
    "process_cpu_system_usec",
    "process_cpu_total_usec",
    "process_cgroup_disagreement_pct",
    "exclusion_reason",
];
const WORKLOAD_NULLABLE_FIELDS: [&str; 2] = ["native", "solr"];
const AUDIT_NULLABLE_FIELDS: [&str; 5] = [
    "native_count",
    "native_digest",
    "engine_count",
    "engine_digest",
    "failure_reason",
];

#[derive(Clone, Copy)]
pub enum JsonlSchema {
    Raw,
    WandsWorkload,
    EsciWorkload,
    WandsAudit,
    EsciAudit,
    IndexArtifacts,
}

impl JsonlSchema {
    pub const fn file(self) -> &'static str {
        match self {
            Self::Raw => "raw.jsonl",
            Self::WandsWorkload => "i61_wands_480.jsonl",
            Self::EsciWorkload => "i61_esci_electronics.jsonl",
            Self::WandsAudit => "candidate_audit_wands.jsonl",
            Self::EsciAudit => "candidate_audit_esci.jsonl",
            Self::IndexArtifacts => "index_artifacts.jsonl",
        }
    }

    const fn required_nullable_fields(self) -> &'static [&'static str] {
        match self {
            Self::Raw => &RAW_NULLABLE_FIELDS,
            Self::WandsWorkload | Self::EsciWorkload => &WORKLOAD_NULLABLE_FIELDS,
            Self::WandsAudit | Self::EsciAudit => &AUDIT_NULLABLE_FIELDS,
            Self::IndexArtifacts => &[],
        }
    }
}

pub fn parse_jsonl<T>(schema: JsonlSchema, bytes: &[u8]) -> Result<Vec<T>, PostRunError>
where
    T: DeserializeOwned,
{
    let file = schema.file();
    if bytes.contains(&b'\r') || !bytes.ends_with(b"\n") {
        return Err(PostRunError::JsonlLineEndings { file });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PostRunError::JsonlEncoding { file })?;
    text[..text.len() - 1]
        .split('\n')
        .enumerate()
        .map(|(index, line)| parse_line(schema, index + 1, line))
        .collect()
}

fn parse_line<T>(schema: JsonlSchema, line_number: usize, line: &str) -> Result<T, PostRunError>
where
    T: DeserializeOwned,
{
    let file = schema.file();
    if line.trim().is_empty() {
        return Err(PostRunError::JsonlBlankLine {
            file,
            line: line_number,
        });
    }
    let mut duplicate_check = serde_json::Deserializer::from_str(line);
    NoDuplicates
        .deserialize(&mut duplicate_check)
        .and_then(|()| duplicate_check.end())
        .map_err(|source| classify_json_error(file, line_number, &source))?;
    let object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(line)
        .map_err(|source| classify_json_error(file, line_number, &source))?;
    for required in schema.required_nullable_fields() {
        if !object.contains_key(*required) {
            return Err(PostRunError::JsonlMissingField {
                file,
                line: line_number,
                field: (*required).to_owned(),
            });
        }
    }
    serde_json::from_str(line).map_err(|source| classify_json_error(file, line_number, &source))
}

fn classify_json_error(
    file: &'static str,
    line: usize,
    source: &serde_json::Error,
) -> PostRunError {
    let message = source.to_string();
    if let Some(field) = quoted_field(&message, "unknown field `") {
        PostRunError::JsonlUnknownField { file, line, field }
    } else if let Some(field) = quoted_field(&message, "duplicate field `")
        .or_else(|| quoted_field(&message, "duplicate JSON field `"))
    {
        PostRunError::JsonlDuplicateField { file, line, field }
    } else if let Some(field) = quoted_field(&message, "missing field `") {
        PostRunError::JsonlMissingField { file, line, field }
    } else {
        PostRunError::JsonlInvalid { file, line }
    }
}

fn quoted_field(message: &str, prefix: &str) -> Option<String> {
    let remainder = message.split_once(prefix)?.1;
    Some(remainder.split_once('`')?.0.to_owned())
}

struct NoDuplicates;

impl<'de> DeserializeSeed<'de> for NoDuplicates {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(NoDuplicatesVisitor)
    }
}

struct NoDuplicatesVisitor;

impl<'de> Visitor<'de> for NoDuplicatesVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("valid JSON without duplicate object fields")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        NoDuplicates.deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element_seed(NoDuplicates)?.is_some() {}
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = BTreeSet::new();
        while let Some(field) = map.next_key::<String>()? {
            if !fields.insert(field.clone()) {
                return Err(A::Error::custom(format!("duplicate JSON field `{field}`")));
            }
            map.next_value_seed(NoDuplicates)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_revision_nine_nullable_key_is_required_by_its_boundary_schema() {
        // Given / When / Then
        for schema in [
            JsonlSchema::Raw,
            JsonlSchema::WandsWorkload,
            JsonlSchema::EsciWorkload,
            JsonlSchema::WandsAudit,
            JsonlSchema::EsciAudit,
        ] {
            let complete = schema
                .required_nullable_fields()
                .iter()
                .map(|field| ((*field).to_owned(), serde_json::Value::Null))
                .collect::<serde_json::Map<_, _>>();
            for missing in schema.required_nullable_fields() {
                let mut incomplete = complete.clone();
                incomplete.remove(*missing);
                let bytes = format!("{}\n", serde_json::Value::Object(incomplete));

                let error = parse_jsonl::<serde_json::Value>(schema, bytes.as_bytes())
                    .expect_err("omitted nullable key is rejected");

                assert!(matches!(
                    error,
                    PostRunError::JsonlMissingField { field, .. } if field == *missing
                ));
            }
        }
    }
}
