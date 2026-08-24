//! Issue #34 Phase 9, P9-E01: does the bitmap-based `Hybrid` delegate
//! restriction (`phase9_eval::bitmap_delegate`) actually cost less per
//! query than the reference `TantivyDelegate`'s `TermSetQuery`-based
//! restriction, at a realistic large candidate-set size? PHASE2_DECISION.md
//! (P2-E17) reported the TermSetQuery arm's candidate set reaching "up to
//! ~60K terms for structural_plus_lexical_residual" on the real ESCI
//! catalog -- this experiment uses that as its candidate-set size.
//!
//! **Hypothesis**: for a fixed text query and a fixed restrict_to set of
//! 60,000 product ids (drawn from a synthetic 500,000-document index, so
//! the text query itself has real, non-trivial candidates to restrict),
//! per-query wall-clock cost (search-call construction + execution,
//! repeated) is materially lower for the bitmap-restrict arm than for the
//! TermSetQuery arm, without changing which documents are returned.
//!
//! **Decision criteria, stated before implementation**: (a) both arms
//! return the exact same top-10 document set (by product ordinal),
//! confirming the comparison is fair (same correctness, different
//! mechanism only) -- see the run's own printed "score offset" note below
//! for a real, disclosed scoring artifact found in arm (a) that this
//! criterion deliberately does not require matching, and why that does not
//! undermine the comparison; (b) bitmap-restrict's mean per-query latency
//! is at most half of TermSetQuery's (a 2x-or-better speedup) to count as
//! a material, not incidental, win, per CLAUDE.md's "material, not
//! incremental" standard for latency claims and this project's other
//! similar 2x-5x-class bars elsewhere; anything smaller is reported as
//! FALSIFIED, not massaged into a qualified win.
//!
//! **Evidence class: synthetic, deliberately scoped.** This isolates the
//! delegate-restriction *mechanism's* cost in isolation from any real
//! catalog's specific vocabulary/relevance distribution -- a fair,
//! like-for-like microbenchmark of exactly the two code paths
//! PHASE2_DECISION.md named, not a relevance or economics claim. The real
//! WANDS-catalog physical-advantage-by-query-class re-run (P9-E02) is
//! where this mechanism gets exercised against real queries end-to-end.

use bench_harness::{measured_repeat, Distribution};
use commerce_core::plan::LexicalDelegate;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, TermSetQuery};
use tantivy::schema::{Field, Schema, FAST, STORED, STRING, TEXT};
use tantivy::{doc, Index, Term};

const NUM_DOCS: usize = 500_000;
const RESTRICT_TO_SIZE: usize = 60_000;
const VOCAB: &[&str] = &[
    "solid", "wood", "platform", "bed", "slow", "cooker", "dining", "table", "office", "chair",
    "storage", "cabinet", "outdoor", "patio", "rug", "lamp", "mirror", "shelf", "sofa", "desk",
];

fn synthetic_title(ordinal: u64) -> String {
    let a = VOCAB[(ordinal as usize) % VOCAB.len()];
    let b = VOCAB[(ordinal as usize / 7) % VOCAB.len()];
    let c = VOCAB[(ordinal as usize / 13) % VOCAB.len()];
    format!("{a} {b} {c} model {ordinal}")
}

struct TermSetSchema {
    index: Index,
    id_field: Field,
    title_field: Field,
}

fn build_termset_index() -> tantivy::Result<TermSetSchema> {
    let mut builder = Schema::builder();
    let id_field = builder.add_text_field("id", STRING | STORED);
    let ordinal_field = builder.add_u64_field("product_ordinal", FAST | STORED);
    let title_field = builder.add_text_field("title", TEXT);
    let schema = builder.build();
    let index = Index::create_in_ram(schema);
    let mut writer = index.writer(128_000_000)?;
    for ordinal in 0..NUM_DOCS as u64 {
        writer.add_document(doc!(
            id_field => ordinal.to_string(),
            ordinal_field => ordinal,
            title_field => synthetic_title(ordinal),
        ))?;
    }
    writer.commit()?;
    Ok(TermSetSchema {
        index,
        id_field,
        title_field,
    })
}

