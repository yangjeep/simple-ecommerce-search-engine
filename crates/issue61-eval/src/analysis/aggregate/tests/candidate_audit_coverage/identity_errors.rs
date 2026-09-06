use super::{assert_error, validate};
use crate::analysis::candidate_audit::validate_candidate_audits;
use crate::analysis::test_support::CompleteCandidateAuditFixture;
use crate::{CandidateAuditError, Dataset};

#[test]
fn duplicate_dataset_evidence_returns_exact_multiplicity_error() {
    // Given: WANDS evidence appears twice while ESCI evidence is absent.
    let fixture = CompleteCandidateAuditFixture::complete();
    let evidence = fixture.evidence();

    // When: the candidate evidence set is validated.
    let result = validate_candidate_audits(&[evidence[0], evidence[0]]);

    // Then: dataset multiplicity identifies WANDS and the duplicate count.
    assert_eq!(
        result,
        Err(CandidateAuditError::DatasetMultiplicity {
            dataset: Dataset::Wands,
            found: 2,
        })
    );
}

#[test]
fn audit_record_dataset_mismatch_returns_exact_error() {
    // Given: one WANDS evidence record names the ESCI dataset.
    let mut fixture = CompleteCandidateAuditFixture::complete();
    fixture.wands_records[0].dataset = Dataset::EsciElectronics.as_str().to_owned();

    // When/Then: the exact source record is rejected.
    assert_error(
        &fixture,
        CandidateAuditError::AuditDatasetMismatch {
            dataset: Dataset::Wands,
            record: 0,
        },
    );
}

#[test]
fn duplicate_workload_query_id_returns_exact_error() {
    // Given: the second WANDS workload query duplicates the first ID.
    let mut fixture = CompleteCandidateAuditFixture::complete();
    fixture.wands_workload[1].query_id = fixture.wands_workload[0].query_id.clone();

    // When/Then: the duplicate workload position is reported exactly.
    assert_error(
        &fixture,
        CandidateAuditError::DuplicateWorkloadQueryId {
            dataset: Dataset::Wands,
            query: 1,
        },
    );
}

#[test]
fn duplicate_audit_query_id_returns_exact_error() {
    // Given: the second WANDS audit record duplicates the first query ID.
    let mut fixture = CompleteCandidateAuditFixture::complete();
    fixture.wands_records[1].query_id = fixture.wands_records[0].query_id.clone();

    // When/Then: the duplicate audit position is reported exactly.
    assert_error(
        &fixture,
        CandidateAuditError::DuplicateAuditQueryId {
            dataset: Dataset::Wands,
            record: 1,
        },
    );
}

#[test]
fn unexpected_audit_query_id_returns_exact_error() {
    // Given: one WANDS audit record has no frozen workload query.
    let mut fixture = CompleteCandidateAuditFixture::complete();
    fixture.wands_records[0].query_id = "unexpected-query".to_owned();

    // When: the complete evidence is validated.
    let result = validate(&fixture);

    // Then: the unexpected audit record is identified exactly.
    assert_eq!(
        result,
        Err(CandidateAuditError::UnexpectedAuditQueryId {
            dataset: Dataset::Wands,
            record: 0,
        })
    );
}
