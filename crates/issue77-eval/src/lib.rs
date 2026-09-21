//! Issue #77 (Infra E3) category PLP / disjunctive-faceting H2H benchmark.
//! Experiment code only; never a dependency of product crates.
//!
//! Two dataset roles, per the #77 preregistration -- do not conflate:
//!
//! - Dataset A: the WANDS-scale catalogs already generated for #62
//!   (`dataset_cache/wands_scale/catalog_*x.jsonl`). Used for PLP/facet/
//!   filter/sort *performance* measurement. Only real WANDS fields are used
//!   (see #77's amendment) -- no brand, no price, no availability, no
//!   synthetic parent/variant grouping.
//! - Dataset B (this module's `fixture` submodule): a tiny, deterministic,
//!   hand-verifiable multi-variant fixture used *only* to gate
//!   product/variant correctness before any engine's PLP/facet numbers are
//!   trusted. Never used for headline performance numbers.

pub mod fixture;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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
    /// `benchmarks/configs/issue77/resource_envelope.env`'s
    /// `I77_*_CONTAINER` values -- deliberately `i77-`-prefixed and
    /// independent of #62's `i62-*` containers so nothing collides even if
    /// an #62 backfill were ever re-run.
    #[must_use]
    pub const fn container_name(self) -> &'static str {
        match self {
            Self::Native => "i77-native",
            Self::Solr => "i77-solr",
            Self::Elasticsearch => "i77-elasticsearch",
            Self::Opensearch => "i77-opensearch",
            Self::Typesense => "i77-typesense",
            Self::Meilisearch => "i77-meilisearch",
            Self::Vespa => "i77-vespa",
            Self::Havenask => "i77-havenask",
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

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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

pub const EXPERIMENT_ID: &str = "I77-E3";
pub const RAW_SCHEMA_VERSION: u32 = 1;

/// One correctness-oracle query result for one engine: did this engine's
/// live query return exactly the oracle's expected product-id set?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectnessCheck {
    pub query_name: String,
    pub expected_product_ids: Vec<String>,
    pub actual_product_ids: Vec<String>,
    pub passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Ok,
    CorrectnessFail,
    HarnessFailure,
    InvalidEnvironmental,
    EngineExcluded,
}
