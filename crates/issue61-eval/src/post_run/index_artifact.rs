use super::error::{IndexReason, PostRunError};
use super::jsonl::{parse_jsonl, JsonlSchema};
use crate::{CampaignCycle, Dataset, Engine, ExactIndexObservation};
use serde::{Deserialize, Deserializer};
use std::str::FromStr;

const MAX_EXACT_INTEGER: u64 = 1 << 53;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexRecord {
    schema_version: u32,
    experiment_id: String,
    cycle: String,
    engine: String,
    dataset: String,
    document_count: u64,
    index_serialized_bytes: u64,
    #[serde(default, deserialize_with = "present_snapshot")]
    schema_snapshot: Option<Snapshot>,
    #[serde(default, deserialize_with = "present_snapshot")]
    config_snapshot: Option<Snapshot>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    path: String,
    sha256: String,
}

fn present_snapshot<'de, D>(deserializer: D) -> Result<Option<Snapshot>, D::Error>
where
    D: Deserializer<'de>,
{
    Snapshot::deserialize(deserializer).map(Some)
}

pub fn parse_index_artifacts(
    bytes: &[u8],
    cycle: CampaignCycle,
) -> Result<Vec<ExactIndexObservation>, PostRunError> {
    let records: Vec<IndexRecord> = parse_jsonl(JsonlSchema::IndexArtifacts, bytes)?;
    if records.len() != 4 {
        return Err(PostRunError::WrongRecordCount {
            file: "index_artifacts.jsonl",
            expected: 4,
            found: records.len(),
        });
    }
    let mut observations = [None; 4];
    for (index, record) in records.into_iter().enumerate() {
        let line = index + 1;
        let observation = validate_record(record, cycle, line)?;
        let position = cell_position(observation.engine, observation.dataset);
        if observations[position].replace(observation).is_some() {
            return Err(PostRunError::InvalidIndexRecord {
                line,
                reason: IndexReason::DuplicateCell,
            });
        }
    }
    observations
        .into_iter()
        .map(|observation| {
            observation.ok_or(PostRunError::InvalidIndexRecord {
                line: 0,
                reason: IndexReason::MissingCell,
            })
        })
        .collect()
}

fn validate_record(
    record: IndexRecord,
    cycle: CampaignCycle,
    line: usize,
) -> Result<ExactIndexObservation, PostRunError> {
    if record.schema_version != 1 {
        return Err(invalid(line, IndexReason::SchemaVersion));
    }
    if record.experiment_id != "I61-E1" {
        return Err(invalid(line, IndexReason::ExperimentId));
    }
    if record.cycle != cycle.as_str() {
        return Err(invalid(line, IndexReason::Cycle));
    }
    let engine =
        Engine::from_str(&record.engine).map_err(|_| invalid(line, IndexReason::Engine))?;
    let dataset =
        Dataset::parse(&record.dataset).map_err(|_| invalid(line, IndexReason::Dataset))?;
    if record.document_count != expected_documents(dataset) {
        return Err(invalid(line, IndexReason::DocumentCount));
    }
    if !(1..=MAX_EXACT_INTEGER).contains(&record.index_serialized_bytes) {
        return Err(invalid(line, IndexReason::SerializedBytes));
    }
    validate_snapshots(
        engine,
        dataset,
        record.schema_snapshot.as_ref(),
        record.config_snapshot.as_ref(),
        line,
    )?;
    Ok(ExactIndexObservation {
        cycle,
        engine,
        dataset,
        bytes: record.index_serialized_bytes,
    })
}

fn validate_snapshots(
    engine: Engine,
    dataset: Dataset,
    schema: Option<&Snapshot>,
    config: Option<&Snapshot>,
    line: usize,
) -> Result<(), PostRunError> {
    match (engine, schema, config) {
        (Engine::Native, None, None) => Ok(()),
        (Engine::Native, Some(_), None | Some(_))
        | (Engine::Native, None, Some(_))
        | (Engine::Solr, None, None | Some(_))
        | (Engine::Solr, Some(_), None) => Err(invalid(line, IndexReason::SnapshotFields)),
        (Engine::Solr, Some(schema), Some(config)) => {
            let expected = expected_snapshots(dataset);
            if schema.path == expected.schema_path
                && schema.sha256 == expected.schema_hash
                && config.path == expected.config_path
                && config.sha256 == expected.config_hash
            {
                Ok(())
            } else {
                Err(invalid(line, IndexReason::SnapshotIdentity))
            }
        }
    }
}

struct ExpectedSnapshots {
    schema_path: &'static str,
    schema_hash: &'static str,
    config_path: &'static str,
    config_hash: &'static str,
}

const fn expected_snapshots(dataset: Dataset) -> ExpectedSnapshots {
    match dataset {
        Dataset::Wands => ExpectedSnapshots {
            schema_path: "benchmarks/configs/issue61/solr_wands_schema.json",
            schema_hash: "997e321ed081133b9a83fce5f35e42a75cfd3333bf91505f876501343600463a",
            config_path: "benchmarks/configs/issue61/solr_wands_config.json",
            config_hash: "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
        },
        Dataset::EsciElectronics => ExpectedSnapshots {
            schema_path: "benchmarks/configs/issue61/solr_esci_electronics_schema.json",
            schema_hash: "62266803df8715b3b09485fdc168b580c1ae337d1dd7b5b43ae1ce09e74eca2e",
            config_path: "benchmarks/configs/issue61/solr_esci_electronics_config.json",
            config_hash: "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
        },
    }
}

const fn expected_documents(dataset: Dataset) -> u64 {
    match dataset {
        Dataset::Wands => 42_994,
        Dataset::EsciElectronics => 2_075,
    }
}

const fn cell_position(engine: Engine, dataset: Dataset) -> usize {
    match (engine, dataset) {
        (Engine::Native, Dataset::Wands) => 0,
        (Engine::Native, Dataset::EsciElectronics) => 1,
        (Engine::Solr, Dataset::Wands) => 2,
        (Engine::Solr, Dataset::EsciElectronics) => 3,
    }
}

const fn invalid(line: usize, reason: IndexReason) -> PostRunError {
    PostRunError::InvalidIndexRecord { line, reason }
}