fn main() -> tantivy::Result<()> {
    println!("=== P9-E01: bitmap-restrict vs TermSetQuery delegate restriction ===");
    println!("synthetic index: {NUM_DOCS} docs, restrict_to size: {RESTRICT_TO_SIZE}");

    // Same restrict_to set (every 8th ordinal, giving exactly 62,500
    // candidates, close to P2-E17's reported ~60K worst case) for both
    // arms, and the same free-text query ("wood table"), fixed up front.
    let restrict_to: Vec<u64> = (0..NUM_DOCS as u64).step_by(8).collect();
    assert!(
        restrict_to.len() >= RESTRICT_TO_SIZE,
        "need at least {RESTRICT_TO_SIZE} candidates, got {}",
        restrict_to.len()
    );
    let restrict_to = &restrict_to[..RESTRICT_TO_SIZE];
    let query_text = "wood table";
    let limit = 10;

    // --- Arm (a): TermSetQuery, reproducing Phase 2's own reference
    // TantivyDelegate pattern (crates/phase2-eval/src/bin/p1d_physical_advantage_eval.rs):
    // a fresh BooleanQuery(Must: text_query, Must: TermSetQuery) built from
    // scratch on every call.
    let termset_schema = build_termset_index()?;
    let termset_reader = termset_schema.index.reader()?;
    let termset_query_parser =
        QueryParser::for_index(&termset_schema.index, vec![termset_schema.title_field]);

    let termset_hits_ref = std::cell::RefCell::new(Vec::new());
    let termset_samples = measured_repeat(3, 30, || {
        let (text_query, _errors) = termset_query_parser.parse_query_lenient(query_text);
        let filter_terms: Vec<Term> = restrict_to
            .iter()
            .map(|ord| Term::from_field_text(termset_schema.id_field, &ord.to_string()))
            .collect();
        let filter_query = TermSetQuery::new(filter_terms);
        let combined: Box<dyn Query> = Box::new(BooleanQuery::new(vec![
            (Occur::Must, text_query),
            (Occur::Must, Box::new(filter_query)),
        ]));
        let searcher = termset_reader.searcher();
        let top_docs = searcher
            .search(&combined, &TopDocs::with_limit(limit))
            .unwrap_or_default();
        let mut hits: Vec<(u64, f32)> = top_docs
            .into_iter()
            .filter_map(|(score, addr)| {
                let seg = searcher.segment_reader(addr.segment_ord);
                let col = seg.fast_fields().u64("product_ordinal").ok()?;
                Some((col.first(addr.doc_id)?, score))
            })
            .collect();
        hits.sort_by_key(|a| a.0);
        *termset_hits_ref.borrow_mut() = hits;
    });

    // --- Arm (b): bitmap-restrict, this experiment's replacement mechanism.
    // Deliberately reuses the *exact same* index arm (a) just built --
    // same documents, same title text, same "product_ordinal" fast field
    // name (`build_termset_index` and `bitmap_delegate`'s own schema both
    // use it) -- so the restriction *mechanism* is the only variable
    // between the two arms, not a difference in schema, field list, or
    // BM25 statistics from two separately-built indices.
    let bitmap_delegate = phase9_eval::bitmap_delegate::BitmapTantivyDelegate::new(
        &termset_schema.index,
        vec![termset_schema.title_field],
    )?;
    let restrict_to_set: std::collections::BTreeSet<commerce_core::domain::ProductId> = restrict_to
        .iter()
        .map(|&o| commerce_core::domain::ProductId(o))
        .collect();

    let bitmap_hits_ref = std::cell::RefCell::new(Vec::new());
    let bitmap_samples = measured_repeat(3, 30, || {
        let hits = bitmap_delegate.search(
            &["wood".to_string(), "table".to_string()],
            Some(&restrict_to_set),
            limit,
        );
        let mut sorted: Vec<(u64, f32)> = hits
            .into_iter()
            .map(|h| (h.product.0, h.score as f32))
            .collect();
        sorted.sort_by_key(|a| a.0);
        *bitmap_hits_ref.borrow_mut() = sorted;
    });

    let termset_dist = Distribution::compute(&termset_samples);
    let bitmap_dist = Distribution::compute(&bitmap_samples);
    termset_dist.print("TermSetQuery restrict (arm a)", "ms");
    bitmap_dist.print("bitmap restrict (arm b)", "ms");

    let ratio = termset_dist.mean / bitmap_dist.mean;
    println!("mean ratio (termset / bitmap): {ratio:.2}x");

    // Same document SET (both sorted by ordinal), not raw score equality:
    // a real, disclosed finding surfaced by this experiment is that
    // TermSetQuery's own Occur::Must clause adds a *constant* +1.0 to
    // every surviving hit's combined BooleanQuery score (Tantivy's default
    // boolean combiner sums all Must clauses' sub-scores, and
    // TermSetQuery's own AutomatonWeight scores every match as a flat
    // 1.0) -- a side effect of the reference TermSetQuery restriction
    // design, unrelated to text relevance, that this bitmap-restrict
    // replacement does not reproduce (its score is exactly the inner text
    // query's own score, proven by the `score_is_exactly_the_inner_text_querys_score_not_rewritten`
    // unit test). A constant per-hit offset does not change which 10
    // documents make the top-10 or their relative order, so it does not
    // undermine this experiment's own correctness comparison -- but it is
    // a second, real defect in the reference TermSetQuery delegate this
    // experiment did not set out to find, recorded here rather than
    // silently observed and dropped.
    let termset_ordinals: Vec<u64> = termset_hits_ref.borrow().iter().map(|(o, _)| *o).collect();
    let bitmap_ordinals: Vec<u64> = bitmap_hits_ref.borrow().iter().map(|(o, _)| *o).collect();
    let same_hits = termset_ordinals == bitmap_ordinals;
    println!("both arms returned the same document set: {same_hits}");
    if let (Some((_, ts)), Some((_, bs))) = (
        termset_hits_ref.borrow().first(),
        bitmap_hits_ref.borrow().first(),
    ) {
        println!(
            "score offset observed (termset - bitmap, same doc): {:.4} \
             -- a disclosed TermSetQuery/BooleanQuery-combiner artifact, see comment above",
            ts - bs
        );
    }
    if !same_hits {
        println!("termset ordinals: {termset_ordinals:?}\nbitmap ordinals: {bitmap_ordinals:?}");
    }

    if !same_hits {
        println!("=== VERDICT: INCONCLUSIVE -- arms disagree on hits, not a fair comparison ===");
    } else if ratio >= 2.0 {
        println!(
            "=== VERDICT: CONFIRMED -- bitmap-restrict is {ratio:.2}x faster than TermSetQuery (>=2x bar met) ==="
        );
    } else {
        println!(
            "=== VERDICT: FALSIFIED -- bitmap-restrict is only {ratio:.2}x faster than TermSetQuery (<2x bar) ==="
        );
    }

    Ok(())
}
