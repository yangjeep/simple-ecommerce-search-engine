#[path = "support/i61_analyzer.rs"]
mod analyzer;
#[path = "support/i61_fixture/mod.rs"]
mod fixture;

use analyzer::analyze_completed;
use fixture::CompletedCycle;

fn assert_failure(fixture: &CompletedCycle, expected: &str) {
    let output = analyze_completed(fixture);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        format!("i61_analyze: {expected}\n").as_bytes()
    );
    assert!(!fixture.analysis_path().exists());
}

#[test]
fn cycle_directory_rejects_an_extra_regular_file() {
    let fixture = CompletedCycle::run1();
    std::fs::write(fixture.input_path("unexpected.txt"), b"extra\n").expect("extra file exists");

    assert_failure(
        &fixture,
        "cycle directory contains unexpected entry unexpected.txt",
    );
}

#[test]
fn cycle_directory_rejects_an_extra_directory() {
    let fixture = CompletedCycle::run1();
    std::fs::create_dir(fixture.input_path("unexpected-dir")).expect("extra directory exists");

    assert_failure(
        &fixture,
        "cycle directory contains unexpected entry unexpected-dir",
    );
}

#[test]
fn cycle_directory_rejects_a_required_name_that_is_not_a_regular_file() {
    let fixture = CompletedCycle::run1();
    let path = fixture.input_path("events.jsonl");
    std::fs::remove_file(&path).expect("old event file is removed");
    std::fs::create_dir(path).expect("directory occupies required name");

    assert_failure(
        &fixture,
        "cycle directory entry events.jsonl must be a regular file",
    );
}

#[test]
fn cycle_directory_rejects_a_missing_required_entry() {
    let fixture = CompletedCycle::run1();
    std::fs::remove_file(fixture.input_path("events.jsonl")).expect("event file is removed");

    assert_failure(
        &fixture,
        "cycle directory is missing required entry events.jsonl",
    );
}

#[cfg(unix)]
#[test]
fn cycle_directory_rejects_a_symlink_at_a_required_name() {
    let fixture = CompletedCycle::run1();
    let path = fixture.input_path("events.jsonl");
    std::fs::remove_file(&path).expect("old event file is removed");
    std::os::unix::fs::symlink(fixture.input_path("commands.log"), path)
        .expect("symlink occupies required name");

    assert_failure(
        &fixture,
        "cycle directory entry events.jsonl must be a regular file",
    );
}

#[cfg(unix)]
#[test]
fn cycle_directory_rejects_a_non_utf8_entry_name_without_panicking() {
    use std::os::unix::ffi::OsStringExt;

    let fixture = CompletedCycle::run1();
    let name = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
    std::fs::write(fixture.cycle_dir().join(name), b"extra\n").expect("non-UTF-8 file exists");

    assert_failure(&fixture, "cycle directory contains a non-UTF-8 entry name");
}

#[test]
fn checksum_seal_without_terminal_lf_is_accepted() {
    let fixture = CompletedCycle::run1();
    let path = fixture.manifest_path();
    let mut bytes = std::fs::read(&path).expect("seal is readable");
    assert_eq!(bytes.pop(), Some(b'\n'));
    std::fs::write(path, bytes).expect("no-LF seal is written");

    let output = analyze_completed(&fixture);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(fixture.analysis_path().exists());
}

#[test]
fn checksum_seal_rejects_crlf() {
    let fixture = CompletedCycle::run1();
    let path = fixture.manifest_path();
    let text = std::fs::read_to_string(&path).expect("seal is readable");
    std::fs::write(path, text.replace('\n', "\r\n")).expect("CRLF seal is written");

    assert_failure(&fixture, "malformed checksum seal");
}

#[test]
fn checksum_seal_rejects_more_than_one_terminal_lf() {
    let fixture = CompletedCycle::run1();
    let path = fixture.manifest_path();
    let mut bytes = std::fs::read(&path).expect("seal is readable");
    bytes.push(b'\n');
    std::fs::write(path, bytes).expect("blank terminal line is written");

    assert_failure(&fixture, "malformed checksum seal");
}

#[test]
fn analysis_collision_takes_precedence_over_directory_enumeration() {
    let fixture = CompletedCycle::run1();
    std::fs::write(fixture.analysis_path(), b"sentinel\n").expect("collision exists");
    std::fs::write(fixture.input_path("unexpected.txt"), b"extra\n").expect("extra file exists");

    let output = analyze_completed(&fixture);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        output.stderr,
        b"i61_analyze: analysis.json already exists\n"
    );
    assert_eq!(
        std::fs::read(fixture.analysis_path()).expect("collision remains"),
        b"sentinel\n"
    );
}
