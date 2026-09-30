//! Issue #65 open-loop load generator (amendment 1 section 9).
//!
//! Usage:
//!   i65_load --pools <pools.json> --mix <primary|structural|lexical|no_lexical>
//!     --rate <qps> --treatment <b0|h1|native> --out <json>
//!     [--native-url http://127.0.0.1:9965] [--solr-url http://127.0.0.1:8985]
//!     [--warmup 10] [--duration 60] [--grace 10] [--threads 128]
//!     [--timeout-ms 2000] [--close-each true] [--noop <path>]
//!     [--cgroup total=/sys/fs/cgroup/...] [--cgroup solr=...] ...
//!
//! The arrival schedule (Poisson) and the request sequence are generated
//! before the run from (seed, mix, rate) and are identical for every
//! treatment. A dispatcher hands each request, at its scheduled time, to a
//! pool of client threads with keep-alive connections; latency runs from the
//! *scheduled* time to response completion, so queueing is included and
//! coordinated omission is avoided. Serving CPU/memory are read from the
//! given cgroups at the measured window's boundaries. The run is flagged
//! HARNESS_SATURATED if the generator's own CPU exceeds 90% of its core or
//! dispatch lateness P99 exceeds 5 ms.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use issue65_eval::cgroup::Group;
use issue65_eval::observe::check_fast;
use issue65_eval::workload::{sequence, sequence_hash, Pools};
use serde::Serialize;

const SOLR_SELECT: &str = "/solr/i77_wands/select";

struct Args {
    pools: String,
    mix: String,
    rate: f64,
    treatment: String,
    out: String,
    native_url: String,
    solr_url: String,
    warmup: f64,
    duration: f64,
    grace: f64,
    threads: usize,
    timeout_ms: u64,
    close_each: bool,
    noop: Option<String>,
    cgroups: Vec<Group>,
}

fn parse_args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in raw.chunks(2) {
        map.entry(pair[0].clone())
            .or_default()
            .push(pair.get(1).cloned().unwrap_or_default());
    }
    let one = |k: &str| map.get(k).and_then(|v| v.last().cloned());
    let req = |k: &str| one(k).unwrap_or_else(|| panic!("missing {k}"));
    Args {
        pools: req("--pools"),
        mix: req("--mix"),
        rate: req("--rate").parse().expect("rate"),
        treatment: req("--treatment"),
        out: req("--out"),
        native_url: one("--native-url").unwrap_or_else(|| "http://127.0.0.1:9965".into()),
        solr_url: one("--solr-url").unwrap_or_else(|| "http://127.0.0.1:8985".into()),
        warmup: one("--warmup").map_or(10.0, |v| v.parse().expect("warmup")),
        duration: one("--duration").map_or(60.0, |v| v.parse().expect("duration")),
        grace: one("--grace").map_or(10.0, |v| v.parse().expect("grace")),
        threads: one("--threads").map_or(128, |v| v.parse().expect("threads")),
        timeout_ms: one("--timeout-ms").map_or(2000, |v| v.parse().expect("timeout")),
        close_each: one("--close-each").as_deref() == Some("true"),
        noop: one("--noop"),
        cgroups: map
            .get("--cgroup")
            .map(|v| v.iter().map(|s| Group::parse(s).expect("cgroup")).collect())
            .unwrap_or_default(),
    }
}

/// What to send for one pool request under this treatment.
struct Target {
    url: String,
    body: Option<Vec<u8>>,
    solr: bool,
    route: &'static str,
}

struct Record {
    seq: usize,
    sched_us: u64,
    latency_us: u64,
    ok: bool,
    error: Option<String>,
}

#[derive(Serialize, Default, Clone)]
struct Stats {
    offered: usize,
    ok: usize,
    errors: usize,
    error_rate: f64,
    achieved_over_offered: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    mean_ms: f64,
}

