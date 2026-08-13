use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_describes_stable_invocation_contract() {
    let mut command = Command::cargo_bin("argos-explorer").unwrap();
    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "argos-explorer.exe [OPTIONS] [DIRECTORY]",
        ))
        .stdout(predicate::str::contains("--icons <ICONS>"));
}

#[test]
fn file_argument_fails_before_terminal_mode() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut command = Command::cargo_bin("argos-explorer").unwrap();
    command
        .arg(file.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Workspace Root is not a directory",
        ));
}

#[test]
fn inaccessible_root_has_actionable_error() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    assert!(!missing.exists());
    let mut command = Command::cargo_bin("argos-explorer").unwrap();
    command
        .arg(&missing)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "does not exist or is inaccessible",
        ));
    drop(fs::metadata(temp.path()));
}

#[test]
fn update_help_exposes_stable_and_preview_modes() {
    let mut command = Command::cargo_bin("argos-explorer").unwrap();
    command
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--preview"))
        .stdout(predicate::str::contains("--check"));
}
