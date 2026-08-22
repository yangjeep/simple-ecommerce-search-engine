//! Issue #34 Phase 9, P9-E04: isolates Hypotheses 1 and 3 behind P9-E02's
//! REVISE, using one shared harness -- for `structural_routed`
//! (FastPath+Hybrid) queries, both engines rank the *identical* structural
//! candidate set (`CatalogIndex::indexed_candidates`, the same pool both
//! `plan::plan`'s `FastPath` and `Hybrid` outcomes derive from), so any
//! NDCG difference is a pure ranking-quality signal (retrieval/coverage
//! held constant), and any latency difference is a pure execution-speed
//! signal within the same semantic scope -- neither conflated with
//! P9-E02's end-to-end comparison, where native and Solr could legally
//! return different candidate sets entirely.
//!
//! **H1 (ranking quality)**: given the identical candidate set, Solr's
//! BM25 (restricted via a `{!terms f=id}` filter to exactly that set)
//! achieves materially higher NDCG@10 than P9-E00's native default
//! ranking signal.
//! **Decision criterion**: >=10% relative NDCG@10 gap (native worse) on
//! the identical-candidate-set comparison counts as CONFIRMED (a real
//! ranking-quality problem, independent of retrieval); under that,
//! FALSIFIED -- meaning P9-E02's end-to-end gap must come predominantly
//! from a *retrieval/coverage* difference between the two engines' real
//! candidate sets, not from ranking the same pool worse.
//!
//! **H3 (execution-speed advantage, relevance-controlled)**: native's
//! `execute_ranked` call (structural retrieval + P9-E00 ranking) over the
//! candidate set is still materially faster (>=2x, this project's
//! standing bar) than Solr's identical-scope, identically-restricted
//! query, i.e. the latency advantage P9-E02 measured end-to-end is not
//! merely an artifact of native returning a cheaper-to-produce but
//! lower-quality result.
//!
//! **Scope, disclosed up front**: candidate sets above `MAX_CANDIDATES`
//! are skipped (counted, not silently dropped) -- a `{!terms f=id}` Solr
//! filter of unbounded size is not a realistic same-request comparison,
//! and this experiment isolates ranking/execution over a *shared*
//! candidate set, not a claim about arbitrarily large ones.

use std::collections::HashMap;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::ProductId;
use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::plan::{plan, ExecutionOutcome, PlannerPolicy};
use phase9_eval::wands_relevance::ndcg_recall_mrr;

const K: usize = 10;
const MAX_CANDIDATES: usize = 5000;

struct WandsQuery {
    query_id: String,
    text: String,
}

fn load_queries(path: &str) -> Vec<WandsQuery> {
    let content = std::fs::read_to_string(path).expect("read query.csv");
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let query_id = parts.next()?.to_string();
            let text = parts.next()?.to_string();
            Some(WandsQuery { query_id, text })
        })
        .collect()
}

fn load_labels(
    path: &str,
) -> std::collections::BTreeMap<
    String,
    std::collections::BTreeMap<String, phase9_eval::wands_relevance::WandsLabel>,
> {
    let content = std::fs::read_to_string(path).expect("read label.csv");
    let mut judged: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, phase9_eval::wands_relevance::WandsLabel>,
    > = Default::default();
    for line in content.lines().skip(1) {
        let mut parts = line.splitn(4, '\t');
        let Some(_id) = parts.next() else { continue };
        let Some(query_id) = parts.next() else {
            continue;
        };
        let Some(product_id) = parts.next() else {
            continue;
        };
        let Some(raw_label) = parts.next() else {
            continue;
        };
        let Some(label) = phase9_eval::wands_relevance::WandsLabel::parse(raw_label) else {
            continue;
        };
        judged
            .entry(query_id.to_string())
            .or_default()
            .insert(product_id.to_string(), label);
    }
    judged
}

