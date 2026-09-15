//! Issue #62 (Infra E2) physical footprint / SKU-density scaling benchmark.
//! Experiment code only; never a dependency of product crates.
//!
//! Reuses #61's generic measurement primitives (`issue61_eval::{CgroupReader,
//! MemorySampler, sha256_hex}`) without modification. Does not reuse #61's
//! `LifecyclePort`/campaign machinery, which is purpose-built for the WANDS/
//! ESCI 310-block equivalence+timing campaign — E2 measures a different
//! thing (index footprint/RSS at scale) with a much simpler per-(engine,
//! tier,run) lifecycle: provision, measure, tear down.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Native,
    Solr,
    Elasticsearch,
    Opensearch,
    Typesense,
    Meilisearch,
    Vespa,
    Havenask,
}

impl Engine {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Solr => "solr",
            Self::Elasticsearch => "elasticsearch",
            Self::Opensearch => "opensearch",
            Self::Typesense => "typesense",
            Self::Meilisearch => "meilisearch",
            Self::Vespa => "vespa",
            Self::Havenask => "havenask",
        }
    }

    #[must_use]
    pub const fn is_jvm(self) -> bool {
        matches!(self, Self::Solr | Self::Elasticsearch | Self::Opensearch)
    }

    /// Fixed Docker container name for this engine, matching
    /// `benchmarks/configs/issue62/container_limits.env`'s `I62_*_CONTAINER`
    /// values (frozen Rust constants here, same pattern #61's
    /// `NativeLaunchContract` already uses for its own frozen values).
    #[must_use]
    pub const fn container_name(self) -> &'static str {
        match self {
            Self::Native => "i62-native",
            Self::Solr => "i62-solr",
            Self::Elasticsearch => "i62-elasticsearch",
            Self::Opensearch => "i62-opensearch",
            Self::Typesense => "i62-typesense",
            Self::Meilisearch => "i62-meilisearch",
            Self::Vespa => "i62-vespa",
            Self::Havenask => "i62-havenask",
        }
    }

    pub const ALL: [Self; 8] = [
        Self::Native,
        Self::Solr,
        Self::Elasticsearch,
        Self::Opensearch,
        Self::Typesense,
        Self::Meilisearch,
        Self::Vespa,
        Self::Havenask,
    ];
}

impl FromStr for Engine {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "native" => Ok(Self::Native),
            "solr" => Ok(Self::Solr),
            "elasticsearch" => Ok(Self::Elasticsearch),
            "opensearch" => Ok(Self::Opensearch),
            "typesense" => Ok(Self::Typesense),
            "meilisearch" => Ok(Self::Meilisearch),
            "vespa" => Ok(Self::Vespa),
            "havenask" => Ok(Self::Havenask),
            other => Err(format!("unknown engine {other:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    T100k,
    T500k,
    T1m,
    T3m,
    T5m,
}

impl Tier {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::T100k => "100k",
            Self::T500k => "500k",
            Self::T1m => "1m",
            Self::T3m => "3m",
            Self::T5m => "5m",
        }
    }

    /// `scripts/datasets/replicate_wands_scale.py`'s multiplier for this
    /// tier, matching `benchmarks/configs/issue62/container_limits.env`'s
    /// `I62_TIER_*_MULTIPLIER` values.
    #[must_use]
    pub const fn multiplier(self) -> u32 {
        match self {
            Self::T100k => 3,
            Self::T500k => 12,
            Self::T1m => 24,
            Self::T3m => 70,
            Self::T5m => 117,
        }
    }

    /// Expected exact row count, matching
    /// `benchmarks/configs/issue62/container_limits.env`'s
    /// `I62_TIER_*_DOCS` values (`multiplier() * 42_994`).
    #[must_use]
    pub const fn expected_docs(self) -> u64 {
        match self {
            Self::T100k => 128_982,
            Self::T500k => 515_928,
            Self::T1m => 1_031_856,
            Self::T3m => 3_009_580,
            Self::T5m => 5_030_298,
        }
    }

    #[must_use]
    pub fn catalog_relative_path(self) -> String {
        format!(
            "dataset_cache/wands_scale/catalog_{}x.jsonl",
            self.multiplier()
        )
    }

    pub const ALL: [Self; 5] = [Self::T100k, Self::T500k, Self::T1m, Self::T3m, Self::T5m];
}

