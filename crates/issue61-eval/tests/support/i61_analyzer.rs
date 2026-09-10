#![allow(dead_code)]

use crate::fixture::CompletedCycle;
use std::ffi::OsString;
use std::process::{Command, Output};

pub fn analyze(args: &[&str]) -> Output {
    let executable = env!("CARGO_BIN_EXE_i61_analyze");
    Command::new(executable)
        .args(args)
        .output()
        .expect("analyzer binary executes")
}

pub fn analyze_os(args: &[OsString]) -> Output {
    let executable = env!("CARGO_BIN_EXE_i61_analyze");
    Command::new(executable)
        .args(args)
        .output()
        .expect("analyzer binary executes")
}

pub fn analyze_completed(fixture: &CompletedCycle) -> Output {
    analyze(&[
        "--repository-root",
        fixture.root().to_str().expect("fixture root is UTF-8"),
        "--cycle",
        "run1",
    ])
}
