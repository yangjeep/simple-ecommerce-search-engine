//! Issue #34 Phase 9, P9-E03: isolates Hypothesis 2 behind P9-E02's REVISE
//! -- is the 330/480 (68.75%) `Punt`-routed traffic share because those
//! queries genuinely have no related structural entity in the catalog, or
//! because `compile_lexicon`'s exact-lowercased-phrase matching misses a
//! real, related entity due to a literal vocabulary mismatch (plural vs.
//! singular, a pipe-delimited multi-value `product_class` ingested as one
//! opaque compound name, or a category name only ever stored as a full
//! compound hierarchical path)?
//!
//! **Hypotheses, each independently falsifiable**:
//! - H2a (pipe-split): a real product_class fragment (WANDS's raw
//!   `product_class` field is occasionally pipe-delimited, e.g.
//!   "Bookcases|Wall Mounted Shelves", confirmed by direct scan:
//!   2,247/42,994 products, 5.2% -- ingested today as one opaque
//!   ProductType name that can never match a single-class query) would
//!   match a query that currently produces zero constraints.
//! - H2b (category leaf segment): `category_leaf` is always the full
//!   slash-joined hierarchical path (confirmed by direct scan, e.g.
//!   "Furniture / Bedroom Furniture / Beds & Headboards / Beds / Twin
//!   Beds") -- the LAST path segment alone (the leaf) would match a query
//!   that currently produces zero constraints.
//! - H2c (plural/singular): a simple trailing-"s" normalization of either
//!   the query token or the catalog's `product_class` value would match a
//!   query that currently produces zero constraints.
//! - H2d (substring/near-miss): the query text contains a real
//!   `product_class` value as a substring, or vice versa, without an
//!   exact whole-token match (a looser catch-all signal, deliberately
//!   reported separately since it is not itself a proposed fix).
//!
//! **Decision criteria, stated before running**: each hypothesis is
//! scored as the fraction of the 330 currently-zero-constraint queries it
//! alone would recover a real entity match for (mechanisms are non-
//! exclusive -- a query can be counted under more than one). A mechanism
//! recovering a material share (>=10% of the 330) is real, disclosed
//! evidence that this specific literal-matching gap (not a deeper "no
//! related entity exists" problem) is contributing to the reproduced
//! structural-routed relevance gap, and names a concrete, scoped
//! candidate fix. A mechanism recovering a negligible share is evidence
//! against that specific hypothesis, reported as such, not discarded
//! silently.
//!
//! **This is a pure measurement pass. No production code (`compile_lexicon`,
//! `commerce_core::ir::query::compile`) is changed here** -- per the
//! "fix only defects exposed by the experiment" discipline, whether to
//! implement any of these relaxations is a decision for a follow-up
//! experiment (P9-E05), conditioned on what this one finds.

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::ir::compile as compile_query;

struct WandsQuery {
    text: String,
}

fn load_queries(path: &str) -> Vec<WandsQuery> {
    let content = std::fs::read_to_string(path).expect("read query.csv");
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let _query_id = parts.next()?;
            let text = parts.next()?.to_string();
            Some(WandsQuery { text })
        })
        .collect()
}

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

/// A crude, deliberately simple singular<->plural equivalence: matches if
/// either string equals the other with a single trailing "s" added or
/// removed. Not linguistically complete (irregular plurals are missed on
/// purpose) -- exactly the kind of "smallest mechanism that could explain
/// the gap" this diagnostic is testing, not a production stemmer.
fn plural_equiv(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if let Some(stripped) = a.strip_suffix('s') {
        if stripped == b {
            return true;
        }
    }
    if let Some(stripped) = b.strip_suffix('s') {
        if stripped == a {
            return true;
        }
    }
    false
}

fn ngrams(tokens: &[&str], max_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    for len in 1..=max_len.min(tokens.len()) {
        for start in 0..=(tokens.len() - len) {
            out.push(tokens[start..start + len].join(" "));
        }
    }
    out
}

