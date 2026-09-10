use super::*;

const WANDS: &[u8] = include_bytes!("../../../../benchmarks/workloads/i61_wands_480.jsonl");
const ESCI: &[u8] = include_bytes!("../../../../benchmarks/workloads/i61_esci_electronics.jsonl");

#[test]
fn workload_parser_independently_rejects_omitted_engine_blocks() {
    for (schema, bytes, expected) in workload_cases() {
        for field in ["native", "solr"] {
            let bytes = mutate_first_record(bytes, field, None);

            assert!(matches!(
                parse_workload(schema, &bytes, expected),
                Err(PostRunError::JsonlMissingField { field: missing, .. }) if missing == field
            ));
        }
    }
}

#[test]
fn workload_parser_independently_rejects_null_engine_blocks() {
    for (schema, bytes, expected) in workload_cases() {
        for field in ["native", "solr"] {
            let bytes = mutate_first_record(bytes, field, Some(serde_json::Value::Null));

            assert!(matches!(
                parse_workload(schema, &bytes, expected),
                Err(PostRunError::MissingEngineRequest { line: 1, .. })
            ));
        }
    }
}

fn workload_cases() -> [(JsonlSchema, &'static [u8], usize); 2] {
    [
        (JsonlSchema::WandsWorkload, WANDS, WANDS_RECORDS),
        (JsonlSchema::EsciWorkload, ESCI, ESCI_RECORDS),
    ]
}

fn mutate_first_record(
    bytes: &[u8],
    field: &str,
    replacement: Option<serde_json::Value>,
) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).expect("frozen workload is UTF-8");
    let (first, remainder) = text.split_once('\n').expect("workload has records");
    let mut value: serde_json::Value = serde_json::from_str(first).expect("record is JSON");
    let object = value.as_object_mut().expect("record is an object");
    match replacement {
        Some(replacement) => {
            *object.get_mut(field).expect("engine block exists") = replacement;
        }
        None => {
            object.remove(field).expect("engine block exists");
        }
    }
    format!(
        "{}\n{remainder}",
        serde_json::to_string(&value).expect("record serializes")
    )
    .into_bytes()
}
