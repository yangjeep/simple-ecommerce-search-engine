use commerce_core::index::CatalogIndex;
use issue61_eval::{
    audit_candidate_sets, fetch_complete_solr, frozen_native_query, load_dataset, load_workload,
    native_candidate_ids, AuditVerdict, Dataset,
};
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

struct Config {
    dataset: Dataset,
    catalog_path: PathBuf,
    workload_path: PathBuf,
    solr_core_url: String,
    output_path: PathBuf,
}

#[derive(Default)]
struct Summary {
    total: usize,
    matched: usize,
    mismatched: usize,
    native_failures: usize,
    engine_failures: usize,
}

impl Summary {
    const fn passes(&self) -> bool {
        self.total == self.matched
    }

    fn observe(&mut self, verdict: AuditVerdict) {
        self.total += 1;
        match verdict {
            AuditVerdict::Match => self.matched += 1,
            AuditVerdict::Mismatch => self.mismatched += 1,
            AuditVerdict::NativeFailure => self.native_failures += 1,
            AuditVerdict::EngineFailure => self.engine_failures += 1,
        }
    }
}

fn required_arg(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("missing {name}"))
}

fn parse_config(args: &[String]) -> Result<Config, String> {
    Ok(Config {
        dataset: Dataset::parse(&required_arg(args, "--dataset")?)?,
        catalog_path: required_arg(args, "--catalog")?.into(),
        workload_path: required_arg(args, "--workload")?.into(),
        solr_core_url: required_arg(args, "--solr-core-url")?,
        output_path: required_arg(args, "--out")?.into(),
    })
}

fn run(config: &Config) -> Result<Summary, Box<dyn Error>> {
    let data = load_dataset(&config.catalog_path, config.dataset)?;
    let index = CatalogIndex::build(&data.catalog);
    let workload = load_workload(&config.workload_path)?;
    if workload.is_empty() {
        return Err("workload is empty".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(30))
        .build();
    let mut writer = BufWriter::new(File::create(&config.output_path)?);
    let mut summary = Summary::default();
    for query in &workload {
        let native = frozen_native_query(query)
            .and_then(|query_text| native_candidate_ids(&data, &index, query_text));
        let engine = fetch_complete_solr(&agent, &config.solr_core_url, query);
        let record = audit_candidate_sets(config.dataset.as_str(), query, native, engine);
        summary.observe(record.verdict);
        serde_json::to_writer(&mut writer, &record)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(summary)
}

fn main() -> ExitCode {
    let config = match parse_config(&std::env::args().collect::<Vec<_>>()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("i61_equivalence_audit: {error}");
            return ExitCode::FAILURE;
        }
    };
    match run(&config) {
        Ok(summary) if summary.passes() => {
            println!(
                "EQUIVALENCE_OK dataset={} total={} matched={}",
                config.dataset.as_str(),
                summary.total,
                summary.matched
            );
            ExitCode::SUCCESS
        }
        Ok(summary) => {
            eprintln!(
                "EQUIVALENCE_FAILED dataset={} total={} matched={} mismatched={} native_failures={} engine_failures={}",
                config.dataset.as_str(),
                summary.total,
                summary.matched,
                summary.mismatched,
                summary.native_failures,
                summary.engine_failures
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("i61_equivalence_audit: {error}");
            ExitCode::FAILURE
        }
    }
}
