//! R1-E07: is the already-built `lexical_postings` structure (Gate 3, no
//! query path has ever read it) a viable fast-retrieval primitive for the
//! lexical side of real queries, and if so, how much of E04's relevance gap
//! against Solr does raw retrieval speed actually close?
//!
//! Two independent questions, kept separate:
//!  1. Latency: does whole-word token-postings lookup close most of R1-E05's
//!     ~961ms unnarrowed-Text-scan gap?
//!  2. Achievable (unranked) recall: does the *candidate set* a token lookup
//!     returns even contain the real relevant products, before any ranking?
//!     This is a generous upper bound (no top-K cutoff), not comparable to
//!     Solr's ranked Recall@10 without that caveat -- reported as such.
//!
//! Usage: cargo run --release -p round1-eval --bin lexical_postings_eval
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use commerce_core::index::{tokenize, CatalogIndex};
use round1_eval::catalog;
use round1_eval::data::{self, EsciLabel};

type JudgedProduct = (String, EsciLabel);

fn percentile_ms(sorted_ns: &[u128], p: f64) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let idx = ((sorted_ns.len() as f64 - 1.0) * p).round() as usize;
    sorted_ns[idx] as f64 / 1_000_000.0
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

    println!("loading + indexing real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    let index = CatalogIndex::build(&ingested.catalog);
    println!(
        "ready: {} products indexed",
        ingested.catalog.products.len()
    );

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut by_query: HashMap<u64, (String, Vec<JudgedProduct>)> = HashMap::new();
    for j in &judgments {
        by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), Vec::new()))
            .1
            .push((j.product_id.clone(), j.label));
    }
    let mut queries: Vec<(u64, String, Vec<JudgedProduct>)> = by_query
        .into_iter()
        .map(|(id, (text, labels))| (id, text, labels))
        .collect();
    queries.sort_by_key(|(id, _, _)| *id);
    println!("{} distinct real queries with judgments", queries.len());

    // --- Question 1: latency, both AND-mode and OR-mode, vs R1-E05's
    // Case 1 baseline (961ms p50 for an unnarrowed substring scan). ---
    let tokenized: Vec<Vec<String>> = queries
        .iter()
        .map(|(_, text, _)| tokenize(text).collect())
        .collect();

    let mut and_samples = Vec::with_capacity(queries.len());
    for tokens in &tokenized {
        let start = Instant::now();
        std::hint::black_box(index.lexical_and_candidates(std::hint::black_box(tokens)));
        and_samples.push(start.elapsed().as_nanos());
    }
    and_samples.sort_unstable();

    let mut or_samples = Vec::with_capacity(queries.len());
    for tokens in &tokenized {
        let start = Instant::now();
        std::hint::black_box(index.lexical_or_candidates(std::hint::black_box(tokens)));
        or_samples.push(start.elapsed().as_nanos());
    }
    or_samples.sort_unstable();

    println!();
    println!(
        "=== Latency: token-postings lookup, one call per real query, n={} ===",
        queries.len()
    );
    println!(
        "  AND-mode: p50={:.4}ms  p95={:.4}ms  p99={:.4}ms",
        percentile_ms(&and_samples, 0.5),
        percentile_ms(&and_samples, 0.95),
        percentile_ms(&and_samples, 0.99)
    );
    println!(
        "  OR-mode:  p50={:.4}ms  p95={:.4}ms  p99={:.4}ms",
        percentile_ms(&or_samples, 0.5),
        percentile_ms(&or_samples, 0.95),
        percentile_ms(&or_samples, 0.99)
    );
    println!("  (R1-E05 baseline: unnarrowed substring Text scan p50=961.23ms)");

    // --- Question 2: achievable (unranked) recall + zero-result rate. ---
    let mut and_zero_results = 0usize;
    let mut or_zero_results = 0usize;
    let mut and_relevant_total = 0usize;
    let mut and_relevant_found = 0usize;
    let mut or_relevant_total = 0usize;
    let mut or_relevant_found = 0usize;
    let mut and_exact_total = 0usize;
    let mut and_exact_found = 0usize;
    let mut or_exact_total = 0usize;
    let mut or_exact_found = 0usize;

    for ((_, _, labels), tokens) in queries.iter().zip(tokenized.iter()) {
        let and_bm = index.lexical_and_candidates(tokens);
        let or_bm = index.lexical_or_candidates(tokens);
        if and_bm.is_empty() {
            and_zero_results += 1;
        }
        if or_bm.is_empty() {
            or_zero_results += 1;
        }

        for (asin, label) in labels {
            let Some(&product_id) = ingested.asin_to_product_id.get(asin) else {
                continue;
            };
            // Real catalog: exactly one variant per product, VariantId ==
            // ProductId numerically (see round1-eval/src/catalog.rs).
            let variant_id = commerce_core::domain::VariantId(product_id.0);
            let Some(ordinal) = index.ordinal_of(variant_id) else {
                continue;
            };

            if label.is_relevant() {
                and_relevant_total += 1;
                or_relevant_total += 1;
                if and_bm.contains(ordinal) {
                    and_relevant_found += 1;
                }
                if or_bm.contains(ordinal) {
                    or_relevant_found += 1;
                }
            }
            if *label == EsciLabel::Exact {
                and_exact_total += 1;
                or_exact_total += 1;
                if and_bm.contains(ordinal) {
                    and_exact_found += 1;
                }
                if or_bm.contains(ordinal) {
                    or_exact_found += 1;
                }
            }
        }
    }

    println!();
    println!("=== Achievable (unranked, no top-K cutoff) candidate-set recall ===");
    println!(
        "  AND-mode: zero-result rate={:.1}% ({}/{})  recall vs Exact+Substitute={:.1}% ({}/{})  recall vs Exact only={:.1}% ({}/{})",
        and_zero_results as f64 / queries.len() as f64 * 100.0,
        and_zero_results,
        queries.len(),
        and_relevant_found as f64 / and_relevant_total.max(1) as f64 * 100.0,
        and_relevant_found,
        and_relevant_total,
        and_exact_found as f64 / and_exact_total.max(1) as f64 * 100.0,
        and_exact_found,
        and_exact_total
    );
    println!(
        "  OR-mode:  zero-result rate={:.1}% ({}/{})  recall vs Exact+Substitute={:.1}% ({}/{})  recall vs Exact only={:.1}% ({}/{})",
        or_zero_results as f64 / queries.len() as f64 * 100.0,
        or_zero_results,
        queries.len(),
        or_relevant_found as f64 / or_relevant_total.max(1) as f64 * 100.0,
        or_relevant_found,
        or_relevant_total,
        or_exact_found as f64 / or_exact_total.max(1) as f64 * 100.0,
        or_exact_found,
        or_exact_total
    );
    println!();
    println!("(Solr E04 baseline, ranked top-10, not directly comparable: zero-result rate=0.2%, Recall@10=0.1811)");

    // --- Candidate-set size: high recall is a hollow finding if it's
    // achieved by matching a large fraction of the whole catalog (any
    // "return everything" retriever gets ~100% recall trivially). ---
    let mut and_sizes: Vec<u64> = tokenized
        .iter()
        .map(|tokens| index.lexical_and_candidates(tokens).len())
        .collect();
    let mut or_sizes: Vec<u64> = tokenized
        .iter()
        .map(|tokens| index.lexical_or_candidates(tokens).len())
        .collect();
    and_sizes.sort_unstable();
    or_sizes.sort_unstable();
    let pct = |sorted: &[u64], p: f64| -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
    };
    let total = ingested.catalog.products.len() as f64;
    println!();
    println!(
        "=== Candidate-set size (how wide is the net?), n={} real queries ===",
        queries.len()
    );
    println!(
        "  AND-mode: p50={} ({:.3}% of catalog)  p95={} ({:.3}%)  p99={} ({:.3}%)",
        pct(&and_sizes, 0.5),
        pct(&and_sizes, 0.5) as f64 / total * 100.0,
        pct(&and_sizes, 0.95),
        pct(&and_sizes, 0.95) as f64 / total * 100.0,
        pct(&and_sizes, 0.99),
        pct(&and_sizes, 0.99) as f64 / total * 100.0
    );
    println!(
        "  OR-mode:  p50={} ({:.3}% of catalog)  p95={} ({:.3}%)  p99={} ({:.3}%)",
        pct(&or_sizes, 0.5),
        pct(&or_sizes, 0.5) as f64 / total * 100.0,
        pct(&or_sizes, 0.95),
        pct(&or_sizes, 0.95) as f64 / total * 100.0,
        pct(&or_sizes, 0.99),
        pct(&or_sizes, 0.99) as f64 / total * 100.0
    );
}
