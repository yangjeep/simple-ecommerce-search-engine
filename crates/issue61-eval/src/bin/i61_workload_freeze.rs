use commerce_core::index::CatalogIndex;
use commerce_core::ir::compile;
use commerce_core::plan::{plan, ExecutionOutcome, PlannerPolicy};
use issue61_eval::{
    freeze_engine_requests, load_dataset, sha256_hex as digest_hex, write_workload, AdmissionClass,
    Dataset, FreezeRequest, FrozenQuery,
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
        let requests = match freeze_engine_requests(FreezeRequest {
            query_id: &source.query_id,
            text: &source.text,
            compiled: &compiled,
            dataset,
            names: &loaded,
        }) {
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
            .with_engine_requests(requests.native, requests.solr),
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
