use athos80_eval::{Engine, ReplayInput};
use serde_json::json;
use std::io::{Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = String::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bm25 = args.iter().any(|v| v == "--bm25");
    if let Some(path) = args.iter().find(|v| !v.starts_with("--")) {
        source = std::fs::read_to_string(path)?;
    } else {
        std::io::stdin().read_to_string(&mut source)?;
    }
    let input: ReplayInput = serde_json::from_str(&source)?;
    let engine = if bm25 {
        #[cfg(feature = "bm25")]
        {
            Engine::new_with_bm25(input.catalog)?
        }
        #[cfg(not(feature = "bm25"))]
        {
            return Err("--bm25 requires build --features bm25".into());
        }
    } else {
        Engine::new(input.catalog)?
    };
    let responses = input
        .requests
        .iter()
        .map(|r| engine.search(r))
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &json!({ "build_micros": engine.build_micros, "responses": responses }),
    )?;
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}
