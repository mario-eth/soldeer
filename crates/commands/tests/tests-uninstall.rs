use soldeer_commands::{
    commands::{install::Install, uninstall::Uninstall},
    run, Command,
};
use soldeer_core::{config::read_config_deps, lock::read_lockfile};
use std::{fs, path::PathBuf};
use temp_env::async_with_vars;
use testdir::testdir;

async fn setup() -> PathBuf {
    let dir = testdir!();
    let contents = r#"[dependencies]
"@openzeppelin-contracts" = "5.0.2"
solady = "0.0.238"
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Install::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    dir
}

#[tokio::test]
async fn test_uninstall_one() {
    let dir = setup().await;
    let cmd: Command = Uninstall { dependency: "solady".to_string() }.into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let deps = read_config_deps(dir.join("soldeer.toml")).unwrap();
    assert!(!deps.iter().any(|d| d.name() == "solady"));
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert!(!remappings.contains("solady"));
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert!(!lock.entries.iter().any(|d| d.name() == "solady"));
    assert!(!dir.join("dependencies").join("solady-0.0.238").exists());
}

#[tokio::test]
async fn test_uninstall_all() {
    let dir = setup().await;
    let cmd: Command = Uninstall { dependency: "solady".to_string() }.into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let cmd: Command = Uninstall { dependency: "@openzeppelin-contracts".to_string() }.into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");

    let deps = read_config_deps(dir.join("soldeer.toml")).unwrap();
    assert!(deps.is_empty());
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert_eq!(remappings, "");
    assert!(!dir.join("soldeer.lock").exists());
}
