use commerce_core::index::CatalogIndex;
use commerce_core::ir::{compile, CommerceQuery};
use commerce_core::plan::{plan, ExecutionOutcome, PlannerPolicy};
use comparator_eval::translate::{
    translate_all_with_config, SolrFieldMap, SolrTranslationConfig, StructuralNames,
};
use issue61_eval::{
    load_dataset, sha256_hex as digest_hex, write_workload, AdmissionClass, Dataset, FrozenQuery,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
struct SourceQuery {
    query_id: String,
    text: String,
}

fn parse_wands_queries(content: &str) -> Result<Vec<SourceQuery>, String> {
    content
        .lines()
        .enumerate()
        .skip(1)
        .map(|(index, line)| {
            let (query_id, rest) = line
                .split_once('\t')
                .ok_or_else(|| format!("malformed WANDS query at line {}", index + 1))?;
            let text = rest
                .split('\t')
                .next()
                .ok_or_else(|| format!("missing query text at line {}", index + 1))?;
            if query_id.is_empty() || text.is_empty() {
                return Err(format!("empty WANDS query field at line {}", index + 1));
            }
            Ok(SourceQuery {
                query_id: query_id.to_string(),
                text: text.to_string(),
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct EsciQuery {
    query: String,
}

fn parse_esci_queries(content: &str) -> Result<Vec<SourceQuery>, String> {
    content
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let parsed: EsciQuery = serde_json::from_str(line)
                .map_err(|error| format!("malformed ESCI query at line {}: {error}", index + 1))?;
            Ok(SourceQuery {
                query_id: (index + 1).to_string(),
                text: parsed.query,
            })
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(bytes)
}

fn required_arg(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("missing {name}"))
}

fn solr_config(dataset: Dataset) -> SolrTranslationConfig {
    let fields = match dataset {
        Dataset::Wands => SolrFieldMap {
            brand: None,
            product_type: Some("product_class"),
            category: Some("category_leaf"),
            price_cents: None,
        },
        Dataset::EsciElectronics => SolrFieldMap {
            brand: Some("brand"),
            product_type: None,
            category: None,
            price_cents: None,
        },
    };
    SolrTranslationConfig {
        fields,
        lowercase_companion_suffix: Some("_lc"),
    }
}

fn query_fields(dataset: Dataset) -> &'static str {
    match dataset {
        Dataset::Wands => "title description",
        Dataset::EsciElectronics => "title description bullet_point",
    }
}

#[derive(Debug)]
struct EngineRequests {
    native_q: String,
    solr_q: String,
    fq: Vec<String>,
    params: BTreeMap<String, String>,
}

fn engine_requests(
    query_id: &str,
    text: &str,
    compiled: &CommerceQuery,
    dataset: Dataset,
    names: &dyn StructuralNames,
) -> Result<EngineRequests, String> {
    let (fq, unresolvable) =
        translate_all_with_config(&compiled.constraints, names, &solr_config(dataset));
    if !unresolvable.is_empty() {
        return Err(format!(
            "query {query_id}: unresolvable constraints: {}",
            unresolvable.join("; ")
        ));
    }
    let params = [
        ("defType".to_string(), "edismax".to_string()),
        ("fl".to_string(), "id".to_string()),
        ("qf".to_string(), query_fields(dataset).to_string()),
        ("rows".to_string(), "10".to_string()),
    ]
    .into_iter()
    .collect();
    let q = if compiled.residual_lexical.is_empty() {
        String::from("*:*")
    } else {
        compiled.residual_lexical.join(" ")
    };
    Ok(EngineRequests {
        native_q: text.to_string(),
        solr_q: q,
        fq,
        params,
    })
}

fn run() -> Result<bool, Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    let catalog_path = PathBuf::from(required_arg(&args, "--catalog")?);
    let query_path = PathBuf::from(required_arg(&args, "--queries")?);
    let dataset = Dataset::parse(&required_arg(&args, "--dataset")?)?;
    let output_path = PathBuf::from(required_arg(&args, "--out")?);
    let content = std::fs::read_to_string(&query_path)?;
    let source_queries = match dataset {
        Dataset::Wands => parse_wands_queries(&content)?,
        Dataset::EsciElectronics => parse_esci_queries(&content)?,
    };
    let loaded = load_dataset(&catalog_path, dataset)?;
    let index = CatalogIndex::build(&loaded.catalog);
    let policy = PlannerPolicy {
        selectivity_threshold: 0.05,
        delegate_oversample: 20,
    };
    let mut histogram = BTreeMap::new();
    let mut frozen = Vec::with_capacity(source_queries.len());
    let mut translation_failures = Vec::new();
    for source in source_queries {
        let compiled = compile(&source.text, &loaded.lexicon);
        let outcome = plan(&compiled, &index, loaded.catalog.products.len(), &policy).outcome;
        let admission_class = match outcome {
            ExecutionOutcome::FastPath => AdmissionClass::FastPath,
            ExecutionOutcome::Hybrid => AdmissionClass::Hybrid,
            ExecutionOutcome::Punt => AdmissionClass::Punt,
        };
        *histogram.entry(admission_class).or_insert(0usize) += 1;
        let requests =
            match engine_requests(&source.query_id, &source.text, &compiled, dataset, &loaded) {
                Ok(requests) => requests,
                Err(error) => {
                    translation_failures.push(error);
                    continue;
                }
            };
        frozen.push(
            FrozenQuery {
                query_id: source.query_id,
                text: source.text,
                admission_class,
                structural_constraint_count: compiled.constraints.len(),
                has_residual_lexical: !compiled.residual_lexical.is_empty(),
                rows: 10,
                native: None,
                solr: None,
            }
            .with_engine_requests(
                requests.native_q,
                requests.solr_q,
                requests.fq,
                requests.params,
            ),
        );
    }
    if !translation_failures.is_empty() {
        for failure in &translation_failures {
            eprintln!("TRANSLATION_FAILURE {failure}");
        }
        return Err(format!(
            "{} queries contain unresolvable constraints; workload not emitted",
            translation_failures.len()
        )
        .into());
    }
    write_workload(&output_path, &frozen)?;
    let bytes = std::fs::read(&output_path)?;
    println!(
        "HISTOGRAM FastPath={} Hybrid={} Punt={}",
        histogram
            .get(&AdmissionClass::FastPath)
            .copied()
            .unwrap_or(0),
        histogram.get(&AdmissionClass::Hybrid).copied().unwrap_or(0),
        histogram.get(&AdmissionClass::Punt).copied().unwrap_or(0)
    );
    println!("SHA256 {}", sha256_hex(&bytes));
    if dataset != Dataset::Wands {
        return Ok(true);
    }
    let structural = histogram
        .get(&AdmissionClass::FastPath)
        .copied()
        .unwrap_or(0)
        + histogram.get(&AdmissionClass::Hybrid).copied().unwrap_or(0);
    let punt = histogram.get(&AdmissionClass::Punt).copied().unwrap_or(0);
    if structural == 21 && punt == 459 {
        println!("ANCHOR_OK");
        Ok(true)
    } else {
        println!("ANCHOR_MISMATCH structural={structural} punt={punt}");
        Ok(false)
    }
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("i61_workload_freeze: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commerce_core::domain::{BrandId, CategoryId, ProductTypeId};
    use commerce_core::ir::{CommerceQuery, ResolvedConstraint, StructuralConstraint};
    use comparator_eval::translate::StructuralNames;

    // The native engine's `lexical_postings` index is built from
    // `product.title` plus every `AttributeValue::Text` attribute
    // (commerce-core/src/index/mod.rs:197 and :268). Solr's `qf` must name
    // exactly those fields, or the two engines search different corpora and
    // any CPU comparison between them measures different work. WANDS ingests
    // `description` as its only Text attribute
    // (phase6a-eval/src/catalog.rs:142); ESCI ingests `description` and
    // `bullet_point` (issue35-eval/src/lib.rs:138-139). `title description` is
    // also exactly what p9_e02 -- the checkpoint that produced this
    // repository's published WANDS numbers -- sent to Solr.
    #[test]
    fn wands_qf_matches_native_lexical_field_scope() {
        assert_eq!(query_fields(Dataset::Wands), "title description");
    }

    #[test]
    fn esci_qf_matches_native_lexical_field_scope() {
        assert_eq!(
            query_fields(Dataset::EsciElectronics),
            "title description bullet_point"
        );
    }

    struct TestNames;

    impl StructuralNames for TestNames {
        fn brand_name(&self, id: BrandId) -> Option<&str> {
            (id == BrandId(1)).then_some("Acme")
        }

        fn product_type_name(&self, id: ProductTypeId) -> Option<&str> {
            (id == ProductTypeId(1)).then_some("Beds")
        }

        fn category_name(&self, id: CategoryId) -> Option<&str> {
            (id == CategoryId(1)).then_some("Bedroom")
        }
    }

    #[test]
    fn wands_structural_constraints_become_lowercase_companion_fq_clauses() {
        // Given
        let compiled = CommerceQuery {
            constraints: vec![
                ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
                ResolvedConstraint::Structural(StructuralConstraint::Category(CategoryId(1))),
            ],
            residual_lexical: vec!["red".to_string()],
            ..CommerceQuery::default()
        };

        // When
        let requests = engine_requests("q1", "red beds", &compiled, Dataset::Wands, &TestNames)
            .expect("resolvable constraints");

        // Then
        assert_eq!(
            requests.fq,
            [
                "product_class_lc:\"beds\"".to_string(),
                "category_leaf_lc:\"bedroom\"".to_string()
            ]
        );
    }

    #[test]
    fn an_unresolvable_constraint_fails_the_freeze_instead_of_emitting_partial_fq() {
        // Given
        let compiled = CommerceQuery {
            constraints: vec![
                ResolvedConstraint::Structural(StructuralConstraint::ProductType(ProductTypeId(1))),
                ResolvedConstraint::Structural(StructuralConstraint::Category(CategoryId(99))),
            ],
            ..CommerceQuery::default()
        };

        // When
        let error = engine_requests(
            "q-unresolvable",
            "beds",
            &compiled,
            Dataset::Wands,
            &TestNames,
        )
        .expect_err("partial translation must fail");

        // Then
        assert!(error.contains("q-unresolvable"));
    }

    #[test]
    fn a_query_with_no_structural_constraints_emits_an_empty_fq_list() {
        // Given
        let compiled = CommerceQuery {
            residual_lexical: vec!["desk".to_string()],
            ..CommerceQuery::default()
        };

        // When
        let requests = engine_requests("q2", "desk", &compiled, Dataset::Wands, &TestNames)
            .expect("no translation needed");

        // Then
        assert!(requests.fq.is_empty());
    }

    #[test]
    fn a_query_with_no_residual_lexical_uses_match_all_as_q() {
        // Given
        let compiled = CommerceQuery {
            constraints: vec![ResolvedConstraint::Structural(
                StructuralConstraint::ProductType(ProductTypeId(1)),
            )],
            ..CommerceQuery::default()
        };

        // When
        let requests = engine_requests("q3", "beds", &compiled, Dataset::Wands, &TestNames)
            .expect("resolvable constraint");

        // Then
        assert_eq!(requests.solr_q, "*:*");
    }

    #[test]
    fn wands_parser_reads_tab_separated_query_rows_when_valid() {
        let queries = parse_wands_queries("query_id\tquery\n1\tred chair\n2\tdesk\n")
            .expect("valid WANDS queries");
        assert_eq!(queries.len(), 2);
        assert_eq!(
            queries[0],
            SourceQuery {
                query_id: "1".to_string(),
                text: "red chair".to_string()
            }
        );
    }

    #[test]
    fn wands_parser_rejects_malformed_rows_instead_of_dropping_them() {
        let error = parse_wands_queries("query_id\tquery\n1\n").expect_err("malformed row");
        assert!(error.contains("line 2"));
    }

    #[test]
    fn sha256_matches_standard_vector_when_input_is_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
