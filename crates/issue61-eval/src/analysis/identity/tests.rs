use crate::analysis::test_support::calibration_records;
use crate::{analyze_calibration, AnalysisError, CampaignCycle, Engine, RawRecord, SessionMode};

#[derive(Clone, Copy)]
enum KeyMutation {
    Calibration,
    Engine,
    Dataset,
    QueryClass,
    Regime,
    Rep,
    EngineOrder,
}

fn mutate_key(record: &mut RawRecord, mutation: KeyMutation) {
    match mutation {
        KeyMutation::Calibration => record.calibration = false,
        KeyMutation::Engine => {
            record.engine = match record.engine.as_str() {
                "native" => Engine::Solr.as_str(),
                "solr" => Engine::Native.as_str(),
                _ => Engine::Native.as_str(),
            }
            .to_owned();
        }
        KeyMutation::Dataset => record.dataset = "esci_electronics".to_owned(),
        KeyMutation::QueryClass => record.query_class = "fast-path".to_owned(),
        KeyMutation::Regime => record.regime = SessionMode::Warm.as_str().to_owned(),
        KeyMutation::Rep => record.rep = 30,
        KeyMutation::EngineOrder => record.engine_order = 1 - record.engine_order,
    }
}

#[test]
fn calibration_pairs_by_typed_identity_when_records_are_reversed() {
    // Given
    let mut records = calibration_records();
    records.reverse();

    // When
    let calibrations = match analyze_calibration(CampaignCycle::Run1, &records) {
        Ok(calibrations) => calibrations,
        Err(error) => panic!("canonical reversed records must validate: {error}"),
    };

    // Then
    for check in [calibrations.native, calibrations.solr] {
        let check = match check {
            Some(check) => check,
            None => panic!("both engine calibrations must be present"),
        };
        assert_eq!(check.expected_ratio, 1.25);
        assert_eq!(check.observed.n_blocks, 30);
        assert_eq!(check.observed.point_ratio, 1.25);
        assert!(check.passed);
    }
}

#[test]
fn calibration_rejects_each_mutated_canonical_key_field() {
    for mutation in [
        KeyMutation::Calibration,
        KeyMutation::Engine,
        KeyMutation::Dataset,
        KeyMutation::QueryClass,
        KeyMutation::Regime,
        KeyMutation::Rep,
        KeyMutation::EngineOrder,
    ] {
        // Given
        let mut records = calibration_records();
        mutate_key(&mut records[0], mutation);

        // When
        let result = analyze_calibration(CampaignCycle::Run1, &records);

        // Then
        assert!(result.is_err());
    }
}

#[test]
fn calibration_rejects_missing_duplicate_and_malformed_identities() {
    // Given / When / Then: missing
    let mut missing = calibration_records();
    missing.pop();
    assert!(matches!(
        analyze_calibration(CampaignCycle::Run1, &missing),
        Err(AnalysisError::MissingIdentity { .. })
    ));

    // Given / When / Then: duplicate
    let mut duplicate = calibration_records();
    duplicate.push(duplicate[0].clone());
    assert!(matches!(
        analyze_calibration(CampaignCycle::Run1, &duplicate),
        Err(AnalysisError::DuplicateIdentity { .. })
    ));

    // Given / When / Then: malformed
    let mut malformed = calibration_records();
    malformed[0].engine = "not-an-engine".to_owned();
    assert!(matches!(
        analyze_calibration(CampaignCycle::Run1, &malformed),
        Err(AnalysisError::MalformedIdentity { .. })
    ));
}

#[test]
fn calibration_rejects_invalid_record_envelope() {
    let mutations: &[fn(&mut RawRecord)] = &[
        |record| record.schema_version += 1,
        |record| record.excluded = true,
        |record| record.exclusion_reason = Some("fixture exclusion".to_owned()),
        |record| record.experiment_id = "wrong-experiment".to_owned(),
        |record| record.run_id = "wrong-run".to_owned(),
    ];
    for mutate in mutations {
        // Given
        let mut records = calibration_records();
        mutate(&mut records[0]);

        // When
        let result = analyze_calibration(CampaignCycle::Run1, &records);

        // Then
        assert!(result.is_err());
    }
}
