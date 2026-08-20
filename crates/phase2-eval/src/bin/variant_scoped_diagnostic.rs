//! P2-E15 diagnostic: after the P2-E14 compiler fix, the corrected P1-D
//! sweep still shows `variant_scoped_structural` at 68.8% zero-result for
//! commerce-native (22/32) *and* 53.1% for Solr (17/32) -- both structural/
//! exact-match systems -- while Tantivy (free-text, no structural
//! involvement) is 0%. That signature (both exact-match systems struggle,
//! lexical does not) is different from P2-E13/P2-E14's compiler bugs: it
//! suggests a genuine catalog-attribute-value mismatch (the query's
//! resolved attribute value literally isn't what the real judged-relevant
//! product's `color`/other attribute field says), not a routing/compiler
//! defect. This diagnostic prints, for each real `variant_scoped_structural`
//! query, the compiled attribute constraint(s) and -- for every judged
//! *relevant* product -- whether that specific product's own attributes
//! satisfy the compiled constraint, to tell "genuinely selective, zero
//! real products match" apart from "the judged-relevant product exists but
//! its attribute value differs from what the query compiled to."
//!
//! Usage: cargo run --release -p phase2-eval --bin variant_scoped_diagnostic
//!        [catalog.jsonl] [queries.jsonl]

use std::collections::BTreeMap;
use std::path::PathBuf;

use commerce_core::cold_start::{compile_lexicon, CatalogProfile};
use commerce_core::domain::AttributeValue;
use commerce_core::ir::compile;
use query_taxonomy::{classify9, QueryClass9};
use round1_eval::data::{self, EsciLabel};
use round1_eval::{catalog, query_taxonomy};

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

    println!("loading + ingesting real catalog...");
    let products = data::load_catalog(&catalog_path);
    let ingested = catalog::build_catalog(&products);
    println!("{} real products ingested", ingested.catalog.products.len());

    let profile = CatalogProfile::build(&ingested.catalog, &ingested.brands, &[], &[]);
    let lexicon = compile_lexicon(&profile, 25);

    println!("loading real queries + judgments...");
    let judgments = data::load_queries(&queries_path);
    let mut judged_by_query: BTreeMap<u64, (String, BTreeMap<String, EsciLabel>)> = BTreeMap::new();
    for j in &judgments {
        judged_by_query
            .entry(j.query_id)
            .or_insert_with(|| (j.query.clone(), BTreeMap::new()))
            .1
            .insert(j.product_id.clone(), j.label);
    }

    let mut shown = 0;
    let mut zero_result_shown = 0;
    for (&qid, (text, judged)) in &judged_by_query {
        if !judged.values().any(|l| l.is_relevant()) {
            continue;
        }
        let compiled = compile(text, &lexicon);
        let class = classify9(text, &compiled);
        if class != QueryClass9::VariantScoped {
            continue;
        }
        shown += 1;

        let any_hit = ingested
            .catalog
            .products
            .iter()
            .any(|p| p.variants.iter().any(|v| compiled.matches_variant(p, v)));
        if any_hit {
            continue; // not a zero-result case; skip for focus
        }
        zero_result_shown += 1;
        if zero_result_shown > 15 {
            continue;
        }

        println!("--- qid={qid} text={text:?} (ZERO commerce-native hits)");
        println!("    constraints: {:?}", compiled.constraints);

        // For every judged-relevant real ASIN, find its actual product and
        // report what its attributes really are for the constrained
        // attribute name(s), to see whether the mismatch is a value/casing
        // difference vs. the attribute being entirely absent.
        for (asin, label) in judged.iter().filter(|(_, l)| l.is_relevant()) {
            let Some(&pid) = ingested.asin_to_product_id.get(asin) else {
                println!(
                    "    relevant asin={asin} label={label:?}: NOT in ingested catalog at all"
                );
                continue;
            };
            let product = ingested
                .catalog
                .products
                .iter()
                .find(|p| p.id == pid)
                .expect("asin_to_product_id must point at a real product");
            for v in &product.variants {
                let attrs = commerce_core::domain::effective_attributes(product, v);
                let relevant_attrs: Vec<(String, String)> = attrs
                    .iter()
                    .filter(|(k, _)| {
                        compiled.constraints.iter().any(|c| match c {
                            commerce_core::ir::ResolvedConstraint::Attribute(
                                commerce_core::domain::Constraint::Enum { attribute, .. }
                                | commerce_core::domain::Constraint::MultiEnumContains {
                                    attribute,
                                    ..
                                }
                                | commerce_core::domain::Constraint::Boolean { attribute, .. }
                                | commerce_core::domain::Constraint::Numeric { attribute, .. }
                                | commerce_core::domain::Constraint::Text { attribute, .. },
                            ) => attribute == *k,
                            _ => false,
                        })
                    })
                    .map(|(k, v)| {
                        let s = match v {
                            AttributeValue::Enum(s) => s.clone(),
                            AttributeValue::MultiEnum(vs) => vs.join("|"),
                            AttributeValue::Text(s) => s.clone(),
                            AttributeValue::Boolean(b) => b.to_string(),
                            AttributeValue::Numeric(n) => n.to_string(),
                        };
                        (k.clone(), s)
                    })
                    .collect();
                println!(
                    "    relevant asin={asin} label={label:?} variant={:?} matches_query={} constrained_attrs={:?}",
                    v.id,
                    compiled.matches_variant(product, v),
                    relevant_attrs
                );
            }
        }
    }
    println!("\n{shown} real queries classified VariantScoped; {zero_result_shown} had zero commerce-native structural hits (showing up to 15)");
}
