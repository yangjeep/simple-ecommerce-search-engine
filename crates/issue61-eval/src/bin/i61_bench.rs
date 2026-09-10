use issue61_eval::{load_workload, project_workload, write_jsonl};
use std::error::Error;
use std::time::Duration;

#[path = "../bench_request.rs"]
mod bench_request;
use bench_request::parse_config;
#[path = "i61_bench/session.rs"]
mod session;
use session::measure_session;

fn run() -> Result<(), Box<dyn Error>> {
    let config = parse_config(&std::env::args().collect::<Vec<_>>())?;
    let workload = load_workload(&config.workload)?;
    let projected = project_workload(&workload, config.projection)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let record = measure_session(&agent, &config, projected)?;
    write_jsonl(&config.output, std::slice::from_ref(&record))?;
    println!(
        "MEASUREMENT_OK rep={} engine={} records=1",
        config.block.get(),
        config.engine.as_str()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("i61_bench: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
use bench_request::{build_request, validate_response, workload_pass};
#[cfg(test)]
#[path = "i61_bench/tests.rs"]
mod tests;
