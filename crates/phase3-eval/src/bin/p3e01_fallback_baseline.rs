//! Issue #14 P3-E01: the transparent fallback baseline. Before trusting
//! any coverage/relevance number from a later admission-policy sweep,
//! measure the one cost every single real query pays regardless of
//! admission outcome -- `admission::admit()`'s own overhead on the
//! *reject* path, relative to a pure, unmodified Solr call. Issue #14's
//! invariant 1: fast-path miss must not materially hurt baseline latency
//! (initial target: <=5% fallback tax).
//!
//! Two arms, over the *same* real rejected queries, interleaved via
//! `bench_harness::round_robin_schedule` (never run back-to-back) so
//! neither arm is favored by drifting shared-system state:
//! - `solr_baseline`: `solr_search` alone, using the same
//!   `round1_eval::solr::solr_query_for` construction Phase 2's P1-D
//!   already validated as fair -- i.e. as if commerce-native did not
//!   exist for this query at all.
//! - `admission_then_solr`: `admission::admit()` (which returns `Reject`
//!   for every query in this sample, by construction) immediately
//!   followed by the identical `solr_search` call.
//!
//! `compile()` itself is excluded from both arms' timed block (computed
//! once into a cache before either arm runs), mirroring every prior
//! phase's own convention of excluding query-compilation cost from a
//! method's measured latency -- a query's *own* text must be understood
//! before either admission or a fq-enriched Solr query can be built at
//! all, so this is a symmetric exclusion on both arms, not a thumb on the
//! scale for either one.
//!
//! Usage: cargo run --release -p phase3-eval --bin p3e01_fallback_baseline
//!        [catalog.jsonl] [queries.jsonl] [solr_base_url]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use bench_harness::{round_robin_schedule, Distribution, RunManifest};
use commerce_core::admission::{admit, AdmissionDecision, AdmissionPolicy, RejectReason};
use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::BrandId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, CommerceQuery};
use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use round1_eval::catalog;
use round1_eval::data;
use round1_eval::solr::{extract_brand_color, solr_query_for, solr_search};

const SEED: u64 = 7;
const LATENCY_SAMPLE_SIZE: usize = 20;
const LATENCY_WARMUP: usize = 5;
const LATENCY_REPS: usize = 30;
/// P3-E01's own initial policy -- a conservative starting point for
/// measuring fallback-tax overhead, not yet a calibrated coverage/
/// relevance frontier point (that is P3-E02's job). Any reasonable
/// max_candidates value produces the same reject queries needed here;
/// this value matches P3-E02's own starting sweep point for continuity.
const INITIAL_POLICY: AdmissionPolicy = AdmissionPolicy { max_candidates: 50 };

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RejectReasonLabel {
    Ambiguous,
    UnresolvedResidual,
    NoStructuralConstraint,
    NotSelectiveEnough,
}

impl RejectReasonLabel {
    fn of(reason: RejectReason) -> Self {
        match reason {
            RejectReason::Ambiguous => RejectReasonLabel::Ambiguous,
            RejectReason::UnresolvedResidual => RejectReasonLabel::UnresolvedResidual,
            RejectReason::NoStructuralConstraint => RejectReasonLabel::NoStructuralConstraint,
            RejectReason::NotSelectiveEnough { .. } => RejectReasonLabel::NotSelectiveEnough,
        }
    }

    fn label(self) -> &'static str {
        match self {
            RejectReasonLabel::Ambiguous => "ambiguous",
            RejectReasonLabel::UnresolvedResidual => "unresolved_residual",
            RejectReasonLabel::NoStructuralConstraint => "no_structural_constraint",
            RejectReasonLabel::NotSelectiveEnough => "not_selective_enough",
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let catalog_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/catalog.jsonl"));
    let queries_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("dataset_cache/export/queries.jsonl"));
    let solr_base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8983/solr/commerce_bench".to_string());

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    println!("building commerce_core structural index...");
    let index = CatalogIndex::build(&ingested.catalog);

