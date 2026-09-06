mod checksum;
mod cycle_directory;
mod error;
mod index_artifact;
mod jsonl;
mod report;
#[cfg(test)]
mod tests;

use checksum::{verify_seal, VerifiedSeal};
use cycle_directory::AnalysisPaths;
pub use error::PostRunError;
use index_artifact::parse_index_artifacts;
use jsonl::{parse_jsonl, JsonlSchema};
use report::{publish_report, serialize_report};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::{
    analyze_campaign, CampaignCycle, CampaignEvidence, CandidateAuditEvidence,
    CandidateAuditRecord, Dataset, FrozenQuery, GateVerdict, RawRecord,
};

const RAW_RECORDS: usize = 620;
const WANDS_RECORDS: usize = 480;
const ESCI_RECORDS: usize = 600;

pub struct CompletedAnalysis {
    summary: String,
    exit_code: u8,
}

impl CompletedAnalysis {
    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub const fn exit_code(&self) -> u8 {
        self.exit_code
    }
}

pub fn run_completed_analysis_cli(args: &[OsString]) -> Result<CompletedAnalysis, PostRunError> {
    let [_, repository_flag, repository, cycle_flag, cycle] = args else {
        return Err(PostRunError::CliInvocation);
    };
    if repository_flag != OsStr::new("--repository-root") || cycle_flag != OsStr::new("--cycle") {
        return Err(PostRunError::CliInvocation);
    }
    let cycle = cycle
        .to_str()
        .ok_or(PostRunError::CliInvocation)
        .and_then(|value| {
            CampaignCycle::from_str(value).map_err(|_| PostRunError::CliInvocation)
        })?;
    run_completed_analysis(&PathBuf::from(repository), cycle)
}

pub fn run_completed_analysis(
    repository_root: &Path,
    cycle: CampaignCycle,
) -> Result<CompletedAnalysis, PostRunError> {
    let paths = AnalysisPaths::validate(repository_root, cycle)?;
    let seal = verify_seal(&paths.repository_root, &paths.cycle_directory, cycle)?;
    let parsed = ParsedEvidence::parse(&seal, cycle)?;
    let candidate_audits = [
        CandidateAuditEvidence {
            dataset: Dataset::Wands,
            workload: &parsed.wands_workload,
            records: &parsed.wands_audits,
        },
        CandidateAuditEvidence {
            dataset: Dataset::EsciElectronics,
            workload: &parsed.esci_workload,
            records: &parsed.esci_audits,
        },
    ];
    let analysis = analyze_campaign(CampaignEvidence {
        cycle,
        records: &parsed.raw,
        exact_index: &parsed.index,
        candidate_audits: &candidate_audits,
    })?;
    let report = serialize_report(&analysis, &seal.inputs)?;
    publish_report(&paths.analysis, &report)?;
    let (verdict, passing, blocked) = verdict_fields(analysis.gate_report.verdict());
    let exit_code = analysis.gate_report.exit_code();
    Ok(CompletedAnalysis {
        summary: format!(
            "i61_analyze: cycle={} verdict={verdict} passing_dataset={passing} blocked_dataset={blocked} exit_code={exit_code}\n",
            cycle.as_str()
        ),
        exit_code: if exit_code == 0 { 0 } else { 1 },
    })
}

struct ParsedEvidence {
    raw: Vec<RawRecord>,
    wands_workload: Vec<FrozenQuery>,
    esci_workload: Vec<FrozenQuery>,
    wands_audits: Vec<CandidateAuditRecord>,
    esci_audits: Vec<CandidateAuditRecord>,
    index: Vec<crate::ExactIndexObservation>,
}

impl ParsedEvidence {
    fn parse(seal: &VerifiedSeal, cycle: CampaignCycle) -> Result<Self, PostRunError> {
        let prefix = format!("artifacts/issue61/i61_e1_{}", cycle.as_str());
        let raw = parse_counted(
            JsonlSchema::Raw,
            seal.bytes(&format!("{prefix}/raw.jsonl"))?,
            RAW_RECORDS,
        )?;
        let wands_workload = parse_workload(
            JsonlSchema::WandsWorkload,
            seal.bytes("benchmarks/workloads/i61_wands_480.jsonl")?,
            WANDS_RECORDS,
        )?;
        let esci_workload = parse_workload(
            JsonlSchema::EsciWorkload,
            seal.bytes("benchmarks/workloads/i61_esci_electronics.jsonl")?,
            ESCI_RECORDS,
        )?;
        let wands_audits = parse_counted(
            JsonlSchema::WandsAudit,
            seal.bytes(&format!("{prefix}/candidate_audit_wands.jsonl"))?,
            WANDS_RECORDS,
        )?;
        let esci_audits = parse_counted(
            JsonlSchema::EsciAudit,
            seal.bytes(&format!("{prefix}/candidate_audit_esci.jsonl"))?,
            ESCI_RECORDS,
        )?;
        let index = parse_index_artifacts(
            seal.bytes(&format!("{prefix}/index_artifacts.jsonl"))?,
            cycle,
        )?;
        Ok(Self {
            raw,
            wands_workload,
            esci_workload,
            wands_audits,
            esci_audits,
            index,
        })
    }
}

fn parse_counted<T>(
    schema: JsonlSchema,
    bytes: &[u8],
    expected: usize,
) -> Result<Vec<T>, PostRunError>
where
    T: serde::de::DeserializeOwned,
{
    let records = parse_jsonl(schema, bytes)?;
    if records.len() != expected {
        return Err(PostRunError::WrongRecordCount {
            file: schema.file(),
            expected,
            found: records.len(),
        });
    }
    Ok(records)
}

fn parse_workload(
    schema: JsonlSchema,
    bytes: &[u8],
    expected: usize,
) -> Result<Vec<FrozenQuery>, PostRunError> {
    let records: Vec<FrozenQuery> = parse_counted(schema, bytes, expected)?;
    if let Some(index) = records
        .iter()
        .position(|record| record.native.is_none() || record.solr.is_none())
    {
        return Err(PostRunError::MissingEngineRequest {
            file: schema.file(),
            line: index + 1,
        });
    }
    Ok(records)
}

const fn verdict_fields(verdict: GateVerdict) -> (&'static str, &'static str, &'static str) {
    match verdict {
        GateVerdict::Keep => ("KEEP", "null", "null"),
        GateVerdict::Refine {
            passing_dataset,
            blocked_dataset,
        } => ("REFINE", passing_dataset.as_str(), blocked_dataset.as_str()),
        GateVerdict::FixMeasurement => ("FIX MEASUREMENT", "null", "null"),
    }
}
