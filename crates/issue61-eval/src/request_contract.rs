use crate::{Dataset, NativeRequest, SolrRequest};
use commerce_core::ir::CommerceQuery;
use comparator_eval::translate::{
    translate_all_with_config, SolrFieldMap, SolrTranslationConfig, StructuralNames,
};
use std::collections::BTreeMap;

pub struct FreezeRequest<'a> {
    pub query_id: &'a str,
    pub text: &'a str,
    pub compiled: &'a CommerceQuery,
    pub dataset: Dataset,
    pub names: &'a dyn StructuralNames,
}

#[derive(Debug)]
pub struct FrozenEngineRequests {
    pub native: NativeRequest,
    pub solr: SolrRequest,
}

pub fn freeze_engine_requests(input: FreezeRequest<'_>) -> Result<FrozenEngineRequests, String> {
    let (fq, unresolvable) = translate_all_with_config(
        &input.compiled.constraints,
        input.names,
        &solr_config(input.dataset),
    );
    if !unresolvable.is_empty() {
        return Err(format!(
            "query {}: unresolvable constraints: {}",
            input.query_id,
            unresolvable.join("; ")
        ));
    }
    if input.compiled.residual_lexical.is_empty() && fq.is_empty() {
        return Err(format!(
            "query {}: neither residual terms nor structural filters",
            input.query_id
        ));
    }

    let solr_q = if input.compiled.residual_lexical.is_empty() {
        String::from("*:*")
    } else {
        escape_edismax_literal(&input.compiled.residual_lexical.join(" "))
    };
    Ok(FrozenEngineRequests {
        native: NativeRequest {
            q: input.text.to_string(),
            params: BTreeMap::from([("rows".to_string(), "10".to_string())]),
        },
        solr: SolrRequest {
            q: solr_q,
            fq,
            params: solr_params(input.dataset),
        },
    })
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

const fn query_fields(dataset: Dataset) -> &'static str {
    match dataset {
        Dataset::Wands => "title description",
        Dataset::EsciElectronics => "title description bullet_point",
    }
}

fn solr_params(dataset: Dataset) -> BTreeMap<String, String> {
    [
        ("defType", "edismax"),
        ("fl", "id"),
        ("lowercaseOperators", "false"),
        ("mm", "100%"),
        ("mm.autoRelax", "false"),
        ("ps", "0"),
        ("ps2", "0"),
        ("ps3", "0"),
        ("q.op", "AND"),
        ("qf", query_fields(dataset)),
        ("qs", "0"),
        ("rows", "10"),
        ("sort", "id asc"),
        ("sow", "true"),
        ("tie", "0.0"),
        ("wt", "json"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect()
}

fn escape_edismax_literal(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for character in input.chars() {
        if matches!(
            character,
            '\\' | '+'
                | '-'
                | '&'
                | '|'
                | '!'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '^'
                | '"'
                | '~'
                | '*'
                | '?'
                | ':'
                | '/'
        ) {
            escaped.push('\\');
            escaped.push(character);
        } else {
            escaped.push(character);
        }
    }
    escaped
}

#[cfg(test)]
#[path = "request_contract/tests.rs"]
mod tests;
