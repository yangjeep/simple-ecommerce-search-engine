use issue61_eval::run_completed_analysis_cli;

fn main() {
    match run_completed_analysis_cli(&std::env::args_os().collect::<Vec<_>>()) {
        Ok(analysis) => {
            print!("{}", analysis.summary());
            std::process::exit(i32::from(analysis.exit_code()));
        }
        Err(error) => {
            eprintln!("i61_analyze: {error}");
            std::process::exit(2);
        }
    }
}
