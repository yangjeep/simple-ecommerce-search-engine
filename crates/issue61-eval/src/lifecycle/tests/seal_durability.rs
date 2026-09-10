use super::{run1, FakePort};
use crate::lifecycle::{LifecycleError, Operation, SealStage};

#[test]
fn each_seal_stage_failure_stops_verification_and_runs_meaningful_cleanup() {
    // Given / When / Then
    for (stage, expected_error) in [
        (SealStage::CreateNew, LifecycleError::SealCreateNew),
        (SealStage::WriteAll, LifecycleError::SealWriteAll),
        (SealStage::Flush, LifecycleError::SealFlush),
        (SealStage::Sync, LifecycleError::SealSync),
        (SealStage::Close, LifecycleError::SealClose),
    ] {
        let mut fake = FakePort::default();
        fake.fail_seal_stages = vec![stage];

        let error = run1(&mut fake).expect_err("seal stage failure must stop finalization");

        assert_eq!(error, expected_error);
        assert!(!fake.seal_verified);
        assert!(!fake.analyzer_invoked);
        let seal_operations = fake
            .finalization_operations()
            .into_iter()
            .skip_while(|operation| *operation != Operation::CreateSeal)
            .collect::<Vec<_>>();
        let expected_operations = match stage {
            SealStage::CreateNew => vec![Operation::CreateSeal],
            SealStage::WriteAll | SealStage::Flush | SealStage::Sync | SealStage::Close => vec![
                Operation::CreateSeal,
                Operation::WriteSeal { entries: 12 },
                Operation::FlushSeal,
                Operation::SyncSeal,
                Operation::CloseSeal,
            ],
        };
        assert_eq!(seal_operations, expected_operations);
    }
}

#[test]
fn first_seal_stage_error_survives_later_durability_failures() {
    // Given
    let mut fake = FakePort::default();
    fake.fail_seal_stages = vec![
        SealStage::WriteAll,
        SealStage::Flush,
        SealStage::Sync,
        SealStage::Close,
    ];

    // When
    let error = run1(&mut fake).expect_err("first seal failure must survive cleanup");

    // Then
    assert_eq!(error, LifecycleError::SealWriteAll);
    assert!(fake.seal_created);
    assert!(fake.has_durable_seal_close());
    assert!(!fake.seal_verified);
    assert!(!fake.analyzer_invoked);
}
