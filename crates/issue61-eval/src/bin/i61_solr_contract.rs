use issue61_eval::solr_contract::{
    validate_contract, ContractDocuments, ContractError, SolrDataset,
};
use serde_json::Value;
use std::env;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

#[derive(Debug)]
enum CliError {
    Usage,
    Contract(ContractError),
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        source_name: String,
        source: serde_json::Error,
    },
    Http {
        endpoint: &'static str,
        source: Box<ureq::Error>,
    },
    HttpBody {
        endpoint: &'static str,
        source: std::io::Error,
    },
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => write!(
                formatter,
                "usage: i61_solr_contract <wands|esci_electronics> <core-url> <config-dir>"
            ),
            Self::Contract(source) => source.fmt(formatter),
            Self::Read { path, source } => write!(formatter, "read {}: {source}", path.display()),
            Self::Json {
                source_name,
                source,
            } => write!(formatter, "parse JSON from {source_name}: {source}"),
            Self::Http { endpoint, source } => write!(formatter, "GET {endpoint} failed: {source}"),
            Self::HttpBody { endpoint, source } => {
                write!(formatter, "read GET {endpoint} body: {source}")
            }
        }
    }
}

impl Error for CliError {}

fn read_json(path: &Path) -> Result<Value, CliError> {
    let body = std::fs::read_to_string(path).map_err(|source| CliError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&body).map_err(|source| CliError::Json {
        source_name: path.display().to_string(),
        source,
    })
}

fn fetch_json(
    agent: &ureq::Agent,
    core_url: &str,
    endpoint: &'static str,
) -> Result<Value, CliError> {
    let url = format!("{}{endpoint}", core_url.trim_end_matches('/'));
    let body = agent
        .get(&url)
        .call()
        .map_err(|source| CliError::Http {
            endpoint,
            source: Box::new(source),
        })?
        .into_string()
        .map_err(|source| CliError::HttpBody { endpoint, source })?;
    serde_json::from_str(&body).map_err(|source| CliError::Json {
        source_name: url,
        source,
    })
}

fn run() -> Result<(), CliError> {
    let mut args = env::args().skip(1);
    let dataset =
        SolrDataset::parse(&args.next().ok_or(CliError::Usage)?).map_err(CliError::Contract)?;
    let core_url = args.next().ok_or(CliError::Usage)?;
    let config_dir = PathBuf::from(args.next().ok_or(CliError::Usage)?);
    if args.next().is_some() {
        return Err(CliError::Usage);
    }

    let expected_schema = read_json(&config_dir.join(dataset.schema_snapshot()))?;
    let expected_config = read_json(&config_dir.join(dataset.config_snapshot()))?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(30))
        .build();
    let live_schema = fetch_json(&agent, &core_url, "/schema")?;
    let live_config = fetch_json(&agent, &core_url, "/config")?;
    validate_contract(
        dataset,
        ContractDocuments {
            live_schema_envelope: &live_schema,
            live_config_envelope: &live_config,
            expected_schema: &expected_schema,
            expected_config: &expected_config,
        },
    )
    .map_err(CliError::Contract)?;
    println!(
        "SOLR_CONTRACT_OK dataset={} core={}",
        dataset.as_str(),
        dataset.core_name()
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FATAL: {error}");
            ExitCode::FAILURE
        }
    }
}
