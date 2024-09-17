use soldeer_commands::{commands::install::Install, run, Command};
use soldeer_core::{config::read_config_deps, lock::read_lockfile};
use std::{fs, path::Path};
use temp_env::async_with_vars;
use testdir::testdir;

fn check_install(dir: &Path, name: &str, version_req: &str) {
    assert!(dir.join("dependencies").exists());
    let deps = read_config_deps(dir.join("soldeer.toml")).unwrap();
    assert_eq!(deps.first().unwrap().name(), name);
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert!(remappings.contains(name));
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(lock.entries.first().unwrap().name(), name);
    let version = lock.entries.first().unwrap().version();
    assert!(version.starts_with(version_req));
    assert!(dir.join("dependencies").join(format!("{name}-{version}")).exists());
}

#[tokio::test]
async fn test_install_registry_any_version() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install {
        dependency: Some("@openzeppelin-contracts~5".to_string()),
        remote_url: None,
        rev: None,
        tag: None,
        branch: None,
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
    }
    .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "@openzeppelin-contracts", "5");
}

#[tokio::test]
async fn test_install_registry_specific_version() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install {
        dependency: Some("@openzeppelin-contracts~4.9.5".to_string()),
        remote_url: None,
        rev: None,
        tag: None,
        branch: None,
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
    }
    .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "@openzeppelin-contracts", "4.9.5");
}

#[tokio::test]
async fn test_install_custom_http() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install {
        dependency: Some("mylib~1.0.0".to_string()),
        remote_url: Some("https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip".to_string()),
        rev: None,
        tag: None,
        branch: None,
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
    }
    .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "mylib", "1.0.0");
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(
        lock.entries.first().unwrap().as_http().unwrap().url,
        "https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip"
    );
}

#[tokio::test]
async fn test_install_git_main() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install {
        dependency: Some("mylib~0.1.0".to_string()),
        remote_url: Some("https://github.com/beeb/test-repo.git".to_string()),
        rev: None,
        tag: None,
        branch: None,
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
    }
    .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "mylib", "0.1.0");
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(
        lock.entries.first().unwrap().as_git().unwrap().rev,
        "d5d72fa135d28b2e8307650b3ea79115183f2406"
    );
}

#[tokio::test]
async fn test_install_git_commit() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install {
        dependency: Some("mylib~0.1.0".to_string()),
        remote_url: Some("https://github.com/beeb/test-repo.git".to_string()),
        rev: Some("78c2f6a1a54db26bab6c3f501854a1564eb3707f".to_string()),
        tag: None,
        branch: None,
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
    }
    .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "mylib", "0.1.0");
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(
        lock.entries.first().unwrap().as_git().unwrap().rev,
        "78c2f6a1a54db26bab6c3f501854a1564eb3707f"
    );
}

#[tokio::test]
async fn test_install_git_tag() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install {
        dependency: Some("mylib~0.1.0".to_string()),
        remote_url: Some("https://github.com/beeb/test-repo.git".to_string()),
        rev: None,
        tag: Some("v0.1.0".to_string()),
        branch: None,
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
    }
    .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "mylib", "0.1.0");
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(
        lock.entries.first().unwrap().as_git().unwrap().rev,
        "78c2f6a1a54db26bab6c3f501854a1564eb3707f"
    );
}

#[tokio::test]
async fn test_install_git_branch() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install {
        dependency: Some("mylib~dev".to_string()),
        remote_url: Some("https://github.com/beeb/test-repo.git".to_string()),
        rev: None,
        tag: None,
        branch: Some("dev".to_string()),
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
    }
    .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "mylib", "dev");
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(
        lock.entries.first().unwrap().as_git().unwrap().rev,
        "8d903e557e8f1b6e62bde768aa456d4ddfca72c4"
    );
}