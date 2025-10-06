use soldeer_commands::{
    Command, Verbosity,
    commands::{clean::Clean, install::Install},
    run,
};
use soldeer_core::{config::read_config_deps, lock::read_lockfile};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::{Path, PathBuf},
};
use temp_env::async_with_vars;
use testdir::testdir;

#[allow(clippy::unwrap_used)]
fn check_clean_success(dir: &Path, config_filename: &str) {
    assert!(!dir.join("dependencies").exists(), "Dependencies folder should be removed");
    assert!(!dir.join("soldeer.lock").exists(), "Lock file should be removed");

    let config_path = dir.join(config_filename);
    assert!(config_path.exists(), "Config file should be preserved");

    let (deps, _) = read_config_deps(&config_path).unwrap();
    assert_eq!(deps.len(), 2, "Config should still have 2 dependencies");
    assert_eq!(deps[0].name(), "@openzeppelin-contracts");
    assert_eq!(deps[1].name(), "solady");
}

#[allow(clippy::unwrap_used)]
fn check_artifacts_exist(dir: &Path) {
    assert!(dir.join("dependencies").exists(), "Dependencies folder should exist");
    assert!(dir.join("soldeer.lock").exists(), "Lock file should exist");

    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(lock.entries.len(), 2, "Lock file should have 2 entries");
    let deps_dir = dir.join("dependencies");
    let entries: Vec<_> = fs::read_dir(&deps_dir).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(!entries.is_empty(), "Dependencies directory should have content");
}

#[allow(clippy::unwrap_used)]
async fn setup_project_with_dependencies(config_filename: &str) -> PathBuf {
    let dir = testdir!();
    let mut contents = r#"[dependencies]
"@openzeppelin-contracts" = "5.0.2"
solady = "0.0.238"
"#
    .to_string();
    if config_filename == "foundry.toml" {
        contents = format!(
            r#"[profile.default]
libs = ["dependencies"]

{contents}"#
        );
    }
    fs::write(dir.join(config_filename), contents).unwrap();
    let cmd: Command = Install::default().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    dir
}

#[tokio::test]
async fn test_clean_basic() {
    let dir = setup_project_with_dependencies("soldeer.toml").await;

    assert!(dir.join("dependencies").exists());
    assert!(dir.join("soldeer.lock").exists());

    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    check_clean_success(&dir, "soldeer.toml");
}

#[tokio::test]
async fn test_clean_foundry_config() {
    let dir = setup_project_with_dependencies("foundry.toml").await;
    check_artifacts_exist(&dir);
    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    check_clean_success(&dir, "foundry.toml");
}

#[tokio::test]
async fn test_clean_no_artifacts() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();

    // Run clean on empty project (no dependencies folder or lock file)
    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;

    // Should succeed silently
    assert!(res.is_ok(), "{res:?}");
}

#[tokio::test]
async fn test_clean_restores_with_install() {
    let dir = setup_project_with_dependencies("soldeer.toml").await;

    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    assert!(!dir.join("dependencies").exists());
    assert!(!dir.join("soldeer.lock").exists());

    let cmd: Command = Install::default().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    assert!(dir.join("dependencies").exists());
    assert!(dir.join("soldeer.lock").exists());

    let dependencies_dir = dir.join("dependencies");
    let entries: Vec<_> =
        fs::read_dir(dependencies_dir).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(!entries.is_empty(), "Dependencies should be installed");
}

#[tokio::test]
async fn test_clean_with_complex_file_structure() {
    let dir = setup_project_with_dependencies("soldeer.toml").await;

    let complex_path = dir.join("dependencies").join("nested").join("deep").join("structure");
    fs::create_dir_all(&complex_path).unwrap();
    fs::write(complex_path.join("test.txt"), "nested content").unwrap();

    // Create symlink (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink(dir.join("soldeer.toml"), dir.join("dependencies").join("config_link"));
    }

    // Create large file to test performance
    let large_content = "x".repeat(1024 * 1024); // 1MB
    fs::write(dir.join("dependencies").join("large_file.txt"), large_content).unwrap();

    let cmd: Command = Clean::builder().build().into();
    let res: Result<(), soldeer_core::SoldeerError> = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;

    assert!(res.is_ok(), "{res:?}");
    check_clean_success(&dir, "soldeer.toml");
}

#[tokio::test]
async fn test_clean_permission_error() {
    let dir = setup_project_with_dependencies("soldeer.toml").await;

    #[cfg(unix)]
    {
        let deps_path = dir.join("dependencies");
        let mut perms = fs::metadata(&deps_path).unwrap().permissions();
        perms.set_mode(0o444); // Read-only
        fs::set_permissions(&deps_path, perms).unwrap();

        let cmd: Command = Clean::builder().build().into();
        let res: Result<(), soldeer_core::SoldeerError> = async_with_vars(
            [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
            run(cmd, Verbosity::default()),
        )
        .await;

        // Should fail due to permission error
        assert!(res.is_err(), "Clean should fail with permission error");

        let mut perms = fs::metadata(&deps_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&deps_path, perms).unwrap();
    }

    #[cfg(not(unix))]
    {
        // On non-Unix systems, just run a successful clean
        let cmd: Command = Clean::builder().build().into();
        let res = async_with_vars(
            [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
            run(cmd, Verbosity::default()),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
    }
}

#[tokio::test]
async fn test_clean_partial_failure() {
    let dir = setup_project_with_dependencies("soldeer.toml").await;

    fs::remove_file(dir.join("soldeer.lock")).unwrap();

    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;

    assert!(res.is_ok(), "{res:?}");
    assert!(!dir.join("dependencies").exists());
    assert!(!dir.join("soldeer.lock").exists());
    assert!(dir.join("soldeer.toml").exists());
}

#[tokio::test]
async fn test_clean_with_soldeer_config_variations() {
    let dir = testdir!();

    let contents = r#"[soldeer]
remappings_generate = false
remappings_regenerate = true
remappings_location = "config"

[dependencies]
"@openzeppelin-contracts" = "5.0.2"
solady = "0.0.238"
"#;

    fs::write(dir.join("soldeer.toml"), contents).unwrap();

    let cmd: Command = Install::default().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    check_artifacts_exist(&dir);

    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;

    assert!(res.is_ok(), "{res:?}");
    check_clean_success(&dir, "soldeer.toml");

    // Verify custom config is preserved
    let config_content = fs::read_to_string(dir.join("soldeer.toml")).unwrap();
    assert!(config_content.contains("remappings_generate = false"));
    assert!(config_content.contains("remappings_location = \"config\""));
}

#[tokio::test]
async fn test_clean_multiple_times() {
    let dir = setup_project_with_dependencies("soldeer.toml").await;

    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    let cmd: Command = Clean::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    // Verify final state
    check_clean_success(&dir, "soldeer.toml");
}
