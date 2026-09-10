use crate::CampaignCycle;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SealEntry {
    pub(crate) path: String,
    pub(crate) hash_source: SealHashSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SealHashSource {
    Computed,
    Pinned(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SealStage {
    CreateNew,
    WriteAll,
    Flush,
    Sync,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SealManifest {
    pub(crate) entries: Vec<SealEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSealEntry {
    pub(crate) path: String,
    pub(crate) hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSealManifest {
    pub(crate) entries: Vec<ResolvedSealEntry>,
}

impl SealManifest {
    pub(crate) fn revision9(cycle: CampaignCycle) -> Self {
        let local = format!("artifacts/issue61/i61_e1_{}", cycle.as_str());
        let mut entries = [
            "candidate_audit_esci.jsonl",
            "candidate_audit_wands.jsonl",
            "commands.log",
            "events.jsonl",
            "index_artifacts.jsonl",
            "raw.jsonl",
        ]
        .map(|file| SealEntry {
            path: format!("{local}/{file}"),
            hash_source: SealHashSource::Computed,
        })
        .to_vec();
        entries.extend([
            static_entry(
                "benchmarks/configs/issue61/solr_esci_electronics_config.json",
                "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
            ),
            static_entry(
                "benchmarks/configs/issue61/solr_esci_electronics_schema.json",
                "62266803df8715b3b09485fdc168b580c1ae337d1dd7b5b43ae1ce09e74eca2e",
            ),
            static_entry(
                "benchmarks/configs/issue61/solr_wands_config.json",
                "ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b",
            ),
            static_entry(
                "benchmarks/configs/issue61/solr_wands_schema.json",
                "997e321ed081133b9a83fce5f35e42a75cfd3333bf91505f876501343600463a",
            ),
            static_entry(
                "benchmarks/workloads/i61_esci_electronics.jsonl",
                "531e39d0feda45591c0f3f17adfa25b1b52d73ff70994a3e31cad739364f050e",
            ),
            static_entry(
                "benchmarks/workloads/i61_wands_480.jsonl",
                "462b5bf8cae6e12fdcfa2cb5177a648d4de43aec0936c35e61aaffaacb0cad08",
            ),
        ]);
        Self { entries }
    }
}

fn static_entry(path: &'static str, hash: &'static str) -> SealEntry {
    SealEntry {
        path: path.to_owned(),
        hash_source: SealHashSource::Pinned(hash),
    }
}
