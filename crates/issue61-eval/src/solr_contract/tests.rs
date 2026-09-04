use super::{validate_contract, ContractDocuments, SolrDataset};
use serde_json::{json, Value};

fn schema(dataset: SolrDataset) -> Value {
    let (lexical, companions, copies) = match dataset {
        SolrDataset::Wands => (
            vec!["title", "description"],
            vec!["product_class_lc", "category_leaf_lc"],
            vec![
                json!({"source": "product_class", "dest": "product_class_lc"}),
                json!({"source": "category_leaf", "dest": "category_leaf_lc"}),
            ],
        ),
        SolrDataset::EsciElectronics => (
            vec!["title", "description", "bullet_point"],
            vec!["brand_lc", "color_lc"],
            vec![
                json!({"source": "brand", "dest": "brand_lc"}),
                json!({"source": "color", "dest": "color_lc"}),
            ],
        ),
    };
    let mut fields = vec![json!({
        "name": "id", "type": "string", "indexed": true,
        "stored": true, "multiValued": false
    })];
    fields.extend(
        lexical
            .into_iter()
            .map(|name| json!({"name": name, "type": "text_general", "indexed": true})),
    );
    fields.extend(companions.into_iter().map(|name| {
        json!({
            "name": name, "type": "string_lc", "indexed": true,
            "stored": false, "multiValued": false
        })
    }));
    json!({
        "uniqueKey": "id",
        "fields": fields,
        "fieldTypes": [{
            "name": "string_lc",
            "class": "solr.TextField",
            "analyzer": {
                "tokenizer": {"class": "solr.KeywordTokenizerFactory"},
                "filters": [{"class": "solr.LowerCaseFilterFactory"}]
            }
        }],
        "copyFields": copies
    })
}

fn config() -> Value {
    json!({"query": {
        "filterCache": {"size": "4096", "initialSize": 4096, "autowarmCount": "0"},
        "queryResultCache": {"size": 0, "initialSize": "0", "autowarmCount": 0},
        "documentCache": {"size": "4096", "initialSize": "4096", "autowarmCount": "0"}
    }})
}

fn validate_matching(dataset: SolrDataset, schema: &Value, config: &Value) -> Result<(), String> {
    let live_schema = json!({"responseHeader": {"status": 0}, "schema": schema});
    let live_config = json!({"responseHeader": {"status": 0}, "config": config});
    validate_contract(
        dataset,
        ContractDocuments {
            live_schema_envelope: &live_schema,
            live_config_envelope: &live_config,
            expected_schema: schema,
            expected_config: config,
        },
    )
    .map_err(|error| error.to_string())
}

#[test]
fn dataset_metadata_when_supported_names_are_parsed() {
    // Given / When
    let wands = SolrDataset::parse("wands").expect("wands must parse");
    let esci = SolrDataset::parse("esci_electronics").expect("ESCI must parse");

    // Then
    assert_eq!(wands.core_name(), "i61_wands");
    assert_eq!(wands.schema_snapshot(), "solr_wands_schema.json");
    assert_eq!(wands.config_snapshot(), "solr_wands_config.json");
    assert_eq!(esci.core_name(), "i61_esci_electronics");
    assert_eq!(esci.schema_snapshot(), "solr_esci_electronics_schema.json");
    assert_eq!(esci.config_snapshot(), "solr_esci_electronics_config.json");
}

#[test]
fn dataset_parse_when_name_is_unknown_reports_supported_values() {
    // Given / When
    let error = SolrDataset::parse("beauty").expect_err("unknown dataset must fail");

    // Then
    assert!(error.to_string().contains("wands | esci_electronics"));
}

#[test]
fn contract_when_both_dataset_contracts_match_accepts_semantic_json() {
    for dataset in [SolrDataset::Wands, SolrDataset::EsciElectronics] {
        // Given
        let expected_schema = schema(dataset);
        let expected_config = config();

        // When
        let result = validate_matching(dataset, &expected_schema, &expected_config);

        // Then
        assert_eq!(result, Ok(()));
    }
}

#[test]
fn contract_when_live_schema_differs_reports_snapshot_path() {
    // Given
    let expected_schema = schema(SolrDataset::Wands);
    let expected_config = config();
    let mut live_schema = expected_schema.clone();
    live_schema["version"] = json!(1.8);
    let schema_envelope = json!({"schema": live_schema});
    let config_envelope = json!({"config": expected_config});

    // When
    let error = validate_contract(
        SolrDataset::Wands,
        ContractDocuments {
            live_schema_envelope: &schema_envelope,
            live_config_envelope: &config_envelope,
            expected_schema: &expected_schema,
            expected_config: &expected_config,
        },
    )
    .expect_err("schema drift must fail");

    // Then
    assert!(error.to_string().contains("schema drift"));
    assert!(error.to_string().contains("solr_wands_schema.json"));
}

