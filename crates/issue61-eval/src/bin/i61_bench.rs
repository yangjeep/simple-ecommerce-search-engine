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
#[path = "i61_bench/schedule.rs"]
mod schedule;
use schedule::{calibration_pass_counts, engine_order, Engine};

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
#[path = "i61_bench/tests.rs"]
mod tests;