    println!("checking Solr ({solr_base_url})...");
    let solr_ping = solr_search(&solr_base_url, "*:*", &[], 0);
    let Some(ping) = solr_ping else {
        eprintln!("Solr NOT reachable at {solr_base_url} -- P3-E01 requires a live Solr instance to measure fallback tax against. Aborting.");
        std::process::exit(1);
    };
    println!("  Solr reachable: numFound={}", ping.num_found);

    let brand_name_by_id: HashMap<BrandId, String> = ingested
        .brands
        .iter()
        .map(|b| (b.id, b.name.clone()))
        .collect();
    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: BTreeMap<u64, String> = BTreeMap::new();
    {
        let mut has_relevant: BTreeMap<u64, bool> = BTreeMap::new();
        for j in &judgments {
            judged_by_query
                .entry(j.query_id)
                .or_insert_with(|| j.query.clone());
            *has_relevant.entry(j.query_id).or_insert(false) |= j.label.is_relevant();
        }
        judged_by_query.retain(|qid, _| has_relevant.get(qid).copied().unwrap_or(false));
    }
    println!(
        "{} distinct real judged queries with at least one relevant label loaded",
        judged_by_query.len()
    );

    // Whole-corpus admission pass (cheap: no Solr calls) -- context for
    // the reject-reason breakdown and this policy's raw coverage rate.
    let compiled_cache: BTreeMap<u64, CommerceQuery> = judged_by_query
        .iter()
        .map(|(&qid, text)| (qid, compile(text, &lexicon)))
        .collect();

    let mut admitted = 0usize;
    let mut reject_counts: BTreeMap<RejectReasonLabel, usize> = BTreeMap::new();
    let mut rejected_qids: Vec<u64> = Vec::new();
    for (&qid, compiled) in &compiled_cache {
        match admit(compiled, &index, &INITIAL_POLICY) {
            AdmissionDecision::Admit { .. } => admitted += 1,
            AdmissionDecision::Reject(reason) => {
                *reject_counts
                    .entry(RejectReasonLabel::of(reason))
                    .or_insert(0) += 1;
                rejected_qids.push(qid);
            }
        }
    }
    let total = compiled_cache.len();
    println!(
        "\n=== whole-corpus admission pass (max_candidates={}) ===",
        INITIAL_POLICY.max_candidates
    );
    println!(
        "  admitted: {admitted}/{total} ({:.2}%)",
        admitted as f64 / total as f64 * 100.0
    );
    for (reason, count) in &reject_counts {
        println!(
            "  rejected [{}]: {count}/{total} ({:.2}%)",
            reason.label(),
            *count as f64 / total as f64 * 100.0
        );
    }