/// POSTs to Solr's `/select` handler (form-encoded, not GET) specifically
/// so a `{!terms f=id}` filter listing thousands of ids never hits a URL
/// length limit -- Solr's own standard handler accepts POSTed form params
/// identically to GET query params.
fn solr_search_restricted(
    base_url: &str,
    q: &str,
    allowed_ids: &[String],
    rows: usize,
) -> Option<(f64, Vec<String>)> {
    let terms_fq = format!("{{!terms f=id}}{}", allowed_ids.join(","));
    let url = format!("{base_url}/select");
    let t0 = std::time::Instant::now();
    let resp = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send_form(&[
            ("q", q),
            ("fq", terms_fq.as_str()),
            ("rows", &rows.to_string()),
            ("fl", "id"),
        ])
        .ok()?;
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let body: serde_json::Value = resp.into_json().ok()?;
    let ids: Vec<String> = body["response"]["docs"]
        .as_array()?
        .iter()
        .filter_map(|d| d["id"].as_str().map(str::to_string))
        .collect();
    Some((elapsed_ms, ids))
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn main() {
    println!("=== P9-E04: isolated ranking-quality (H1) + execution-speed (H3) comparison, identical candidate set ===");

    let solr_base_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://localhost:8983/solr/wands_bench".to_string());

    let raw_products =
        phase6a_eval::data::load_catalog(std::path::Path::new("dataset_cache/wands/catalog.jsonl"));
    let ingested = phase6a_eval::catalog::build_catalog(&raw_products);
    let index = CatalogIndex::build(&ingested.catalog);
    let profile = CatalogProfile::build(
        &ingested.catalog,
        &[],
        &ingested.product_types,
        &ingested.categories,
    );
    let lexicon = compile_lexicon(&profile, 1);
    let mut wands_id_by_product_id: HashMap<ProductId, String> = HashMap::new();
    for (wands_id, pid) in &ingested.wands_id_to_product_id {
        wands_id_by_product_id.insert(*pid, wands_id.clone());
    }
    let raw_product_by_wands_id: HashMap<&str, &phase6a_eval::data::WandsProduct> =
        raw_products.iter().map(|p| (p.id.as_str(), p)).collect();
    let product_type_name_by_id: HashMap<commerce_core::domain::ProductTypeId, String> = ingested
        .product_types
        .iter()
        .map(|pt| (pt.id, pt.name.clone()))
        .collect();
    let category_name_by_id: HashMap<commerce_core::domain::CategoryId, String> = ingested
        .categories
        .iter()
        .map(|c| (c.id, c.name.clone()))
        .collect();

    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };

    let queries = load_queries("dataset_cache/wands/query.csv");
    let judged = load_labels("dataset_cache/wands/label.csv");

    // Warmup pass (this project's own P2-E16/P9-E02 precedent: an
    // unwarmed Solr latency measurement was previously found broken by
    // exactly this omission, and this binary's own first run showed the
    // H3 latency ratio swing from 2.25x to 0.98x between successive runs
    // before this warmup was added -- caught, not assumed away). Runs
    // every structural_routed query against both engines once via the
    // same restricted-query path the measured pass uses, discarding
    // results.
    println!("warmup pass (both engines, restricted-query path, discarded)...");
    for q in &queries {
        let compiled = compile(&q.text, &lexicon);
        let outcome = plan(&compiled, &index, ingested.catalog.products.len(), &policy).outcome;
        if !matches!(
            outcome,
            ExecutionOutcome::FastPath | ExecutionOutcome::Hybrid
        ) {
            continue;
        }
        let candidate_ords = index.indexed_candidates(&compiled.constraints);
        if candidate_ords.len() > MAX_CANDIDATES as u64 {
            continue;
        }
        let candidate_product_ids = index.candidate_product_ids(&candidate_ords);
        let allowed_wands_ids: Vec<String> = candidate_product_ids
            .iter()
            .filter_map(|pid| wands_id_by_product_id.get(pid).cloned())
            .collect();
        if allowed_wands_ids.is_empty() {
            continue;
        }
        let _ = index.execute_ranked(&compiled, &ingested.catalog, candidate_ords.len() as usize);
        let text = if compiled.residual_lexical.is_empty() {
            q.text.clone()
        } else {
            compiled.residual_lexical.join(" ")
        };
        let solr_q = if text.trim().is_empty() {
            "*:*".to_string()
        } else {
            format!("{{!edismax qf=\"title description\"}}{text}")
        };
        let _ = solr_search_restricted(&solr_base_url, &solr_q, &allowed_wands_ids, K);
    }

    let mut native_ndcg = Vec::new();
    let mut solr_ndcg = Vec::new();
    let mut native_ms = Vec::new();
    let mut solr_ms = Vec::new();
    let mut candidate_sizes = Vec::new();
    let mut candidate_set_relevant_recall = Vec::new();
    let mut candidate_set_exact_recall = Vec::new();
    let mut candidate_set_partial_recall = Vec::new();
    let mut low_exact_recall_examples: Vec<String> = Vec::new();
    let mut exact_recall_with_entity_constraint: Vec<f64> = Vec::new();
    let mut exact_recall_attribute_only: Vec<f64> = Vec::new();
    let mut skipped_too_large = 0usize;
    let mut evaluated = 0usize;

    for q in &queries {
        let Some(query_judged) = judged.get(&q.query_id) else {
            continue;
        };
        let compiled = compile(&q.text, &lexicon);
        let outcome = plan(&compiled, &index, ingested.catalog.products.len(), &policy).outcome;
        if !matches!(
            outcome,
            ExecutionOutcome::FastPath | ExecutionOutcome::Hybrid
        ) {
            continue; // Punt-routed: no structural candidate set to isolate ranking over.
        }

        let candidate_ords = index.indexed_candidates(&compiled.constraints);
        candidate_sizes.push(candidate_ords.len());
        if candidate_ords.len() > MAX_CANDIDATES as u64 {
            skipped_too_large += 1;
            continue;
        }
        let candidate_product_ids = index.candidate_product_ids(&candidate_ords);
        let allowed_wands_ids: Vec<String> = candidate_product_ids
            .iter()
            .filter_map(|pid| wands_id_by_product_id.get(pid).cloned())
            .collect();
        if allowed_wands_ids.is_empty() {
            continue;
        }

        // Follow-on diagnostic, prompted directly by H1's falsification
        // below (if ranking the same pool is not the problem, the
        // problem must be which documents are even IN the pool): what
        // fraction of this query's real judged-relevant documents are
        // present anywhere in native's structural candidate set at all,
        // before any ranking happens? Split by grade (Exact vs Partial):
        // WANDS's own labeling methodology grades "Partial" by broader
        // semantic/attribute similarity, not strict category membership
        // -- if the misses concentrate in Partial (not Exact), that is
        // evidence the gap is partly an inherent scope mismatch between a
        // single hard structural constraint and a graded relevance
        // definition that spans multiple categories, not purely a native
        // matching defect.
        let allowed_set: std::collections::HashSet<&str> =
            allowed_wands_ids.iter().map(String::as_str).collect();
        let relevant_total = query_judged.values().filter(|l| l.is_relevant()).count();
        if relevant_total > 0 {
            let relevant_in_candidates = query_judged
                .iter()
                .filter(|(_, label)| label.is_relevant())
                .filter(|(pid, _)| allowed_set.contains(pid.as_str()))
                .count();
            candidate_set_relevant_recall
                .push(relevant_in_candidates as f64 / relevant_total as f64);
        }
        let exact_total = query_judged
            .values()
            .filter(|&&l| l == phase9_eval::wands_relevance::WandsLabel::Exact)
            .count();
        if exact_total > 0 {
            let exact_in_candidates = query_judged
                .iter()
                .filter(|(_, &label)| label == phase9_eval::wands_relevance::WandsLabel::Exact)
                .filter(|(pid, _)| allowed_set.contains(pid.as_str()))
                .count();
            let exact_recall = exact_in_candidates as f64 / exact_total as f64;
            candidate_set_exact_recall.push(exact_recall);

            // Aggregate (not anecdotal) test of the pattern the
            // qualitative examples below suggest: does having a real
            // entity constraint (ProductType/Category) rather than only
            // an attribute-level one (e.g. a color Enum that happens to
            // coincide with a head-noun word, like "coffee" in "coffee
            // table" matching color="Coffee") predict materially higher
            // Exact recall?
            let has_entity_constraint = compiled.constraints.iter().any(|c| {
                matches!(
                    c,
                    commerce_core::ir::ResolvedConstraint::Structural(
                        commerce_core::ir::StructuralConstraint::ProductType(_)
                            | commerce_core::ir::StructuralConstraint::Category(_)
                    )
                )
            });
            if has_entity_constraint {
                exact_recall_with_entity_constraint.push(exact_recall);
            } else {
                exact_recall_attribute_only.push(exact_recall);
            }

            // Qualitative diagnostic for a handful of low-Exact-recall
            // queries: what did native's compiled constraints actually
            // resolve to, and what is the real product_class/category of
            // a sample of the Exact-labeled products that were missed?
            if exact_recall < 0.5 && low_exact_recall_examples.len() < 6 {
                let constraint_desc: Vec<String> = compiled
                    .constraints
                    .iter()
                    .map(|c| match c {
                        commerce_core::ir::ResolvedConstraint::Structural(
                            commerce_core::ir::StructuralConstraint::ProductType(id),
                        ) => format!(
                            "ProductType({:?})",
                            product_type_name_by_id
                                .get(id)
                                .map(String::as_str)
                                .unwrap_or("?")
                        ),
                        commerce_core::ir::ResolvedConstraint::Structural(
                            commerce_core::ir::StructuralConstraint::Category(id),
                        ) => format!(
                            "Category({:?})",
                            category_name_by_id
                                .get(id)
                                .map(String::as_str)
                                .unwrap_or("?")
                        ),
                        other => format!("{other:?}"),
                    })
                    .collect();
                let missed_examples: Vec<String> = query_judged
                    .iter()
                    .filter(|(_, &label)| label == phase9_eval::wands_relevance::WandsLabel::Exact)
                    .filter(|(pid, _)| !allowed_set.contains(pid.as_str()))
                    .take(3)
                    .map(|(pid, _)| {
                        let p = raw_product_by_wands_id.get(pid.as_str());
                        format!(
                            "{pid}: product_class={:?}, category_leaf={:?}",
                            p.and_then(|p| p.product_class.as_deref()),
                            p.and_then(|p| p.category_leaf.as_deref())
                        )
                    })
                    .collect();
                low_exact_recall_examples.push(format!(
                    "query={:?} exact_recall={exact_recall:.2} resolved_constraints={constraint_desc:?} \
                     missed_exact_examples={missed_examples:?}",
                    q.text
                ));
            }
        }
        let partial_total = query_judged
            .values()
            .filter(|&&l| l == phase9_eval::wands_relevance::WandsLabel::Partial)
            .count();
        if partial_total > 0 {
            let partial_in_candidates = query_judged
                .iter()
                .filter(|(_, &label)| label == phase9_eval::wands_relevance::WandsLabel::Partial)
                .filter(|(pid, _)| allowed_set.contains(pid.as_str()))
                .count();
            candidate_set_partial_recall.push(partial_in_candidates as f64 / partial_total as f64);
        }

        // Arm (a): native's own execute_ranked, over the FULL candidate
        // set (k = candidate count), so it can freely reorder every
        // member before truncating to K -- a fair shot for the ranking
        // signal, not artificially limited.
        let t0 = std::time::Instant::now();
        let native_ranked =
            index.execute_ranked(&compiled, &ingested.catalog, candidate_ords.len() as usize);
        let native_latency_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let native_ids: Vec<String> = native_ranked
            .iter()
            .take(K)
            .filter_map(|h| wands_id_by_product_id.get(&h.product).cloned())
            .collect();
        let (n_ndcg, _, _) = ndcg_recall_mrr(&native_ids, query_judged, K);

        // Arm (b): Solr BM25, restricted via {!terms f=id} to exactly the
        // same candidate set -- same pool, different ranking mechanism.
        let text = if compiled.residual_lexical.is_empty() {
            q.text.clone()
        } else {
            compiled.residual_lexical.join(" ")
        };
        let solr_q = if text.trim().is_empty() {
            "*:*".to_string()
        } else {
            format!("{{!edismax qf=\"title description\"}}{text}")
        };
        let Some((solr_latency_ms, solr_ids)) =
            solr_search_restricted(&solr_base_url, &solr_q, &allowed_wands_ids, K)
        else {
            continue;
        };
        let (s_ndcg, _, _) = ndcg_recall_mrr(&solr_ids, query_judged, K);

        native_ndcg.push(n_ndcg);
        solr_ndcg.push(s_ndcg);
        native_ms.push(native_latency_ms);
        solr_ms.push(solr_latency_ms);
        evaluated += 1;
    }

    candidate_sizes.sort_unstable();
    let median_candidates = candidate_sizes
        .get(candidate_sizes.len() / 2)
        .copied()
        .unwrap_or(0);
    let max_candidates = candidate_sizes.last().copied().unwrap_or(0);

    println!(
        "structural_routed queries seen: {}, evaluated (candidate set <= {MAX_CANDIDATES}): {evaluated}, \
         skipped (candidate set too large): {skipped_too_large}",
        candidate_sizes.len()
    );
    println!("candidate-set size: median={median_candidates}, max={max_candidates}");
    println!();

    let native_ndcg_mean = mean(&native_ndcg);
    let solr_ndcg_mean = mean(&solr_ndcg);
    let native_ms_mean = mean(&native_ms);
    let solr_ms_mean = mean(&solr_ms);
    println!("=== H1: ranking quality, identical candidate set (n={evaluated}) ===");
    println!("native NDCG@10={native_ndcg_mean:.4}  solr-restricted NDCG@10={solr_ndcg_mean:.4}");
    let ndcg_gap = if solr_ndcg_mean > 0.0 {
        (native_ndcg_mean - solr_ndcg_mean) / solr_ndcg_mean * 100.0
    } else {
        0.0
    };
    println!("relative gap (native vs solr, same candidates): {ndcg_gap:+.2}%");
    if ndcg_gap <= -10.0 {
        println!("=== H1 CONFIRMED: native's ranking signal is materially worse than Solr's BM25 on the identical candidate set ===");
    } else {
        println!("=== H1 FALSIFIED: native's ranking signal is NOT materially worse on the identical candidate set -- the end-to-end gap must come predominantly from retrieval/coverage differences, not ranking ===");
    }

    println!();
    println!("=== H1 follow-on diagnostic: candidate-set relevant-document recall (prompted by H1's own result) ===");
    println!(
        "mean fraction of a query's real judged-relevant documents present ANYWHERE in native's \
         structural candidate set, before any ranking (n={}): {:.4}",
        candidate_set_relevant_recall.len(),
        mean(&candidate_set_relevant_recall)
    );
    let full_recall_count = candidate_set_relevant_recall
        .iter()
        .filter(|&&r| r >= 0.999)
        .count();
    println!(
        "queries where the candidate set contains 100% of judged-relevant documents: {full_recall_count}/{}",
        candidate_set_relevant_recall.len()
    );
    println!(
        "split by grade -- Exact recall (n={}): {:.4}   Partial recall (n={}): {:.4}",
        candidate_set_exact_recall.len(),
        mean(&candidate_set_exact_recall),
        candidate_set_partial_recall.len(),
        mean(&candidate_set_partial_recall)
    );
    println!();
    println!("=== aggregate test (not anecdotal): does a real entity constraint (ProductType/Category) predict higher Exact recall than an attribute-only (e.g. incidental color) constraint? ===");
    println!(
        "queries with a ProductType/Category constraint (n={}): mean Exact recall = {:.4}",
        exact_recall_with_entity_constraint.len(),
        mean(&exact_recall_with_entity_constraint)
    );
    println!(
        "queries with only attribute-level constraints, no entity (n={}): mean Exact recall = {:.4}",
        exact_recall_attribute_only.len(),
        mean(&exact_recall_attribute_only)
    );

    println!();
    println!("=== qualitative examples: low-Exact-recall queries, resolved constraint, missed Exact products' real category ===");
    for example in &low_exact_recall_examples {
        println!("{example}");
    }
    println!();
    if mean(&candidate_set_exact_recall) >= 0.7 && mean(&candidate_set_partial_recall) < 0.3 {
        println!(
            "=== recall gap concentrates in Partial, not Exact: consistent with WANDS's graded \
             relevance spanning categories no single hard structural constraint can capture, \
             not purely a native matching defect ==="
        );
    } else if mean(&candidate_set_exact_recall) < 0.5 {
        println!(
            "=== Exact recall itself is low: native's structural constraint is missing even \
             same-category ground truth -- a real native matching defect, not just a scope-mismatch \
             artifact ==="
        );
    }

    println!();
    println!(
        "=== H3: execution speed, identical candidate set and semantic scope (n={evaluated}) ==="
    );
    println!("native mean latency={native_ms_mean:.4}ms  solr-restricted mean latency={solr_ms_mean:.4}ms");
    let ratio = if native_ms_mean > 0.0 {
        solr_ms_mean / native_ms_mean
    } else {
        0.0
    };
    println!("latency ratio (solr / native): {ratio:.2}x");
    if ratio >= 2.0 {
        println!("=== H3 CONFIRMED: native is still materially faster (>=2x) even with candidate set/semantic scope held identical -- the speed advantage is not an artifact of a relevance shortfall ===");
    } else {
        println!("=== H3 FALSIFIED: native's speed advantage does not clear the >=2x bar once candidate set is held identical ===");
    }
}
