use super::{frozen_record, http_capture, workload_pass};
use issue61_eval::{project_workload, Engine, WorkloadProjection};
use std::collections::BTreeMap;
use std::time::Duration;

#[test]
fn timed_solr_get_transmits_the_complete_frozen_contract() {
    // Given
    let response =
        r#"{"responseHeader":{"status":0},"response":{"numFound":1,"docs":[{"id":"P1"}]}}"#;
    let (base_url, capture) = http_capture::spawn_capture_server(response);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    let workload = vec![frozen_record()];
    let projected = project_workload(&workload, WorkloadProjection::All).expect("project workload");

    // When
    let observations =
        workload_pass(&agent, &base_url, projected, Engine::Solr).expect("timed request succeeds");
    let captured = capture.join().expect("capture server succeeds");

    // Then
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].num_found, 1);
    assert_eq!(captured.path, "/select");
    assert_eq!(
        captured.params,
        BTreeMap::from([
            ("defType".to_string(), vec!["edismax".to_string()]),
            ("fl".to_string(), vec!["id".to_string()]),
            (
                "fq".to_string(),
                vec![
                    "product_class_lc:\"beds\"".to_string(),
                    "color_lc:\"red\"".to_string(),
                ],
            ),
            ("q".to_string(), vec!["red".to_string()]),
            ("qf".to_string(), vec!["title".to_string()]),
            ("rows".to_string(), vec!["10".to_string()]),
        ])
    );
}
