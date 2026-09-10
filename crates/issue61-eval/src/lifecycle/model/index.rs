use super::{DatasetIdentity, EngineIdentity};
use crate::CampaignCycle;
use serde::Serialize;

const MAX_EXACT_INTEGER: u64 = 1 << 53;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexError {
    WrongSchema,
    WrongExperiment,
    WrongCycle,
    WrongOrder,
    WrongDocumentCount,
    WrongSerializedBytes,
    WrongSnapshot,
    DuplicateCell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Snapshot {
    pub(crate) path: &'static str,
    pub(crate) sha256: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct IndexCell {
    pub(crate) schema_version: u8,
    pub(crate) experiment_id: &'static str,
    pub(crate) cycle: &'static str,
    pub(crate) engine: EngineIdentity,
    pub(crate) dataset: DatasetIdentity,
    pub(crate) document_count: u64,
    pub(crate) index_serialized_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) schema_snapshot: Option<Snapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) config_snapshot: Option<Snapshot>,
}

impl IndexCell {
    pub(crate) const fn native(cycle: CampaignCycle, dataset: DatasetIdentity, bytes: u64) -> Self {
        Self::new(cycle, EngineIdentity::Native, dataset, bytes, None)
    }

    pub(crate) const fn solr(cycle: CampaignCycle, dataset: DatasetIdentity, bytes: u64) -> Self {
        Self::new(
            cycle,
            EngineIdentity::Solr,
            dataset,
            bytes,
            Some(expected_snapshots(dataset)),
        )
    }

    const fn new(
        cycle: CampaignCycle,
        engine: EngineIdentity,
        dataset: DatasetIdentity,
        bytes: u64,
        snapshots: Option<(Snapshot, Snapshot)>,
    ) -> Self {
        let (schema_snapshot, config_snapshot) = match snapshots {
            Some((schema, config)) => (Some(schema), Some(config)),
            None => (None, None),
        };
        Self {
            schema_version: 1,
            experiment_id: "I61-E1",
            cycle: cycle.as_str(),
            engine,
            dataset,
            document_count: expected_documents(dataset),
            index_serialized_bytes: bytes,
            schema_snapshot,
            config_snapshot,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexArtifact(pub(crate) IndexCell);

pub(crate) fn validate_index(
    cycle: CampaignCycle,
    artifacts: &[IndexArtifact],
) -> Result<(), IndexError> {
    let expected = [
        (EngineIdentity::Native, DatasetIdentity::Wands),
        (EngineIdentity::Native, DatasetIdentity::EsciElectronics),
        (EngineIdentity::Solr, DatasetIdentity::Wands),
        (EngineIdentity::Solr, DatasetIdentity::EsciElectronics),
    ];
    if artifacts.len() != expected.len() {
        return Err(IndexError::WrongOrder);
    }
    for (index, artifact) in artifacts.iter().enumerate() {
        let cell = &artifact.0;
        if artifacts[..index]
            .iter()
            .any(|prior| prior.0.engine == cell.engine && prior.0.dataset == cell.dataset)
        {
            return Err(IndexError::DuplicateCell);
        }
        if (cell.engine, cell.dataset) != expected[index] {
            return Err(IndexError::WrongOrder);
        }
        validate_cell(cycle, cell)?;
    }
    Ok(())
}

fn validate_cell(cycle: CampaignCycle, cell: &IndexCell) -> Result<(), IndexError> {
    if cell.schema_version != 1 {
        return Err(IndexError::WrongSchema);
    }
    if cell.experiment_id != "I61-E1" {
        return Err(IndexError::WrongExperiment);
    }
    if cell.cycle != cycle.as_str() {
        return Err(IndexError::WrongCycle);
    }
    if cell.document_count != expected_documents(cell.dataset) {
        return Err(IndexError::WrongDocumentCount);
    }
    if !(1..=MAX_EXACT_INTEGER).contains(&cell.index_serialized_bytes) {
        return Err(IndexError::WrongSerializedBytes);
    }
    let expected = match cell.engine {
        EngineIdentity::Native => (None, None),
        EngineIdentity::Solr => {
            let (schema, config) = expected_snapshots(cell.dataset);
            (Some(schema), Some(config))
        }
    };
    if cell.schema_snapshot != expected.0 || cell.config_snapshot != expected.1 {
        return Err(IndexError::WrongSnapshot);
    }
    Ok(())
}

const fn expected_documents(dataset: DatasetIdentity) -> u64 {
    match dataset {
        DatasetIdentity::Wands => 42_994,
        DatasetIdentity::EsciElectronics => 2_075,
    }
}

const fn expected_snapshots(dataset: DatasetIdentity) -> (Snapshot, Snapshot) {
    match dataset {
        DatasetIdentity::Wands => snapshots(
            "benchmarks/configs/issue61/solr_wands_schema.json",
            "997e321ed081133b9a83fce5f35e42a75cfd3333bf91505f876501343600463a",
            "benchmarks/configs/issue61/solr_wands_config.json",
        ),
        DatasetIdentity::EsciElectronics => snapshots(
            "benchmarks/configs/issue61/solr_esci_electronics_schema.json",
            "62266803df8715b3b09485fdc168b580c1ae337d1dd7b5b43ae1ce09e74eca2e",
            "benchmarks/configs/issue61/solr_esci_electronics_config.json",
        ),
    }
}

const fn snapshots(
    schema_path: &'static str,
    schema_hash: &'static str,
    config_path: &'static str,
) -> (Snapshot, Snapshot) {
    (
        Snapshot {
            path: schema_path,
            sha256: schema_hash,
        },
        Snapshot {
            path: config_path,
            sha256: "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
        },
    )
}