impl FromStr for Tier {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "100k" => Ok(Self::T100k),
            "500k" => Ok(Self::T500k),
            "1m" => Ok(Self::T1m),
            "3m" => Ok(Self::T3m),
            "5m" => Ok(Self::T5m),
            other => Err(format!("unknown tier {other:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Ok,
    DocCountMismatch,
    HarnessFailure,
    InvalidEnvironmental,
    EngineExcluded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementResult {
    pub schema_version: u32,
    pub experiment_id: String,
    pub engine: String,
    pub tier: String,
    pub run: u32,
    pub expected_docs: u64,
    pub docs: Option<u64>,
    pub index_bytes: Option<u64>,
    pub bytes_per_product: Option<f64>,
    pub build_wall_ms: Option<u64>,
    pub build_cpu_usec: Option<u64>,
    pub peak_build_memory_bytes: Option<u64>,
    pub peak_build_disk_bytes: Option<u64>,
    pub cold_rss_bytes: Option<u64>,
    pub warm_rss_bytes: Option<u64>,
    pub probe_queries_ok: u32,
    pub probe_queries_failed: u32,
    pub jvm_configured_heap_bytes: Option<u64>,
    pub container_name: String,
    pub cpus: String,
    pub cpuset: String,
    pub memory_limit: String,
    pub git_sha: String,
    pub hostname: String,
    pub timestamp_utc: String,
    pub status: RunStatus,
    pub failure_reason: Option<String>,
    pub provision_stdout_tail: Option<String>,
    pub provision_stderr_tail: Option<String>,
}

pub const EXPERIMENT_ID: &str = "I62-E2";
pub const RAW_SCHEMA_VERSION: u32 = 1;

/// Result of a bounded subprocess invocation, mirroring
/// `issue61_eval::lifecycle::live_port`'s private `run_bounded` (not
/// exported across the crate boundary, so this is a small, independent
/// reimplementation of the same pattern rather than a cross-crate reuse).
pub struct BoundedOutput {
    pub status: Option<std::process::ExitStatus>,
    pub stdout: String,
    pub stderr: String,
}

impl BoundedOutput {
    #[must_use]
    pub fn timed_out(&self) -> bool {
        self.status.is_none()
    }

    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.status.is_some_and(|status| status.success())
    }

    #[must_use]
    pub fn tail(text: &str, max_chars: usize) -> String {
        if text.chars().count() <= max_chars {
            text.to_owned()
        } else {
            let skip = text.chars().count() - max_chars;
            text.chars().skip(skip).collect()
        }
    }
}

pub fn run_bounded(command: &mut Command, timeout: Duration) -> io::Result<BoundedOutput> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child: Child = command.spawn()?;
    let mut stdout_pipe = child.stdout.take().expect("stdout is piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr is piped");
    let stdout_thread = thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stdout_pipe.read_to_string(&mut buffer);
        buffer
    });
    let stderr_thread = thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stderr_pipe.read_to_string(&mut buffer);
        buffer
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(200));
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
    })
}

/// Parses `PROVISION_OK container=<name> docs=<n> index_bytes=<n>` from a
/// provisioning script's stdout, matching every `scripts/issue62/provision_*.sh`
/// script's contract.
#[must_use]
pub fn parse_provision_ok(text: &str) -> Option<(String, u64, u64)> {
    let line = text.lines().find(|line| line.contains("PROVISION_OK"))?;
    let container = line
        .split_whitespace()
        .find_map(|token| token.strip_prefix("container="))?
        .to_owned();
    let docs = extract_kv(line, "docs=")?;
    let index_bytes = extract_kv(line, "index_bytes=")?;
    Some((container, docs, index_bytes))
}

fn extract_kv(line: &str, key: &str) -> Option<u64> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(key))
        .and_then(|value| value.parse().ok())
}

impl fmt::Display for Engine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.as_str())
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_expected_docs_matches_multiplier_times_real_catalog_size() {
        for tier in Tier::ALL {
            assert_eq!(tier.expected_docs(), u64::from(tier.multiplier()) * 42_994);
        }
    }

    #[test]
    fn tier_round_trips_through_str() {
        for tier in Tier::ALL {
            assert_eq!(tier.as_str().parse::<Tier>().unwrap(), tier);
        }
    }

    #[test]
    fn engine_round_trips_through_str() {
        for engine in Engine::ALL {
            assert_eq!(engine.as_str().parse::<Engine>().unwrap(), engine);
        }
    }

    #[test]
    fn container_names_are_pairwise_distinct() {
        let names: Vec<&str> = Engine::ALL
            .iter()
            .map(|engine| engine.container_name())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn parse_provision_ok_extracts_all_three_fields() {
        let stdout =
            "==> progress\nPROVISION_OK container=i62-solr docs=128982 index_bytes=12345678\n";
        assert_eq!(
            parse_provision_ok(stdout),
            Some(("i62-solr".to_owned(), 128_982, 12_345_678))
        );
    }

    #[test]
    fn parse_provision_ok_is_none_without_marker() {
        assert_eq!(parse_provision_ok("PROVISION_FAILED reason=x"), None);
    }
}