fn main() {
    println!("=== P9-E03: lexicon-coverage diagnostic (Hypothesis 2: semantic/lexicon-compilation gap) ===");

    let raw_products =
        phase6a_eval::data::load_catalog(std::path::Path::new("dataset_cache/wands/catalog.jsonl"));
    println!("raw WANDS records: {}", raw_products.len());

    // Current-behavior baseline: exactly what compile_lexicon/compile()
    // already produce today, reusing the real production path (not
    // reimplemented) so "zero constraints today" is ground truth, not
    // assumed.
    let ingested = phase6a_eval::catalog::build_catalog(&raw_products);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );
    let lexicon = compile_lexicon(&profile, 1);

    // Vocabularies for the relaxation mechanisms, built directly from raw
    // WANDS records (not from the already-ingested/normalized
    // commerce_core types), so this diagnostic tests the *raw data's*
    // recoverability, independent of any ingestion choice already made.
    let mut product_class_whole: std::collections::BTreeSet<String> = Default::default();
    let mut product_class_pipe_split: std::collections::BTreeSet<String> = Default::default();
    let mut category_leaf_segments: std::collections::BTreeSet<String> = Default::default();
    let mut pipe_delimited_count = 0usize;
    for p in &raw_products {
        if let Some(pc) = &p.product_class {
            let norm = normalize(pc);
            product_class_whole.insert(norm.clone());
            if pc.contains('|') {
                pipe_delimited_count += 1;
                for part in pc.split('|') {
                    product_class_pipe_split.insert(normalize(part));
                }
            } else {
                product_class_pipe_split.insert(norm);
            }
        }
        if let Some(leaf) = &p.category_leaf {
            if let Some(last) = leaf.split('/').next_back() {
                category_leaf_segments.insert(normalize(last));
            }
        }
    }
    println!(
        "distinct product_class (raw, whole): {}, distinct product_class (pipe-split): {}, \
         distinct category_leaf leaf-segments: {}, products with pipe-delimited product_class: {} ({:.2}%)",
        product_class_whole.len(),
        product_class_pipe_split.len(),
        category_leaf_segments.len(),
        pipe_delimited_count,
        100.0 * pipe_delimited_count as f64 / raw_products.len() as f64
    );

    let queries = load_queries("dataset_cache/wands/query.csv");
    let mut zero_constraint_queries: Vec<String> = Vec::new();
    for q in &queries {
        let compiled = compile_query(&q.text, &lexicon);
        if compiled.constraints.is_empty() {
            zero_constraint_queries.push(q.text.clone());
        }
    }
    println!(
        "queries with zero constraints today (current production compile_lexicon behavior): {}/{}",
        zero_constraint_queries.len(),
        queries.len()
    );

    let mut h2a_pipe_split = 0usize;
    let mut h2b_leaf_segment = 0usize;
    let mut h2c_plural = 0usize;
    let mut h2d_substring = 0usize;
    let mut none_recoverable = 0usize;
    let mut examples_h2a: Vec<String> = Vec::new();
    let mut examples_h2b: Vec<String> = Vec::new();
    let mut examples_h2c: Vec<String> = Vec::new();
    let mut examples_none: Vec<String> = Vec::new();

    for text in &zero_constraint_queries {
        let lower = normalize(text);
        let tokens: Vec<&str> = lower.split_whitespace().collect();
        let candidates = ngrams(&tokens, 4);

        let hit_pipe_split = candidates
            .iter()
            .any(|c| product_class_pipe_split.contains(c) && !product_class_whole.contains(c));
        let hit_leaf_segment = candidates
            .iter()
            .any(|c| category_leaf_segments.contains(c));
        let hit_plural = candidates.iter().any(|c| {
            product_class_whole.iter().any(|pc| plural_equiv(c, pc))
                || category_leaf_segments
                    .iter()
                    .any(|seg| plural_equiv(c, seg))
        });
        let hit_substring = product_class_whole
            .iter()
            .any(|pc| pc.len() > 3 && (lower.contains(pc.as_str()) || pc.contains(&lower)))
            || category_leaf_segments
                .iter()
                .any(|seg| seg.len() > 3 && (lower.contains(seg.as_str()) || seg.contains(&lower)));

        if hit_pipe_split {
            h2a_pipe_split += 1;
            if examples_h2a.len() < 5 {
                examples_h2a.push(text.clone());
            }
        }
        if hit_leaf_segment {
            h2b_leaf_segment += 1;
            if examples_h2b.len() < 5 {
                examples_h2b.push(text.clone());
            }
        }
        if hit_plural {
            h2c_plural += 1;
            if examples_h2c.len() < 5 {
                examples_h2c.push(text.clone());
            }
        }
        if hit_substring {
            h2d_substring += 1;
        }
        if !hit_pipe_split && !hit_leaf_segment && !hit_plural && !hit_substring {
            none_recoverable += 1;
            if examples_none.len() < 8 {
                examples_none.push(text.clone());
            }
        }
    }

    let n = zero_constraint_queries.len();
    println!();
    println!("=== recoverability breakdown (of {n} zero-constraint queries; mechanisms are non-exclusive) ===");
    println!(
        "H2a pipe-split product_class:  {h2a_pipe_split}/{n} ({:.1}%)  examples: {examples_h2a:?}",
        pct(h2a_pipe_split, n)
    );
    println!(
        "H2b category leaf segment:     {h2b_leaf_segment}/{n} ({:.1}%)  examples: {examples_h2b:?}",
        pct(h2b_leaf_segment, n)
    );
    println!(
        "H2c plural/singular:           {h2c_plural}/{n} ({:.1}%)  examples: {examples_h2c:?}",
        pct(h2c_plural, n)
    );
    println!(
        "H2d substring near-miss:       {h2d_substring}/{n} ({:.1}%)  (catch-all signal, not itself a proposed fix)",
        pct(h2d_substring, n)
    );
    println!(
        "none of the above recoverable: {none_recoverable}/{n} ({:.1}%)  examples: {examples_none:?}",
        pct(none_recoverable, n)
    );

    println!();
    println!("=== per-hypothesis verdict (>=10% of zero-constraint queries = material, disclosed evidence) ===");
    for (name, count) in [
        ("H2a (pipe-split product_class)", h2a_pipe_split),
        ("H2b (category leaf segment)", h2b_leaf_segment),
        ("H2c (plural/singular)", h2c_plural),
    ] {
        if pct(count, n) >= 10.0 {
            println!(
                "{name}: CONFIRMED material ({:.1}% of zero-constraint queries recoverable)",
                pct(count, n)
            );
        } else {
            println!(
                "{name}: FALSIFIED as a material contributor ({:.1}%, below the 10% bar)",
                pct(count, n)
            );
        }
    }
}

fn pct(count: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        100.0 * count as f64 / total as f64
    }
}
