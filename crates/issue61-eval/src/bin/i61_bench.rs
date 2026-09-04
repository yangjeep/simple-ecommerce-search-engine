use bench_harness::Distribution;
use issue61_eval::{
    check_calibration, load_workload, read_proc_stat, steal_percent, write_jsonl, CgroupReader,
    FrozenQuery, PairedBlock, RawRecord, RAW_SCHEMA_VERSION,
};
use std::error::Error;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "../bench_request.rs"]
mod bench_request;
#[cfg(test)]
use bench_request::{build_request, validate_response};
use bench_request::{parse_config, repeated_passes, workload_pass, Config, RequestEngine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Baseline,
    Treatment,
}

impl Engine {
    const fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Treatment => "treatment",
        }
    }
}

struct SplitMix64(u64);

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }
}

fn engine_order(seed: u64, blocks: usize) -> Vec<[Engine; 2]> {
    let mut random = SplitMix64::new(seed);
    let mut baseline_first = blocks / 2;
    let mut treatment_first = blocks - baseline_first;
    let mut orders = Vec::with_capacity(blocks);
    for remaining in (1..=blocks).rev() {
        let choose_baseline = if baseline_first == 0 {
            false
        } else if treatment_first == 0 {
            true
        } else {
            random.next() % u64::try_from(remaining).unwrap_or(1)
                < u64::try_from(baseline_first).unwrap_or(0)
        };
        if choose_baseline {
            orders.push([Engine::Baseline, Engine::Treatment]);
            baseline_first -= 1;
        } else {
            orders.push([Engine::Treatment, Engine::Baseline]);
            treatment_first -= 1;
        }
    }
    orders
}

fn calibration_pass_counts(passes: usize) -> Result<(usize, usize), String> {
    passes
        .checked_sub(1)
        .filter(|baseline| *baseline > 0)
        .map(|baseline| (baseline, passes))
        .ok_or_else(|| "calibration passes must be at least 2".to_string())
}

struct Arm<'a> {
    engine: Engine,
    url: &'a str,
    cgroup: &'a CgroupReader,
}

fn measure_arm(
    agent: &ureq::Agent,
    arm: Arm<'_>,
    workload: &[FrozenQuery],
    block: usize,
    order: usize,
    config: &Config,
) -> Result<RawRecord, Box<dyn Error>> {
    let cpu_before = arm.cgroup.snapshot()?;
    let steal_before = read_proc_stat(Path::new("/proc"))?;
    let wall_started = Instant::now();
    let request_engine = match arm.engine {
        Engine::Baseline => RequestEngine::Solr,
        Engine::Treatment => RequestEngine::Native,
    };
    let observations = workload_pass(agent, arm.url, workload, request_engine)?;
    let wall_elapsed_us = u64::try_from(wall_started.elapsed().as_micros())?;
    let steal_after = read_proc_stat(Path::new("/proc"))?;
    let cpu_after = arm.cgroup.snapshot()?;
    let delta = cpu_after.delta_since(&cpu_before)?;
    let latencies: Vec<f64> = observations
        .iter()
        .map(|observation| observation.latency_us)
        .collect();
    let _num_found_by_query: Vec<u64> = observations
        .iter()
        .map(|observation| observation.num_found)
        .collect();
    let distribution = Distribution::compute(&latencies);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .to_string();
    Ok(RawRecord {
        schema_version: RAW_SCHEMA_VERSION,
        experiment_id: "I61-E1".to_string(),
        run_id: format!("seed-{}", config.seed),
        rep: block,
        engine_order: order,
        calibration: false,
        engine: arm.engine.name().to_string(),
        dataset: config.dataset.clone(),
        query_class: "all".to_string(),
        regime: config.regime.clone(),
        queries: u64::try_from(workload.len())?,
        wall_elapsed_us,
        cpu_usage_usec: delta.usage_usec,
        cpu_user_usec: delta.user_usec,
        cpu_system_usec: delta.system_usec,
        cgroup_memory_footprint_bytes: cpu_after.memory_current_bytes,
        cgroup_memory_peak_bytes: cpu_after.memory_peak_bytes,
        index_serialized_bytes: 0,
        latency_p50_us: distribution.p50,
        latency_p95_us: distribution.p95,
        latency_p99_us: distribution.p99,
        latency_mean_us: distribution.mean,
        steal_pct: steal_percent(&steal_before, &steal_after),
        excluded: false,
        exclusion_reason: None,
        git_sha: std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
        host: std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
        timestamp_utc: timestamp,
    })
}

