#![allow(dead_code)]

mod raw;

use issue61_eval::{sha256_hex, CampaignCycle, RawRecord};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use raw::campaign_records;

const STATIC_INPUTS: [&str; 6] = [
    "benchmarks/configs/issue61/solr_esci_electronics_config.json",
    "benchmarks/configs/issue61/solr_esci_electronics_schema.json",
    "benchmarks/configs/issue61/solr_wands_config.json",
    "benchmarks/configs/issue61/solr_wands_schema.json",
    "benchmarks/workloads/i61_esci_electronics.jsonl",
    "benchmarks/workloads/i61_wands_480.jsonl",
];
const CYCLE_INPUTS: [&str; 6] = [
    "candidate_audit_esci.jsonl",
    "candidate_audit_wands.jsonl",
    "commands.log",
    "events.jsonl",
    "index_artifacts.jsonl",
    "raw.jsonl",
];
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

pub struct CompletedCycle {
    root: PathBuf,
    cycle: CampaignCycle,
    records: Vec<RawRecord>,
}

impl CompletedCycle {
    pub fn run1() -> Self {
        let root = create_temporary_root();
        let cycle = CampaignCycle::Run1;
        let cycle_dir = cycle_dir(&root, cycle);
        std::fs::create_dir_all(&cycle_dir).expect("cycle directory is created");
        copy_static_inputs(&root);
        copy_audits(&cycle_dir);
        std::fs::write(cycle_dir.join("events.jsonl"), b"completed\n")
            .expect("opaque events evidence is written");
        std::fs::write(cycle_dir.join("commands.log"), b"campaign complete\n")
            .expect("opaque command evidence is written");
        std::fs::write(cycle_dir.join("index_artifacts.jsonl"), index_bytes(cycle))
            .expect("index evidence is written");
        let records = campaign_records(cycle);
        issue61_eval::write_jsonl(&cycle_dir.join("raw.jsonl"), &records)
            .expect("raw evidence is written");
        let fixture = Self {
            root,
            cycle,
            records,
        };
        fixture.reseal();
        fixture
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn cycle_dir(&self) -> PathBuf {
        cycle_dir(&self.root, self.cycle)
    }

    pub fn analysis_path(&self) -> PathBuf {
        self.cycle_dir().join("analysis.json")
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.cycle_dir().join("checksums.sha256")
    }

    pub fn input_path(&self, name: &str) -> PathBuf {
        self.cycle_dir().join(name)
    }

    pub fn rewrite_records(&mut self, change: impl FnOnce(&mut [RawRecord])) {
        change(&mut self.records);
        let path = self.input_path("raw.jsonl");
        std::fs::remove_file(&path).expect("old raw evidence is removed");
        issue61_eval::write_jsonl(&path, &self.records).expect("changed raw evidence is written");
        self.reseal();
    }

    pub fn reseal(&self) {
        let prefix = format!("artifacts/issue61/i61_e1_{}", self.cycle.as_str());
        let mut paths = CYCLE_INPUTS
            .iter()
            .map(|name| format!("{prefix}/{name}"))
            .chain(STATIC_INPUTS.iter().map(|path| (*path).to_owned()))
            .collect::<Vec<_>>();
        paths.sort_unstable();
        let seal = paths
            .iter()
            .map(|relative| {
                let bytes =
                    std::fs::read(self.root.join(relative)).expect("sealed input is readable");
                format!("{}  {relative}\n", sha256_hex(&bytes))
            })
            .collect::<String>();
        std::fs::write(self.manifest_path(), seal).expect("checksum seal is written");
    }
}

impl Drop for CompletedCycle {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("isolated fixture cleanup succeeds");
    }
}

fn repository_source() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate belongs to repository workspace")
        .to_owned()
}

fn create_temporary_root() -> PathBuf {
    loop {
        let nonce = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "issue61-analysis-test-{}-{nonce}",
            std::process::id()
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("failed to create isolated root {path:?}: {error}"),
        }
    }
}

fn cycle_dir(root: &Path, cycle: CampaignCycle) -> PathBuf {
    root.join(format!("artifacts/issue61/i61_e1_{}", cycle.as_str()))
}

fn copy_static_inputs(root: &Path) {
    let source = repository_source();
    for relative in STATIC_INPUTS {
        let destination = root.join(relative);
        std::fs::create_dir_all(destination.parent().expect("static input has a parent"))
            .expect("static input parent is created");
        std::fs::copy(source.join(relative), destination).expect("real static bytes are copied");
    }
}

fn copy_audits(cycle_dir: &Path) {
    let source = repository_source().join("artifacts/issue61");
    for (from, to) in [
        (
            "i61_esci_electronics_equivalence_rev5.jsonl",
            "candidate_audit_esci.jsonl",
        ),
        (
            "i61_wands_equivalence_rev5.jsonl",
            "candidate_audit_wands.jsonl",
        ),
    ] {
        std::fs::copy(source.join(from), cycle_dir.join(to))
            .expect("Revision 5 audit bytes are copied under cycle-local names");
    }
}

fn index_bytes(cycle: CampaignCycle) -> String {
    let cycle = cycle.as_str();
    [
        format!(r#"{{"schema_version":1,"experiment_id":"I61-E1","cycle":"{cycle}","engine":"native","dataset":"wands","document_count":42994,"index_serialized_bytes":7777}}"#),
        format!(r#"{{"schema_version":1,"experiment_id":"I61-E1","cycle":"{cycle}","engine":"native","dataset":"esci_electronics","document_count":2075,"index_serialized_bytes":7778}}"#),
        solr_index(cycle, "wands", 42_994, "997e321ed081133b9a83fce5f35e42a75cfd3333bf91505f876501343600463a"),
        solr_index(cycle, "esci_electronics", 2_075, "62266803df8715b3b09485fdc168b580c1ae337d1dd7b5b43ae1ce09e74eca2e"),
    ]
    .join("\n")
        + "\n"
}

fn solr_index(cycle: &str, dataset: &str, documents: u64, schema_hash: &str) -> String {
    let stem = if dataset == "wands" {
        "wands"
    } else {
        "esci_electronics"
    };
    let index_serialized_bytes = if dataset == "wands" { 8_888 } else { 8_889 };
    format!(
        r#"{{"schema_version":1,"experiment_id":"I61-E1","cycle":"{cycle}","engine":"solr","dataset":"{dataset}","document_count":{documents},"index_serialized_bytes":{index_serialized_bytes},"schema_snapshot":{{"path":"benchmarks/configs/issue61/solr_{stem}_schema.json","sha256":"{schema_hash}"}},"config_snapshot":{{"path":"benchmarks/configs/issue61/solr_{stem}_config.json","sha256":"ae5c0e1c8de23a798a550b6042617f0bedd4b8e04a1ecbe5d8d9633df5bfff5b"}}}}"#
    )
}
