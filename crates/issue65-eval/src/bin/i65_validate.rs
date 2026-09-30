//! Issue #65 correctness pre-pass (amendment 1 section 5), against live
//! serving processes, before any load.
//!
//! Usage: i65_validate --pools <in.json> --out-pools <out.json> --report <json>
//!          [--solr http://127.0.0.1:8985] [--native http://127.0.0.1:9965]
//!          [--router http://127.0.0.1:9965] [--threads 32] [--rounds 3]
//!
//! 1. Solr (B0): every A-E request vs the oracle expectation; a mismatch is
//!    NOT_EQUIVALENT_WORK and the request is excluded from BOTH treatments.
//!    Every F request's ranked top-48 ids and `num_found` are recorded as
//!    its expectation.
//! 2. Native (N1): every A-E request must equal the oracle exactly (a
//!    failure is a correctness failure: exit 1, stop).
//! 3. Router (H1): every F request through N1's delegate path must return
//!    exactly the recorded Solr ids and `num_found` (exit 1 otherwise).
//! 4. Concurrency: every A-E request, shuffled, `rounds` times, over
//!    `threads` concurrent connections, must return exactly the
//!    single-threaded observation (exit 1 otherwise).

use std::sync::{Arc, Mutex};

use issue65_eval::observe::{check_full, parse, Observed};
use issue65_eval::workload::Pools;

const SOLR_SELECT: &str = "/solr/i77_wands/select";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn sort_field(body: &serde_json::Value) -> Option<String> {
    body["sort"]
        .as_str()
        .and_then(|s| s.split_whitespace().next())
        .map(str::to_owned)
}

