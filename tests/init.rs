#![allow(deprecated)]

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn test_init_creates_local_config() {
    let temp = assert_fs::TempDir::new().unwrap();

    Command::cargo_bin("ofsht")
        .unwrap()
        .arg("init")
        .arg("--local")
        .current_dir(&temp)
        .assert()
        .success()
        .stderr(predicate::str::contains("Created Local config"));

    temp.child(".ofsht.toml").assert(predicate::path::exists());

    temp.close().unwrap();
}

#[test]
fn test_init_skip_existing_without_force() {
    let temp = assert_fs::TempDir::new().unwrap();
    let config = temp.child(".ofsht.toml");
    config.write_str("# existing content").unwrap();

    Command::cargo_bin("ofsht")
        .unwrap()
        .arg("init")
        .arg("--local")
        .current_dir(&temp)
        .assert()
        .success()
        .stderr(predicate::str::contains("already exists"))
        .stderr(predicate::str::contains("--force"));

    // Original content should be preserved
    config.assert("# existing content");

    temp.close().unwrap();
}

#[test]
fn test_init_force_overwrite() {
    let temp = assert_fs::TempDir::new().unwrap();
    let config = temp.child(".ofsht.toml");
    config.write_str("# existing content").unwrap();

    Command::cargo_bin("ofsht")
        .unwrap()
        .arg("init")
        .arg("--local")
        .arg("--force")
        .current_dir(&temp)
        .assert()
        .success()
        .stderr(predicate::str::contains("Created Local config"));

    // Content should be replaced with template
    config.assert(predicate::str::contains("ofsht project configuration"));

    temp.close().unwrap();
}

#[test]
fn test_init_global_creates_global_config() {
    // This test may encounter an existing global config, which is fine
    // We just verify the command succeeds and mentions global config
    Command::cargo_bin("ofsht")
        .unwrap()
        .arg("init")
        .arg("--global")
        .assert()
        .success()
        .stderr(predicate::str::contains("Global config"));
}

#[test]
fn test_init_without_flags_creates_both() {
    let temp = assert_fs::TempDir::new().unwrap();

    Command::cargo_bin("ofsht")
        .unwrap()
        .arg("init")
        .current_dir(&temp)
        .assert()
        .success()
        .stderr(predicate::str::contains("Global config"))
        .stderr(predicate::str::contains("Local config"));

    // Local config should be created
    temp.child(".ofsht.toml").assert(predicate::path::exists());

    temp.close().unwrap();
}

#[test]
fn test_init_global_and_local_flags_conflict() {
    Command::cargo_bin("ofsht")
        .unwrap()
        .arg("init")
        .arg("--global")
        .arg("--local")
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}