#[test]
fn contract_when_live_config_differs_reports_snapshot_path() {
    // Given
    let expected_schema = schema(SolrDataset::EsciElectronics);
    let expected_config = config();
    let mut live_config = expected_config.clone();
    live_config["query"]["filterCache"]["size"] = json!(2048);
    let schema_envelope = json!({"schema": expected_schema});
    let config_envelope = json!({"config": live_config});

    // When
    let error = validate_contract(
        SolrDataset::EsciElectronics,
        ContractDocuments {
            live_schema_envelope: &schema_envelope,
            live_config_envelope: &config_envelope,
            expected_schema: &expected_schema,
            expected_config: &expected_config,
        },
    )
    .expect_err("config drift must fail");

    // Then
    assert!(error.to_string().contains("config drift"));
    assert!(error
        .to_string()
        .contains("solr_esci_electronics_config.json"));
}

#[test]
fn contract_when_only_live_znode_version_differs_accepts_normalized_config() {
    // Given
    let expected_schema = schema(SolrDataset::Wands);
    let expected_config = config();
    let mut live_config = expected_config.clone();
    live_config["znodeVersion"] = json!(1829200977);
    let schema_envelope = json!({"schema": expected_schema});
    let config_envelope = json!({"config": live_config});

    // When
    let result = validate_contract(
        SolrDataset::Wands,
        ContractDocuments {
            live_schema_envelope: &schema_envelope,
            live_config_envelope: &config_envelope,
            expected_schema: &expected_schema,
            expected_config: &expected_config,
        },
    );

    // Then
    assert_eq!(result.map_err(|error| error.to_string()), Ok(()));
}

#[test]
fn contract_when_expected_config_contains_znode_version_rejects_snapshot() {
    // Given
    let expected_schema = schema(SolrDataset::EsciElectronics);
    let mut expected_config = config();
    expected_config["znodeVersion"] = json!(1789923749);

    // When
    let error = validate_matching(
        SolrDataset::EsciElectronics,
        &expected_schema,
        &expected_config,
    )
    .expect_err("volatile metadata in frozen config must fail");

    // Then
    assert!(error.contains("config.znodeVersion"));
    assert!(error.contains("omitted from frozen snapshot"));
}

#[test]
fn contract_when_schema_snapshot_breaks_e1_invariants_rejects_straw_baseline() {
    let cases = [
        ("/uniqueKey", json!("sku"), "uniqueKey"),
        ("/fields/0/stored", json!(false), "field id.stored"),
        ("/fields/1/type", json!("string"), "field title.type"),
        (
            "/fields/3/stored",
            json!(true),
            "field product_class_lc.stored",
        ),
        (
            "/fieldTypes/0/analyzer/tokenizer/class",
            json!("solr.StandardTokenizerFactory"),
            "string_lc analyzer.tokenizer.class",
        ),
        (
            "/copyFields/0/dest",
            json!("wrong"),
            "copyField product_class",
        ),
    ];
    for (pointer, replacement, expected_message) in cases {
        // Given
        let mut invalid = schema(SolrDataset::Wands);
        *invalid
            .pointer_mut(pointer)
            .expect("fixture path must exist") = replacement;

        // When
        let error = validate_matching(SolrDataset::Wands, &invalid, &config())
            .expect_err("invalid frozen schema must fail");

        // Then
        assert!(error.contains(expected_message), "{error}");
    }
}

#[test]
fn contract_when_cache_snapshot_breaks_e1_invariants_reports_semantic_value() {
    // Given
    let expected_schema = schema(SolrDataset::EsciElectronics);
    let mut invalid_config = config();
    invalid_config["query"]["documentCache"]["autowarmCount"] = json!("1");

    // When
    let error = validate_matching(
        SolrDataset::EsciElectronics,
        &expected_schema,
        &invalid_config,
    )
    .expect_err("invalid frozen config must fail");

    // Then
    assert!(error.contains("query.documentCache.autowarmCount"));
    assert!(error.contains("expected integer 0, got \"1\""));
}

#[test]
fn contract_when_live_envelope_lacks_inner_object_reports_endpoint() {
    // Given
    let expected_schema = schema(SolrDataset::Wands);
    let expected_config = config();
    let schema_envelope = json!({"responseHeader": {"status": 0}});
    let config_envelope = json!({"config": expected_config});

    // When
    let error = validate_contract(
        SolrDataset::Wands,
        ContractDocuments {
            live_schema_envelope: &schema_envelope,
            live_config_envelope: &config_envelope,
            expected_schema: &expected_schema,
            expected_config: &expected_config,
        },
    )
    .expect_err("missing schema must fail");

    // Then
    assert!(error
        .to_string()
        .contains("live /schema response missing .schema"));
}