fn post(agent: &ureq::Agent, url: &str, body: &serde_json::Value) -> Result<String, String> {
    agent
        .post(url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
}

fn get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut pools: Pools = serde_json::from_str(
        &std::fs::read_to_string(arg(&args, "--pools").expect("--pools")).expect("read"),
    )
    .expect("json");
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let mut report = serde_json::Map::new();
    let mut fatal = Vec::new();

    if let Some(solr) = arg(&args, "--solr") {
        let url = format!("{solr}{SOLR_SELECT}");
        let mut not_equivalent = Vec::new();
        let mut recorded = 0;
        for r in &mut pools.requests {
            let body = post(&agent, &url, &r.solr_body);
            if r.class == "F" {
                match body.and_then(|b| parse(&b, true, None)) {
                    Ok(got) => {
                        r.expect.num_found = got.num_found;
                        r.expect.hit_count = Some(got.ids.len() as u64);
                        r.expect.ids = Some(got.ids);
                        recorded += 1;
                    }
                    Err(e) => not_equivalent.push(serde_json::json!({"id": r.id, "error": e})),
                }
                continue;
            }
            let outcome = body
                .and_then(|b| parse(&b, true, sort_field(&r.solr_body).as_deref()))
                .and_then(|got| check_full(&r.expect, &got));
            if let Err(e) = outcome {
                not_equivalent.push(serde_json::json!({"id": r.id, "class": r.class, "error": e}));
                pools.excluded.push(r.id.clone());
            }
        }
        println!(
            "SOLR: F recorded={recorded} not_equivalent={}",
            not_equivalent.len()
        );
        report.insert(
            "solr_not_equivalent".into(),
            serde_json::json!(not_equivalent),
        );
        report.insert("f_recorded".into(), serde_json::json!(recorded));
    }

    let mut reference: Vec<Option<Observed>> = vec![None; pools.requests.len()];
    if let Some(native) = arg(&args, "--native") {
        let mut failures = Vec::new();
        let mut checked = 0;
        for (i, r) in pools.requests.iter().enumerate() {
            let Some(path) = &r.native_path else { continue };
            let outcome =
                get(&agent, &format!("{native}{path}")).and_then(|b| parse(&b, false, None));
            match outcome.and_then(|got| check_full(&r.expect, &got).map(|()| got)) {
                Ok(got) => reference[i] = Some(got),
                Err(e) => {
                    failures.push(serde_json::json!({"id": r.id, "class": r.class, "error": e}))
                }
            }
            checked += 1;
        }
        let correctness = get(&agent, &format!("{native}/correctness")).unwrap_or_default();
        let fixture_ok = serde_json::from_str::<serde_json::Value>(&correctness)
            .map(|v| v["all_passed"] == serde_json::json!(true))
            .unwrap_or(false);
        println!(
            "NATIVE: checked={checked} failures={} fixture_ok={fixture_ok}",
            failures.len()
        );
        if !failures.is_empty() || !fixture_ok {
            fatal.push("native correctness".to_owned());
        }
        report.insert("native_checked".into(), serde_json::json!(checked));
        report.insert("native_failures".into(), serde_json::json!(failures));
        report.insert("fixture_ok".into(), serde_json::json!(fixture_ok));

        // Concurrency: shuffled, repeated, over many connections.
        let threads: usize = arg(&args, "--threads").map_or(32, |v| v.parse().expect("threads"));
        let rounds: usize = arg(&args, "--rounds").map_or(3, |v| v.parse().expect("rounds"));
        let mut work: Vec<usize> = (0..pools.requests.len())
            .filter(|&i| reference[i].is_some())
            .collect();
        let mut all = Vec::new();
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..rounds {
            for j in (1..work.len()).rev() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                work.swap(j, (state % (j as u64 + 1)) as usize);
            }
            all.extend(work.iter().copied());
        }
        let queue = Arc::new(Mutex::new(all));
        let pools_arc = Arc::new(pools.clone());
        let reference = Arc::new(reference);
        let mismatches = Arc::new(Mutex::new(Vec::new()));
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let (queue, pools, reference, mismatches, native) = (
                    Arc::clone(&queue),
                    Arc::clone(&pools_arc),
                    Arc::clone(&reference),
                    Arc::clone(&mismatches),
                    native.clone(),
                );
                std::thread::spawn(move || {
                    let agent = ureq::AgentBuilder::new()
                        .timeout(std::time::Duration::from_secs(30))
                        .build();
                    let mut done = 0usize;
                    loop {
                        let Some(i) = queue.lock().expect("queue").pop() else {
                            break;
                        };
                        let r = &pools.requests[i];
                        let got = get(
                            &agent,
                            &format!("{native}{}", r.native_path.as_deref().unwrap_or("")),
                        )
                        .and_then(|b| parse(&b, false, None));
                        if got.as_ref().ok() != reference[i].as_ref() {
                            mismatches.lock().expect("m").push(r.id.clone());
                        }
                        done += 1;
                    }
                    done
                })
            })
            .collect();
        let total: usize = handles.into_iter().map(|h| h.join().expect("join")).sum();
        let mismatches = mismatches.lock().expect("m").clone();
        println!(
            "CONCURRENCY: requests={total} threads={threads} mismatches={}",
            mismatches.len()
        );
        if !mismatches.is_empty() {
            fatal.push("concurrency".to_owned());
        }
        report.insert("concurrency".into(), serde_json::json!({"requests": total, "threads": threads, "rounds": rounds, "mismatches": mismatches}));
    }

    if let Some(router) = arg(&args, "--router") {
        let url = format!("{router}{SOLR_SELECT}");
        let mut failures = Vec::new();
        let mut checked = 0;
        for r in pools.requests.iter().filter(|r| r.class == "F") {
            let outcome = post(&agent, &url, &r.solr_body)
                .and_then(|b| parse(&b, true, None))
                .and_then(|got| check_full(&r.expect, &got));
            if let Err(e) = outcome {
                failures.push(serde_json::json!({"id": r.id, "error": e}));
            }
            checked += 1;
        }
        println!("ROUTER: F checked={checked} failures={}", failures.len());
        if !failures.is_empty() {
            fatal.push("router lexical identity".to_owned());
        }
        report.insert("router_checked".into(), serde_json::json!(checked));
        report.insert("router_failures".into(), serde_json::json!(failures));
    }

    pools.excluded.sort();
    pools.excluded.dedup();
    report.insert("excluded".into(), serde_json::json!(pools.excluded));
    report.insert("pools_sha256".into(), serde_json::json!(pools.hash()));
    report.insert("fatal".into(), serde_json::json!(fatal));
    if let Some(out) = arg(&args, "--out-pools") {
        std::fs::write(out, serde_json::to_string_pretty(&pools).expect("json"))
            .expect("write pools");
    }
    std::fs::write(
        arg(&args, "--report").expect("--report"),
        serde_json::to_string_pretty(&report).expect("json"),
    )
    .expect("write");
    println!(
        "I65_VALIDATE excluded={} fatal={fatal:?}",
        pools.excluded.len()
    );
    if !fatal.is_empty() {
        std::process::exit(1);
    }
}
