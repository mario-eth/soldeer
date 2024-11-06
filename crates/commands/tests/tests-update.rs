use soldeer_commands::{
    commands::{install::Install, update::Update},
    run, Command, ConfigLocation,
};
use soldeer_core::lock::read_lockfile;
use std::{fs, path::PathBuf};
use temp_env::async_with_vars;
use testdir::testdir;

#[allow(clippy::unwrap_used)]
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
    assert_eq!(remappings, format!("forge-std-1/=dependencies/forge-std-{version}/\n"));
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

#[tokio::test]
async fn test_update_foundry_config_multi_dep() {
    let dir = testdir!();

    let contents = r#"[profile.default]

[dependencies]
"@tt" = {version = "1.6.1", url = "https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip"}
forge-std = { version = "1.8.1" }
solmate = "6.7.0"
mario = { version = "1.0", git = "https://gitlab.com/mario4582928/Mario.git", rev = "22868f426bd4dd0e682b5ec5f9bd55507664240c" }
mario-custom-tag = { version = "1.0", git = "https://gitlab.com/mario4582928/Mario.git", tag = "custom-tag" }
mario-custom-branch = { version = "1.0", git = "https://gitlab.com/mario4582928/Mario.git", tag = "custom-branch" }

[soldeer]
remappings_location = "config"
"#;

    fs::write(dir.join("foundry.toml"), contents).unwrap();

    let cmd: Command = Update::default().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let deps = dir.join("dependencies");
    assert!(deps.join("@tt-1.6.1").exists());
    assert!(deps.join("forge-std-1.8.1").exists());
    assert!(deps.join("solmate-6.7.0").exists());
    assert!(deps.join("mario-1.0").exists());
    assert!(deps.join("mario-custom-tag-1.0").exists());
    assert!(deps.join("mario-custom-branch-1.0").exists());
}

#[tokio::test]
async fn test_install_new_foundry_no_foundry_toml() {
    let dir = testdir!();

    let cmd: Command = Update::builder().config_location(ConfigLocation::Foundry).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("foundry.toml")).unwrap();
    let expected = r#"[profile.default]
src = "src"
out = "out"
libs = ["dependencies"]

[dependencies]

# See more config options https://github.com/foundry-rs/foundry/blob/master/crates/config/README.md#all-options
"#;
    assert_eq!(config, expected);
}

#[tokio::test]
async fn test_install_new_soldeer_no_soldeer_toml() {
    let dir = testdir!();

    let cmd: Command = Update::builder().config_location(ConfigLocation::Soldeer).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("soldeer.toml")).unwrap();
    let content = "[dependencies]\n";
    assert_eq!(config, content);
}
