use issue61_eval::{summarize_memory_samples, CgroupReader, MemorySampler, MEMORY_SAMPLE_INTERVAL};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn sample_summary_records_standard_median_and_maximum() {
    // Given / When
    let summary = summarize_memory_samples(&[9, 1, 7, 3]).expect("non-empty samples");

    // Then
    assert_eq!(summary.median_bytes, 5);
    assert_eq!(summary.max_bytes, 9);
    assert_eq!(MEMORY_SAMPLE_INTERVAL, Duration::from_millis(500));
}

#[test]
fn sampler_is_stopped_and_joined_before_returning_samples() {
    // Given
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "issue61-memory-sampler-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("fixture root");
    std::fs::write(root.join("memory.current"), "4096\n").expect("memory fixture");
    let reader = CgroupReader::at_dir(PathBuf::from(&root));

    // When
    let samples = MemorySampler::start(reader)
        .expect("sampler should start")
        .finish()
        .expect("sampler should join");

    // Then
    assert_eq!(samples.median_bytes, 4096);
    assert_eq!(samples.max_bytes, 4096);
    std::fs::remove_dir_all(root).expect("fixture cleanup");
}

#[test]
fn sampler_appends_closing_boundary_after_subinterval_growth() {
    // Given
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "issue61-memory-sampler-boundary-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("fixture root");
    std::fs::write(root.join("memory.current"), "4096\n").expect("initial memory fixture");
    let sampler = MemorySampler::start(CgroupReader::at_dir(PathBuf::from(&root)))
        .expect("sampler should start");
    std::fs::write(root.join("memory.current"), "8192\n").expect("closing memory fixture");

    // When
    let samples = sampler.finish().expect("sampler should join");

    // Then
    assert_eq!(samples.median_bytes, 6144);
    assert_eq!(samples.max_bytes, 8192);
    std::fs::remove_dir_all(root).expect("fixture cleanup");
}
