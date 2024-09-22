use soldeer_commands::{
    commands::{install::Install, update::Update},
    run, Command,
};
use soldeer_core::lock::read_lockfile;
use std::{fs, path::PathBuf};
use temp_env::async_with_vars;
use testdir::testdir;

async fn setup(config_filename: &str) -> PathBuf {
    // install v1.9.0 of forge-std (faking an old install)
    let dir = testdir!();
    let mut contents = r#"[dependencies]
forge-std = "1.9.0"
"#
    .to_string();
    if config_filename == "foundry.toml" {
        contents = format!(
            r#"[profile.default]

[soldeer]
remappings_location = "config"

{contents}"#
        );
    }
    fs::write(dir.join(config_filename), &contents).unwrap();
    let cmd: Command = Install::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    // change install requirement to forge-std ^1.0.0 (making the current install outdated)
    contents = contents.replace("1.9.0", "1");
    fs::write(dir.join(config_filename), &contents).unwrap();
    // update remappings accordingly
    fs::write(dir.join("remappings.txt"), "forge-std-1/=dependencies/forge-std-1.9.0/\n").unwrap();
    dir
}

#[tokio::test]
async fn test_update_existing() {
    let dir = setup("soldeer.toml").await;
    let cmd: Command = Update::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let lockfile = read_lockfile(dir.join("soldeer.lock")).unwrap();
    let version = lockfile.entries.first().unwrap().version();
    assert_ne!(version, "1.9.0");
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert_eq!(remappings, format!("forge-std-1/=dependencies/forge-std-1.9.2/\n"));
    assert!(dir.join("dependencies").join(format!("forge-std-{version}")).exists());
}

#[tokio::test]
async fn test_update_foundry_config() {
    let dir = setup("foundry.toml").await;
    let cmd: Command = Update::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let lockfile = read_lockfile(dir.join("soldeer.lock")).unwrap();
    let version = lockfile.entries.first().unwrap().version();
    assert_ne!(version, "1.9.0");
    assert!(dir.join("dependencies").join(format!("forge-std-{version}")).exists());
}

#[tokio::test]
async fn test_update_missing() {
    let dir = testdir!();
    let contents = r#"[dependencies]
forge-std = "1"
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Update::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let lockfile = read_lockfile(dir.join("soldeer.lock")).unwrap();
    let version = lockfile.entries.first().unwrap().version();
    assert!(dir.join("dependencies").join(format!("forge-std-{version}")).exists());
}

#[tokio::test]
async fn test_update_custom_remappings() {
    let dir = setup("soldeer.toml").await;
    // customize remappings before update
    fs::write(dir.join("remappings.txt"), "forge-std/=dependencies/forge-std-1.9.0/src/\n")
        .unwrap();
    let cmd: Command = Update::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let lockfile = read_lockfile(dir.join("soldeer.lock")).unwrap();
    let version = lockfile.entries.first().unwrap().version();
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert_eq!(remappings, format!("forge-std/=dependencies/forge-std-{version}/src/\n"));
}

#[tokio::test]
async fn test_update_git_main() {
    let dir = testdir!();
    // install older commit in "main" branch
    let contents = r#"[dependencies]
my-lib = { version = "branch-main", git = "https://github.com/beeb/test-repo.git" }
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let lockfile = r#"[[dependencies]]
name = "my-lib"
version = "branch-main"
git = "https://github.com/beeb/test-repo.git"
rev = "78c2f6a1a54db26bab6c3f501854a1564eb3707f"
"#;
    fs::write(dir.join("soldeer.lock"), lockfile).unwrap();
    let cmd: Command = Install::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");

    // update to latest commit in "main" branch
    let cmd: Command = Update::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let lockfile = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(
        lockfile.entries.first().unwrap().as_git().unwrap().rev,
        "d5d72fa135d28b2e8307650b3ea79115183f2406"
    );
}

#[tokio::test]
async fn test_update_git_branch() {
    let dir = testdir!();
    // install older commit in "dev" branch
    let contents = r#"[dependencies]
my-lib = { version = "branch-dev", git = "https://github.com/beeb/test-repo.git", branch = "dev" }
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let lockfile = r#"[[dependencies]]
name = "my-lib"
version = "branch-dev"
git = "https://github.com/beeb/test-repo.git"
rev = "78c2f6a1a54db26bab6c3f501854a1564eb3707f"
"#;
    fs::write(dir.join("soldeer.lock"), lockfile).unwrap();
    let cmd: Command = Install::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");

    // update to latest commit in "dev" branch
    let cmd: Command = Update::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let lockfile = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(
        lockfile.entries.first().unwrap().as_git().unwrap().rev,
        "8d903e557e8f1b6e62bde768aa456d4ddfca72c4"
    );
}
