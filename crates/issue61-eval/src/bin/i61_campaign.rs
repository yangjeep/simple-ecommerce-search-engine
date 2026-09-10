use issue61_eval::{
    campaign_plan, CampaignCycle, CampaignPhase, EXACT_INDEX_CELLS, STABILITY_CELLS,
};

struct DryRunConfig {
    cycle: CampaignCycle,
}

fn parse_config(args: &[String]) -> Result<DryRunConfig, String> {
    let cycle = match args {
        [_, dry_run, cycle_flag, value] if dry_run == "--dry-run" && cycle_flag == "--cycle" => {
            value.parse()?
        }
        [_, cycle_flag, value, dry_run] if cycle_flag == "--cycle" && dry_run == "--dry-run" => {
            value.parse()?
        }
        _ => {
            return Err(
                "execution is unavailable; expected exactly --dry-run --cycle <run1|rerun1|rerun2>"
                    .to_string(),
            )
        }
    };
    Ok(DryRunConfig { cycle })
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

fn run(args: &[String]) -> Result<String, String> {
    parse_config(args).map(|config| render(&config))
}

fn main() {
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(summary) => print!("{summary}"),
        Err(error) => {
            eprintln!("i61_campaign: {error}");
            std::process::exit(1);
        }
    }
}