fn percentile(sorted: &[u64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64 / 1e3
}

fn stats(records: &[&Record]) -> Stats {
    let mut lat: Vec<u64> = records.iter().map(|r| r.latency_us).collect();
    lat.sort_unstable();
    let ok = records.iter().filter(|r| r.ok).count();
    let offered = records.len();
    Stats {
        offered,
        ok,
        errors: offered - ok,
        error_rate: if offered == 0 {
            0.0
        } else {
            (offered - ok) as f64 / offered as f64
        },
        achieved_over_offered: if offered == 0 {
            0.0
        } else {
            ok as f64 / offered as f64
        },
        p50_ms: percentile(&lat, 50.0),
        p95_ms: percentile(&lat, 95.0),
        p99_ms: percentile(&lat, 99.0),
        max_ms: lat.last().map_or(f64::NAN, |v| *v as f64 / 1e3),
        mean_ms: if lat.is_empty() {
            f64::NAN
        } else {
            lat.iter().sum::<u64>() as f64 / lat.len() as f64 / 1e3
        },
    }
}

fn self_cpu_us() -> u64 {
    // SAFETY: getrusage writes into a zeroed struct we own.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let tv = |t: libc::timeval| t.tv_sec as u64 * 1_000_000 + t.tv_usec as u64;
    tv(usage.ru_utime) + tv(usage.ru_stime)
}

fn main() {
    let args = parse_args();
    let pools: Pools = serde_json::from_str(&std::fs::read_to_string(&args.pools).expect("pools"))
        .expect("pools json");
    let total_s = args.warmup + args.duration;
    let seq = sequence(&pools, &args.mix, args.rate, total_s);
    let seq_hash = sequence_hash(&seq);

    let targets: Vec<Target> = pools
        .requests
        .iter()
        .map(|r| {
            if let Some(path) = &args.noop {
                let base = if args.treatment == "b0" {
                    &args.solr_url
                } else {
                    &args.native_url
                };
                return Target {
                    url: format!("{base}{path}"),
                    body: None,
                    solr: false,
                    route: "noop",
                };
            }
            let solr_body = || Some(serde_json::to_vec(&r.solr_body).expect("json"));
            match (args.treatment.as_str(), &r.native_path) {
                ("b0", _) => Target {
                    url: format!("{}{SOLR_SELECT}", args.solr_url),
                    body: solr_body(),
                    solr: true,
                    route: "solr",
                },
                (_, Some(path)) => Target {
                    url: format!("{}{path}", args.native_url),
                    body: None,
                    solr: false,
                    route: "native",
                },
                ("h1", None) => Target {
                    url: format!("{}{SOLR_SELECT}", args.native_url),
                    body: solr_body(),
                    solr: true,
                    route: "solr_delegate",
                },
                // A native-only treatment has no route for class F. Mixes used
                // with it give F zero weight; this inert target is never
                // scheduled (asserted below).
                (_, None) => Target {
                    url: String::new(),
                    body: None,
                    solr: false,
                    route: "unroutable",
                },
            }
        })
        .collect();
    assert!(
        seq.iter().all(|s| targets[s.req].route != "unroutable"),
        "the {} mix schedules a class this treatment cannot route",
        args.mix
    );
    let targets = Arc::new(targets);
    let pools = Arc::new(pools);
    let seq = Arc::new(seq);

    let (tx, rx) = mpsc::channel::<(usize, Instant)>();
    let rx = Arc::new(Mutex::new(rx));
    let t0 = Instant::now() + Duration::from_millis(300);
    let mut workers = Vec::new();
    for _ in 0..args.threads {
        let (rx, targets, pools, seq) = (
            Arc::clone(&rx),
            Arc::clone(&targets),
            Arc::clone(&pools),
            Arc::clone(&seq),
        );
        let timeout = Duration::from_millis(args.timeout_ms);
        let close_each = args.close_each;
        let noop = args.noop.is_some();
        workers.push(std::thread::spawn(move || {
            let agent = ureq::AgentBuilder::new().timeout(timeout).build();
            let mut out: Vec<Record> = Vec::new();
            loop {
                let job = rx.lock().expect("rx").recv();
                let Ok((i, scheduled)) = job else { break };
                let s = seq[i];
                // Already past its deadline while queued client-side (the
                // server is not keeping up): a timeout, without sending, so
                // an overloaded run drains in bounded time.
                if scheduled.elapsed() > timeout {
                    out.push(Record {
                        seq: i,
                        sched_us: s.at_us,
                        latency_us: scheduled.elapsed().as_micros() as u64,
                        ok: false,
                        error: Some("timeout".to_owned()),
                    });
                    continue;
                }
                let target = &targets[s.req];
                let req = &pools.requests[s.req];
                let mut call = match &target.body {
                    Some(_) => agent
                        .post(&target.url)
                        .set("Content-Type", "application/json"),
                    None => agent.get(&target.url),
                };
                if close_each {
                    call = call.set("Connection", "close");
                }
                let result = match &target.body {
                    Some(b) => call.send_bytes(b),
                    None => call.call(),
                };
                let (ok, error) = match result {
                    Ok(resp) => match resp.into_string() {
                        Ok(body) => {
                            if noop || check_fast(&req.class, &req.expect, &body, target.solr) {
                                (true, None)
                            } else {
                                (false, Some("check".to_owned()))
                            }
                        }
                        Err(e) => (false, Some(format!("read: {e}"))),
                    },
                    Err(ureq::Error::Status(code, _)) => (false, Some(format!("http {code}"))),
                    Err(e) => (false, Some(format!("transport: {}", e.kind()))),
                };
                let latency = scheduled.elapsed();
                out.push(Record {
                    seq: i,
                    sched_us: s.at_us,
                    latency_us: latency.as_micros() as u64,
                    ok: ok && latency <= timeout,
                    error: if latency > timeout {
                        Some("timeout".to_owned())
                    } else {
                        error
                    },
                });
            }
            out
        }));
    }

    // Dispatcher: open loop on the pre-generated schedule.
    let window_start_us = (args.warmup * 1e6) as u64;
    let window_end_us = (total_s * 1e6) as u64;
    let mut lateness_us: Vec<u64> = Vec::with_capacity(seq.len());
    let mut cpu_start: Option<(Vec<Option<u64>>, u64, Instant)> = None;
    let mut cpu_end: Option<(Vec<Option<u64>>, u64, Instant)> = None;
    let snapshot = |groups: &[Group]| groups.iter().map(Group::cpu_usec).collect::<Vec<_>>();
    for (i, s) in seq.iter().enumerate() {
        let due = t0 + Duration::from_micros(s.at_us);
        if cpu_start.is_none() && s.at_us >= window_start_us {
            let at = t0 + Duration::from_micros(window_start_us);
            while Instant::now() < at {
                std::thread::sleep(at - Instant::now());
            }
            cpu_start = Some((snapshot(&args.cgroups), self_cpu_us(), Instant::now()));
        }
        loop {
            let now = Instant::now();
            if now >= due {
                break;
            }
            let wait = due - now;
            if wait > Duration::from_micros(300) {
                std::thread::sleep(wait - Duration::from_micros(200));
            } else {
                std::hint::spin_loop();
            }
        }
        lateness_us.push(due.elapsed().as_micros() as u64);
        tx.send((i, due)).expect("send");
    }
    let end_at = t0 + Duration::from_micros(window_end_us);
    while Instant::now() < end_at {
        std::thread::sleep(end_at - Instant::now());
    }
    cpu_end.get_or_insert((snapshot(&args.cgroups), self_cpu_us(), Instant::now()));
    let memory: BTreeMap<String, Option<u64>> = args
        .cgroups
        .iter()
        .map(|g| (g.name.clone(), g.memory_bytes()))
        .collect();
    drop(tx);
    let grace_deadline = Instant::now() + Duration::from_secs_f64(args.grace);
    let mut records: Vec<Record> = Vec::new();
    for w in workers {
        records.extend(w.join().expect("worker"));
    }
    let late_join = Instant::now() > grace_deadline;

    // Measured window: requests *scheduled* inside it. A request that never
    // completed is impossible here (workers drain the queue; the client
    // timeout bounds each request), so every scheduled request has a record.
    let window: Vec<&Record> = records
        .iter()
        .filter(|r| r.sched_us >= window_start_us && r.sched_us < window_end_us)
        .collect();
    let scheduled_in_window = seq
        .iter()
        .filter(|s| s.at_us >= window_start_us && s.at_us < window_end_us)
        .count();
    let overall = stats(&window);
    let mut per_class: BTreeMap<String, Stats> = BTreeMap::new();
    let mut per_route: BTreeMap<String, usize> = BTreeMap::new();
    let mut error_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_class: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for r in &window {
        let s = seq[r.seq];
        by_class
            .entry(pools.requests[s.req].class.clone())
            .or_default()
            .push(r);
        *per_route
            .entry(targets[s.req].route.to_owned())
            .or_insert(0) += 1;
        if let Some(e) = &r.error {
            *error_kinds.entry(e.clone()).or_insert(0) += 1;
        }
    }
    for (c, rs) in &by_class {
        per_class.insert(c.clone(), stats(rs));
    }
    let (cs, gs, ts) = cpu_start.expect("window start");
    let (ce, ge, te) = cpu_end.expect("window end");
    let wall_s = (te - ts).as_secs_f64();
    let cpu: BTreeMap<String, Option<f64>> = args
        .cgroups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            (
                g.name.clone(),
                match (cs[i], ce[i]) {
                    (Some(a), Some(b)) if b >= a => Some((b - a) as f64),
                    _ => None,
                },
            )
        })
        .collect();
    let total_cpu_us = cpu.values().next().copied().flatten();
    let generator_cpu_share = (ge - gs) as f64 / 1e6 / wall_s;
    lateness_us.sort_unstable();
    let lateness_p99_ms = percentile(&lateness_us, 99.0);
    let saturated = generator_cpu_share > 0.9 || lateness_p99_ms > 5.0;
    let pass = !saturated
        && overall.achieved_over_offered >= 0.98
        && overall.p95_ms < 50.0
        && overall.p99_ms < 100.0
        && overall.error_rate < 0.001
        && !error_kinds.contains_key("check");
    let report = serde_json::json!({
        "experiment_id": issue65_eval::EXPERIMENT_ID,
        "schema_version": issue65_eval::RAW_SCHEMA_VERSION,
        "treatment": args.treatment, "mix": args.mix, "offered_qps": args.rate,
        "warmup_s": args.warmup, "duration_s": args.duration, "threads": args.threads,
        "close_each": args.close_each, "noop": args.noop, "timeout_ms": args.timeout_ms,
        "pools_sha256": pools.hash(), "sequence_sha256": seq_hash,
        "scheduled_total": seq.len(), "scheduled_in_window": scheduled_in_window,
        "achieved_qps": overall.ok as f64 / args.duration,
        "overall": overall, "per_class": per_class, "per_route": per_route, "error_kinds": error_kinds,
        "cpu_usec": cpu, "window_wall_s": wall_s,
        "total_cpu_us_per_ok_query": total_cpu_us.map(|c| c / overall.ok.max(1) as f64),
        "total_cpu_utilization_of_3_cores": total_cpu_us.map(|c| c / 1e6 / wall_s / 3.0),
        "memory_current_bytes": memory,
        "generator_cpu_share": generator_cpu_share, "dispatch_lateness_p99_ms": lateness_p99_ms,
        "harness_saturated": saturated, "late_join": late_join,
        "slo": {"p95_ms": 50.0, "p99_ms": 100.0, "error_rate": 0.001, "achieved_over_offered": 0.98},
        "pass": pass,
        "timestamp_utc": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
    });
    std::fs::write(
        &args.out,
        serde_json::to_string_pretty(&report).expect("json"),
    )
    .expect("write");
    println!(
        "I65_LOAD treatment={} mix={} offered={} achieved={:.1} p95={:.2}ms p99={:.2}ms err={:.4} cpu/q={:?}us util={:?} gen_cpu={:.2} late_p99={:.2}ms saturated={} PASS={}",
        args.treatment, args.mix, args.rate, overall.ok as f64 / args.duration, overall.p95_ms, overall.p99_ms,
        overall.error_rate, report["total_cpu_us_per_ok_query"].as_f64().map(|v| v.round()),
        report["total_cpu_utilization_of_3_cores"].as_f64().map(|v| (v * 1000.0).round() / 1000.0),
        generator_cpu_share, lateness_p99_ms, saturated, pass
    );
}
