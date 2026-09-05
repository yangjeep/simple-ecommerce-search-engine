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
mod lexical_tokens;
mod memory_sampler;
mod native_candidates;
mod protocol;
mod ratio;
mod raw;
mod request_contract;
mod sha256;
pub mod solr_contract;
mod solr_pagination;
mod steal;
mod workload;
mod workload_projection;

pub use catalog_data::{load_dataset, Dataset, LoadedDataset};
pub use cgroup::{
    CgroupDelta, CgroupError, CgroupReader, CgroupSnapshot, MemoryEvents, MemorySwapEvents,
};
pub use equivalence::{
    audit_all, audit_candidate_sets, audit_query, candidate_digest, AuditVerdict,
    CandidateAuditRecord, EngineOutcome, EquivalenceReport, QueryAudit, QueryVerdict,
};
pub use floor::{
    is_above_floor, measure_timer_floor, required_batch_size, validate_measurement_window,
    MeasurementFloorError, TimerFloor, MIN_CGROUP_CPU_USEC, MIN_FLOOR_MULTIPLE,
};
pub use gate::{
    evaluate, evaluate_cell, evaluate_exact_artifact, CellStability, GateReport, GateVerdict,
    MetricKind, ALPHA, MAX_CV, MAX_REL_HALFWIDTH, MIN_BLOCKS,
};
pub use memory_sampler::{
    summarize_memory_samples, MemorySampleSummary, MemorySampler, MemorySamplerError,
    MEMORY_SAMPLE_INTERVAL,
};
pub use native_candidates::{frozen_native_query, native_candidate_ids};
pub use protocol::{
    campaign_schedule, BlockIndex, CampaignSeed, Engine, EngineOrder, EnginePair, SessionMode,
    SessionPlan, SessionStep, CAMPAIGN_BLOCKS,
};
pub use ratio::{
    check_calibration, paired_ratio, CalibrationCheck, PairedBlock, RatioResult, RatioVerdict,
    MATERIALITY_RATIO,
};
pub use raw::{read_jsonl, write_jsonl, RawError, RawRecord, RAW_SCHEMA_VERSION};
pub use request_contract::{freeze_engine_requests, FreezeRequest, FrozenEngineRequests};
pub use sha256::sha256_hex;
pub use solr_pagination::{
    fetch_complete_solr, prepare_solr_request, CursorCollector, CursorDecision,
    PreparedSolrRequest, SolrPage, AUDIT_PAGE_ROWS,
};
pub use steal::{
    assess_assigned_cpu_steal, parse_assigned_proc_stat, parse_proc_stat, probe_assigned_cpu_steal,
    read_proc_stat, run_assigned_cpu_steal_probe, should_exclude_rep, steal_percent, CpuSet,
    CpuTimes, StealError, StealProbeConfig, StealProbeResult, STEAL_EXCLUSION_THRESHOLD_PCT,
    STEAL_PROBE_DURATION,
};
pub use workload::{
    load_workload, write_workload, AdmissionClass, FrozenQuery, NativeRequest, SolrRequest,
};
pub use workload_projection::{project_workload, ProjectedWorkload, WorkloadProjection};
