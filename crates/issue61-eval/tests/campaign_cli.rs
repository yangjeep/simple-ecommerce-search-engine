use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const EXPECTED: &str = "I61_CAMPAIGN_DRY_RUN cycle=run1 logical_pairs=310 sessions=620 warm_sessions=480 calibration_sessions=120 cold_sessions=20 stability_cells=48 exact_index_cells=4 external_commands=0 writes=0\n";

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn create() -> Self {
        loop {
            let nonce = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "issue61-campaign-test-{}-{nonce}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("failed to create isolated directory {path:?}: {error}"),
            }
        }
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn assert_empty(&self) {
        assert_eq!(
            std::fs::read_dir(&self.0)
                .expect("isolated directory remains readable")
                .count(),
            0
        );
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("isolated directory cleanup succeeds");
    }
}

fn campaign(args: &[&str], current_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_i61_campaign"))
        .args(args)
        .current_dir(current_dir)
        .output()
        .expect("campaign binary executes")
}

#[test]
fn dry_run_prints_exactly_the_frozen_summary_and_succeeds() {
    // Given
    let directory = TemporaryDirectory::create();

    // When
    let output = campaign(&["--dry-run", "--cycle", "run1"], directory.path());

    // Then
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        EXPECTED
    );
    assert!(output.stderr.is_empty());
    directory.assert_empty();
}

#[test]
fn non_dry_run_and_third_rerun_fail_closed() {
    // Given / When / Then
    for args in [
        &["--cycle", "run1"][..],
        &["--dry-run", "--cycle", "rerun3"][..],
    ] {
        let directory = TemporaryDirectory::create();
        let output = campaign(args, directory.path());
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(output.stdout.is_empty());
        directory.assert_empty();
    }
}

#[test]
fn unknown_duplicate_missing_and_execution_shaped_arguments_are_rejected() {
    // Given / When / Then
    for args in [
        &["--dry-run", "--cycle"][..],
        &["--dry-run", "--dry-run", "--cycle", "run1"][..],
        &["--dry-run", "--cycle", "run1", "--unknown"][..],
        &["--dry-run", "--cycle", "run1", "--execute"][..],
    ] {
        let directory = TemporaryDirectory::create();
        let output = campaign(args, directory.path());
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(output.stdout.is_empty());
        directory.assert_empty();
    }
}

#[test]
fn exact_execution_dispatch_reaches_static_validation() {
    // Given
    let directory = TemporaryDirectory::create();
    let repository_root = std::fs::canonicalize(directory.path())
        .expect("temporary repository root has a canonical path");
    let repository_root = repository_root
        .to_str()
        .expect("temporary repository root is UTF-8");

    // When
    let output = campaign(
        &[
            "--execute",
            "--repository-root",
            repository_root,
            "--cycle",
            "run1",
        ],
        directory.path(),
    );
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");

    // Then
    assert!(
        !stderr.contains("execution is unavailable"),
        "execution dispatch remained on the dry-run-only branch; stderr: {stderr:?}"
    );
    assert!(
        stderr.contains("static validation"),
        "execution dispatch did not reach static validation; stderr: {stderr:?}"
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    directory.assert_empty();
}

#[test]
fn mixed_reordered_and_unknown_execution_arguments_fail_closed() {
    // Given / When / Then
    for args in [
        &[
            "--execute",
            "--repository-root",
            "/canonical/repository",
            "--cycle",
        ][..],
        &[
            "--repository-root",
            "/canonical/repository",
            "--execute",
            "--cycle",
            "run1",
        ][..],
        &[
            "--execute",
            "--repository-root",
            "/canonical/repository",
            "--cycle",
            "run1",
            "--unknown",
        ][..],
        &[
            "--execute",
            "--execute",
            "--repository-root",
            "/canonical/repository",
            "--cycle",
            "run1",
        ][..],
        &[
            "--execute",
            "--dry-run",
            "--repository-root",
            "/canonical/repository",
            "--cycle",
            "run1",
        ][..],
        &[
            "--execute",
            "--repository-root",
            "/canonical/repository",
            "--cycle",
            "rerun3",
        ][..],
    ] {
        let directory = TemporaryDirectory::create();
        let output = campaign(args, directory.path());
        assert!(!output.status.success(), "accepted {args:?}");
        assert!(output.stdout.is_empty());
        directory.assert_empty();
    }
}