    // Seeded random sample of rejected queries for the repeated-latency
    // sub-experiment (Issue #6's own adversarial review flagged Phase 2's
    // "smallest-N-by-native-id" sampling as non-random; Phase 3 draws
    // seeded-random samples from the start).
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);
    rejected_qids.shuffle(&mut rng);
    let latency_sample: Vec<u64> = rejected_qids
        .into_iter()
        .take(LATENCY_SAMPLE_SIZE)
        .collect();
    println!(
        "\n=== fallback-tax latency sub-experiment: {} rejected queries, {} reps/arm ===",
        latency_sample.len(),
        LATENCY_REPS
    );

    // Precompute (q, fq) for every sampled query -- excluded from the
    // timed block on both arms, matching the compile-cost exclusion above.
    let solr_query_cache: BTreeMap<u64, (String, Vec<String>)> = latency_sample
        .iter()
        .map(|&qid| {
            let compiled = &compiled_cache[&qid];
            let text = &judged_by_query[&qid];
            let (brand, color) = extract_brand_color(&compiled.constraints, &brand_name_by_id);
            let (q, fq) = solr_query_for(
                text,
                &compiled.residual_lexical,
                brand.as_deref(),
                color.as_deref(),
            );
            (qid, (q, fq))
        })
        .collect();

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Arm {
        SolrBaseline,
        AdmissionThenSolr,
    }
    impl Arm {
        fn label(self) -> &'static str {
            match self {
                Arm::SolrBaseline => "solr_baseline",
                Arm::AdmissionThenSolr => "admission_then_solr",
            }
        }
    }
    let arms = [Arm::SolrBaseline, Arm::AdmissionThenSolr];
    let schedule = round_robin_schedule(arms.len(), LATENCY_REPS, SEED);

    for _ in 0..LATENCY_WARMUP {
        for &qid in &latency_sample {
            let (q, fq) = &solr_query_cache[&qid];
            let _ = solr_search(&solr_base_url, q, fq, 10);
            let compiled = &compiled_cache[&qid];
            let _ = admit(compiled, &index, &INITIAL_POLICY);
        }
    }

    let mut samples: BTreeMap<Arm, Vec<f64>> = BTreeMap::new();
    for (i, &arm_idx) in schedule.iter().enumerate() {
        let arm = arms[arm_idx];
        let qid = latency_sample[i % latency_sample.len()];
        let (q, fq) = &solr_query_cache[&qid];
        let compiled = &compiled_cache[&qid];

        let start = Instant::now();
        match arm {
            Arm::SolrBaseline => {
                let _ = solr_search(&solr_base_url, q, fq, 10);
            }
            Arm::AdmissionThenSolr => {
                let decision = admit(compiled, &index, &INITIAL_POLICY);
                assert!(
                    !decision.is_admit(),
                    "P3-E01's latency sample must contain only rejected queries: qid={qid}"
                );
                let _ = solr_search(&solr_base_url, q, fq, 10);
            }
        }
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        samples.entry(arm).or_default().push(elapsed_ms);
    }

    let artifacts_dir = PathBuf::from("dataset_cache/p3e01_artifacts");
    let manifest = RunManifest::capture(
        "p3e01_fallback_baseline",
        "solr_baseline_vs_admission_then_solr",
        &catalog_path,
        &queries_path,
        serde_json::json!({
            "max_candidates": INITIAL_POLICY.max_candidates,
            "latency_sample_size": LATENCY_SAMPLE_SIZE,
            "latency_reps": LATENCY_REPS,
            "solr_base_url": solr_base_url,
        }),
        SEED,
        LATENCY_WARMUP,
        LATENCY_REPS,
    );
    manifest.print();
    manifest
        .write_json(&artifacts_dir.join("manifest.json"))
        .ok();

    for &arm in &arms {
        let s = &samples[&arm];
        let d = Distribution::compute(s);
        d.print(arm.label(), "ms");
        bench_harness::append_raw_samples(
            &artifacts_dir.join("raw_latency_samples.csv"),
            "p3e01_fallback_baseline",
            arm.label(),
            s,
        )
        .ok();
        bench_harness::append_summary_row(
            &artifacts_dir.join("summary_latency.csv"),
            "p3e01_fallback_baseline",
            arm.label(),
            "latency_ms",
            &d,
        )
        .ok();
    }

    let baseline = &samples[&Arm::SolrBaseline];
    let with_admission = &samples[&Arm::AdmissionThenSolr];
    let ci = bench_harness::bootstrap_ci_diff_of_means(baseline, with_admission, 5000, 0.05, SEED);
    let baseline_mean = baseline.iter().sum::<f64>() / baseline.len() as f64;
    let tax_pct = ci.diff / baseline_mean * 100.0;
    println!(
        "\nfallback tax (admission_then_solr - solr_baseline), ms: diff={:.4} ci=[{:.4}, {:.4}] excludes_zero={}",
        ci.diff, ci.ci_low, ci.ci_high, ci.excludes_zero()
    );
    println!(
        "fallback tax as % of solr_baseline mean ({:.4}ms): {:.2}%",
        baseline_mean, tax_pct
    );
    println!(
        "Issue #14 invariant 1 target: <=5% fallback tax -- {}",
        if tax_pct.abs() <= 5.0 {
            "MET"
        } else {
            "NOT MET at this sample size -- see threats to validity before concluding"
        }
    );

    println!("\nartifacts written to {}", artifacts_dir.display());
}
