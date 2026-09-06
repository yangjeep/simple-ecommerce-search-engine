#[path = "support/i61_analyzer.rs"]
mod analyzer;
#[path = "support/i61_fixture/mod.rs"]
mod fixture;

use analyzer::{analyze, analyze_completed};
use fixture::CompletedCycle;

fn assert_cli_rejection(args: &[&str]) {
    let output = analyze(args);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"i61_analyze: expected exactly --repository-root <PATH> --cycle <run1|rerun1|rerun2>\n"
    );
}

#[test]
fn exact_cli_rejects_missing_extra_duplicate_and_reordered_arguments() {
    // Given / When / Then
    for args in [
        &[][..],
        &["--repository-root", "/tmp", "--cycle"][..],
        &["--cycle", "run1", "--repository-root", "/tmp"][..],
        &["--repository-root", "/tmp", "--cycle", "run1", "--force"][..],
        &[
            "--repository-root",
            "/tmp",
            "--repository-root",
            "/tmp",
            "--cycle",
            "run1",
        ][..],
    ] {
        assert_cli_rejection(args);
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_repository_path_is_rejected_without_panicking() {
    use analyzer::analyze_os;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let output = analyze_os(&[
        OsString::from("--repository-root"),
        OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]),
        OsString::from("--cycle"),
        OsString::from("run1"),
    ]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"i61_analyze: repository root does not exist\n"
    );
}

#[test]
fn cycle_path_is_derived_from_repository_root() {
    // Given
    let fixture = CompletedCycle::run1();
    let mut fixture = fixture;
    fixture.rewrite_records(|_| {});
    let root = fixture.root().to_str().expect("fixture root is UTF-8");

    // When
    let output = analyze(&["--repository-root", root, "--cycle", "rerun1"]);

    // Then
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"i61_analyze: cycle directory does not exist\n"
    );
    assert!(!fixture.analysis_path().exists());
}

#[cfg(unix)]
#[test]
fn repository_root_symlink_is_rejected() {
    // Given
    let fixture = CompletedCycle::run1();
    let link = fixture.root().join("repository-link");
    std::os::unix::fs::symlink(fixture.root(), &link).expect("repository symlink is created");

    // When
    let output = analyze(&[
        "--repository-root",
        link.to_str().expect("fixture link is UTF-8"),
        "--cycle",
        "run1",
    ]);

    // Then
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"i61_analyze: repository root must not be a symlink\n"
    );
}

#[cfg(unix)]
#[test]
fn cycle_directory_symlink_is_rejected() {
    // Given
    let fixture = CompletedCycle::run1();
    let cycle = fixture.cycle_dir();
    let backing = cycle.with_file_name("i61_e1_run1_backing");
    std::fs::rename(&cycle, &backing).expect("cycle fixture is moved");
    std::os::unix::fs::symlink(&backing, &cycle).expect("cycle symlink is created");

    // When
    let output = analyze_completed(&fixture);

    // Then
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"i61_analyze: cycle directory must not be a symlink\n"
    );
}
