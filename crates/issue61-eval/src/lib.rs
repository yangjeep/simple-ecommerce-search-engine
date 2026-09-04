//! Experiment-only resource-measurement foundation for Issue #61; product
//! crates must never depend on this crate. Native search runs in-process while
//! Solr and Elasticsearch run as HTTP servers in containers, so uniform
//! server-side cgroup CPU accounting neutralizes that process-boundary
//! asymmetry.

mod catalog_data;
mod cgroup;
mod equivalence;
mod floor;
mod gate;
mod ratio;
mod raw;
mod request_contract;
mod sha256;
pub mod solr_contract;
mod steal;
mod workload;

pub use catalog_data::{load_dataset, Dataset, LoadedDataset};
pub use cgroup::{CgroupDelta, CgroupError, CgroupReader, CgroupSnapshot};
pub use equivalence::{
    audit_all, audit_query, EngineOutcome, EquivalenceReport, QueryAudit, QueryVerdict,
};
pub use floor::{
    is_above_floor, measure_timer_floor, required_batch_size, TimerFloor, MIN_FLOOR_MULTIPLE,
};
pub use gate::{
    evaluate, evaluate_cell, evaluate_exact_artifact, CellStability, GateReport, GateVerdict,
    MetricKind, ALPHA, MAX_CV, MAX_REL_HALFWIDTH, MIN_BLOCKS,
};
pub use ratio::{
    check_calibration, paired_ratio, CalibrationCheck, PairedBlock, RatioResult, RatioVerdict,
    MATERIALITY_RATIO,
};
pub use raw::{read_jsonl, write_jsonl, RawError, RawRecord, RAW_SCHEMA_VERSION};
pub use request_contract::{freeze_engine_requests, FreezeRequest, FrozenEngineRequests};
pub use sha256::sha256_hex;
pub use steal::{
    parse_proc_stat, read_proc_stat, should_exclude_rep, steal_percent, CpuTimes, StealError,
    STEAL_EXCLUSION_THRESHOLD_PCT,
};
pub use workload::{
    load_workload, write_workload, AdmissionClass, FrozenQuery, NativeRequest, SolrRequest,
};
