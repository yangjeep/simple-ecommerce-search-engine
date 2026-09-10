use issue61_eval::{project_workload, AdmissionClass, FrozenQuery, WorkloadProjection};

fn query(id: &str, admission_class: AdmissionClass) -> FrozenQuery {
    FrozenQuery {
        query_id: id.to_string(),
        text: id.to_string(),
        admission_class,
        structural_constraint_count: 0,
        has_residual_lexical: false,
        rows: 10,
        native: None,
        solr: None,
    }
}

#[test]
fn typed_projections_select_all_and_each_admission_class() {
    // Given
    let workload = [
        query("fast", AdmissionClass::FastPath),
        query("hybrid", AdmissionClass::Hybrid),
        query("punt", AdmissionClass::Punt),
    ];

    // When / Then
    assert_eq!(
        project_workload(&workload, WorkloadProjection::All)
            .expect("all is non-empty")
            .query_count(),
        3
    );
    for projection in [
        WorkloadProjection::FastPath,
        WorkloadProjection::Hybrid,
        WorkloadProjection::Punt,
    ] {
        assert_eq!(
            project_workload(&workload, projection)
                .expect("class is non-empty")
                .query_count(),
            1
        );
    }
}

#[test]
fn projection_rejects_an_empty_dataset_or_admission_class() {
    // Given
    let fast_only = [query("fast", AdmissionClass::FastPath)];

    // When / Then
    assert!(project_workload(&[], WorkloadProjection::All).is_err());
    assert!(project_workload(&fast_only, WorkloadProjection::Hybrid).is_err());
}