fn calibrate(
    agent: &ureq::Agent,
    config: &Config,
    workload: &[FrozenQuery],
) -> Result<issue61_eval::CalibrationCheck, Box<dyn Error>> {
    let (baseline_passes, treatment_passes) = calibration_pass_counts(config.calibration_passes)?;
    let reader = CgroupReader::at_dir(config.treatment_cgroup.clone());
    let mut paired = Vec::with_capacity(config.blocks);
    for (block, order) in engine_order(config.seed ^ 0xca11_ba7e, config.blocks)
        .into_iter()
        .enumerate()
    {
        let mut values = [0u64; 2];
        for arm in order {
            let passes = match arm {
                Engine::Baseline => baseline_passes,
                Engine::Treatment => treatment_passes,
            };
            let before = reader.snapshot()?;
            repeated_passes(
                agent,
                &config.treatment_url,
                workload,
                passes,
                RequestEngine::Native,
            )?;
            let delta = reader.snapshot()?.delta_since(&before)?;
            values[match arm {
                Engine::Baseline => 0,
                Engine::Treatment => 1,
            }] = delta.usage_usec;
        }
        paired.push(PairedBlock {
            block,
            baseline: values[0] as f64,
            treatment: values[1] as f64,
        });
    }
    let expected = config.calibration_passes as f64 / baseline_passes as f64;
    check_calibration(&paired, expected, 0.05)
        .ok_or_else(|| "calibration ratio was undefined".into())
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = parse_config(&std::env::args().collect::<Vec<_>>())?;
    let workload = load_workload(&config.workload)?;
    if workload.is_empty() {
        return Err("workload is empty".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    for _ in 0..config.warmup_passes {
        workload_pass(&agent, &config.baseline_url, &workload, RequestEngine::Solr)?;
        workload_pass(
            &agent,
            &config.treatment_url,
            &workload,
            RequestEngine::Native,
        )?;
    }
    let baseline_reader = CgroupReader::at_dir(config.baseline_cgroup.clone());
    let treatment_reader = CgroupReader::at_dir(config.treatment_cgroup.clone());
    let mut records = Vec::with_capacity(config.blocks * 2);
    for (block, order) in engine_order(config.seed, config.blocks)
        .into_iter()
        .enumerate()
    {
        for (position, engine) in order.into_iter().enumerate() {
            let arm = match engine {
                Engine::Baseline => Arm {
                    engine,
                    url: &config.baseline_url,
                    cgroup: &baseline_reader,
                },
                Engine::Treatment => Arm {
                    engine,
                    url: &config.treatment_url,
                    cgroup: &treatment_reader,
                },
            };
            records.push(measure_arm(
                &agent, arm, &workload, block, position, &config,
            )?);
        }
    }
    let calibration = calibrate(&agent, &config, &workload)?;
    write_jsonl(&config.output, &records)?;
    println!(
        "CALIBRATION observed={:.6} ci=[{:.6},{:.6}] expected={:.6} passed={}",
        calibration.observed.point_ratio,
        calibration.observed.ci_low,
        calibration.observed.ci_high,
        calibration.expected_ratio,
        calibration.passed
    );
    println!(
        "MEASUREMENT_OK blocks={} records={}",
        config.blocks,
        records.len()
    );
    if !calibration.passed {
        return Err("calibration gate failed".into());
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("i61_bench: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frozen_record() -> FrozenQuery {
        serde_json::from_str(
            r#"{
                "query_id":"q-structural",
                "text":"red beds",
                "admission_class":"Hybrid",
                "structural_constraint_count":2,
                "has_residual_lexical":true,
                "rows":10,
                "native":{"q":"red beds"},
                "solr":{
                    "q":"red",
                    "fq":["product_class_lc:\"beds\"","color_lc:\"red\""],
                    "params":{"defType":"edismax","qf":"title","fl":"id","rows":"10"}
                }
            }"#,
        )
        .expect("valid frozen record")
    }

    #[test]
    fn solr_request_includes_every_frozen_fq_as_a_repeated_parameter() {
        // Given
        let record = frozen_record();

        // When
        let request = build_request(&record, RequestEngine::Solr).expect("complete record");

        // Then
        assert_eq!(
            request.fq,
            [
                "product_class_lc:\"beds\"".to_string(),
                "color_lc:\"red\"".to_string()
            ]
        );
    }

    #[test]
    fn solr_request_includes_deftype_qf_and_fl_id_from_the_frozen_params() {
        // Given
        let record = frozen_record();

        // When
        let request = build_request(&record, RequestEngine::Solr).expect("complete record");

        // Then
        assert_eq!(
            request.params.get("defType").map(String::as_str),
            Some("edismax")
        );
        assert_eq!(request.params.get("qf").map(String::as_str), Some("title"));
        assert_eq!(request.params.get("fl").map(String::as_str), Some("id"));
    }

    #[test]
    fn native_request_sends_only_the_query_text_and_rows() {
        // Given
        let record = frozen_record();

        // When
        let request = build_request(&record, RequestEngine::Native).expect("complete record");

        // Then
        assert_eq!(request.q, "red beds");
        assert!(request.fq.is_empty());
        assert_eq!(request.params.len(), 1);
        assert_eq!(request.params.get("rows").map(String::as_str), Some("10"));
    }

    #[test]
    fn a_record_missing_its_per_engine_block_is_an_error_not_a_bare_q_fallback() {
        // Given
        let record: FrozenQuery = serde_json::from_str(
            r#"{"query_id":"q-missing","text":"beds","admission_class":"FastPath","structural_constraint_count":1,"has_residual_lexical":false,"rows":10,"native":{"q":"beds"}}"#,
        )
        .expect("missing engine blocks remain parseable for a query-specific error");

        // When
        let error = build_request(&record, RequestEngine::Solr).expect_err("missing Solr request");

        // Then
        assert!(error.contains("q-missing"));
    }

    #[test]
    fn validate_response_records_num_found() {
        // Given
        let body =
            r#"{"responseHeader":{"status":0},"response":{"numFound":12,"docs":[{"id":"P1"}]}}"#;

        // When
        let validated = validate_response(body).expect("valid response");

        // Then
        assert_eq!(validated.num_found, 12);
    }

    #[test]
    fn seeded_scheduler_is_deterministic_and_balanced_across_blocks() {
        let first = engine_order(61, 30);
        assert_eq!(first, engine_order(61, 30));
        assert_eq!(
            first
                .iter()
                .filter(|order| order[0] == Engine::Baseline)
                .count(),
            15
        );
    }

    #[test]
    fn response_validation_rejects_nonzero_status_missing_docs_and_non_string_ids() {
        assert!(
            validate_response(r#"{"responseHeader":{"status":500},"error":{"msg":"bad"}}"#)
                .is_err()
        );
        assert!(
            validate_response(r#"{"responseHeader":{"status":0},"response":{"numFound":0}}"#)
                .is_err()
        );
        assert!(validate_response(
            r#"{"responseHeader":{"status":0},"response":{"numFound":1,"docs":[{"id":7}]}}"#
        )
        .is_err());
    }

    #[test]
    fn response_validation_accepts_solr_shape_and_returns_total_count() {
        assert_eq!(
            validate_response(
                r#"{"responseHeader":{"status":0},"response":{"numFound":12,"docs":[{"id":"P1"}]}}"#
            )
            .expect("valid response")
            .num_found,
            12
        );
    }

    #[test]
    fn calibration_block_construction_yields_expected_pass_counts() {
        assert_eq!(calibration_pass_counts(5).expect("valid passes"), (4, 5));
        assert!(calibration_pass_counts(1).is_err());
    }
}
