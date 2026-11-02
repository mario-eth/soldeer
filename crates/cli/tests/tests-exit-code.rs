use std::{fs, process::Command};
use testdir::testdir;

#[test]
fn test_exit_code_failure() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_soldeer"))
        .args(["uninstall", "nonexistent"])
        .current_dir(&dir)
        .env("SOLDEER_PROJECT_ROOT", &dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("dependency nonexistent was not found"), "{stderr}");
}

#[test]
fn test_exit_code_success() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_soldeer"))
        .arg("clean")
        .current_dir(&dir)
        .env("SOLDEER_PROJECT_ROOT", &dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
