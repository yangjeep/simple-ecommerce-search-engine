use issue61_eval::{
    campaign_plan, lifecycle::live::validate_repository_root, CampaignCycle, CampaignPhase,
    EXACT_INDEX_CELLS, STABILITY_CELLS,
};
use std::path::PathBuf;

struct DryRunConfig {
    cycle: CampaignCycle,
}

struct ExecutionConfig {
    repository_root: PathBuf,
    cycle: CampaignCycle,
}

enum CampaignConfig {
    DryRun(DryRunConfig),
    Execute(ExecutionConfig),
}

enum CampaignError {
    Invocation(String),
    Execution(String),
}

fn parse_config(args: &[String]) -> Result<CampaignConfig, String> {
    match args {
        [_, dry_run, cycle_flag, value] if dry_run == "--dry-run" && cycle_flag == "--cycle" => {
            Ok(CampaignConfig::DryRun(DryRunConfig {
                cycle: value.parse()?,
            }))
        }
        [_, cycle_flag, value, dry_run] if cycle_flag == "--cycle" && dry_run == "--dry-run" => {
            Ok(CampaignConfig::DryRun(DryRunConfig {
                cycle: value.parse()?,
            }))
        }
        [_, execute, root_flag, root, cycle_flag, value]
            if execute == "--execute"
                && root_flag == "--repository-root"
                && cycle_flag == "--cycle" =>
        {
            Ok(CampaignConfig::Execute(ExecutionConfig {
                repository_root: PathBuf::from(root),
                cycle: value.parse()?,
            }))
        }
        _ => Err("expected exactly --dry-run --cycle <run1|rerun1|rerun2> or --execute --repository-root <canonical-root> --cycle <run1|rerun1|rerun2>".to_owned()),
    }
}

fn render(config: &DryRunConfig) -> String {
    let plan = campaign_plan(config.cycle);
    let warm_sessions = plan
        .sessions()
        .filter(|session| session.series().phase() == CampaignPhase::Warm)
        .count();
    let calibration_sessions = plan
        .sessions()
        .filter(|session| session.series().phase() == CampaignPhase::Calibration)
        .count();
    let cold_sessions = plan
        .sessions()
        .filter(|session| session.series().phase() == CampaignPhase::Cold)
        .count();
    format!(
        "I61_CAMPAIGN_DRY_RUN cycle={} logical_pairs={} sessions={} warm_sessions={} calibration_sessions={} cold_sessions={} stability_cells={} exact_index_cells={} external_commands=0 writes=0\n",
        plan.cycle().as_str(),
        plan.blocks().len(),
        plan.sessions().count(),
        warm_sessions,
        calibration_sessions,
        cold_sessions,
        STABILITY_CELLS,
        EXACT_INDEX_CELLS,
    )
}

fn run(args: &[String]) -> Result<String, CampaignError> {
    match parse_config(args).map_err(CampaignError::Invocation)? {
        CampaignConfig::DryRun(config) => Ok(render(&config)),
        CampaignConfig::Execute(config) => {
            validate_repository_root(&config.repository_root)
                .map_err(|error| CampaignError::Execution(error.to_string()))?;
            Err(CampaignError::Execution(format!(
                "live execution is not implemented for cycle {}",
                config.cycle.as_str()
            )))
        }
    }
}

fn main() {
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(summary) => print!("{summary}"),
        Err(CampaignError::Invocation(error)) => {
            eprintln!("i61_campaign: {error}");
            std::process::exit(1);
        }
        Err(CampaignError::Execution(error)) => {
            eprintln!("i61_campaign: {error}");
            std::process::exit(2);
        }
    }
}
