use soldeer_commands::{commands::init::Init, run, Command, ConfigLocation};
use soldeer_core::{config::read_config_deps, lock::read_lockfile, utils::run_git_command};
use std::fs;
use temp_env::async_with_vars;
use testdir::testdir;

#[tokio::test]
async fn test_init_clean() {
    let dir = testdir!();
    run_git_command(
        ["clone", "--recursive", "https://github.com/foundry-rs/forge-template.git", "."],
        Some(&dir),
    )
    .await
    .unwrap();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command =
        Init::builder().clean(true).config_location(ConfigLocation::Soldeer).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    assert!(!dir.join("lib").exists());
    assert!(!dir.join(".gitmodules").exists());
    assert!(dir.join("dependencies").exists());
    let deps = read_config_deps(dir.join("soldeer.toml")).unwrap();
    assert_eq!(deps.first().unwrap().name(), "forge-std");
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(lock.entries.first().unwrap().name(), "forge-std");
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert!(remappings.contains("forge-std"));
    let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/dependencies"));
    let foundry_config = fs::read_to_string(dir.join("foundry.toml")).unwrap();
    assert!(foundry_config.contains("libs = [\"dependencies\"]"));
}

#[tokio::test]
async fn test_init_no_clean() {
    let dir = testdir!();
    run_git_command(
        ["clone", "--recursive", "https://github.com/foundry-rs/forge-template.git", "."],
        Some(&dir),
    )
    .await
    .unwrap();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Init::builder().config_location(ConfigLocation::Soldeer).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    assert!(dir.join("lib").exists());
    assert!(dir.join(".gitmodules").exists());
    assert!(dir.join("dependencies").exists());
    let deps = read_config_deps(dir.join("soldeer.toml")).unwrap();
    assert_eq!(deps.first().unwrap().name(), "forge-std");
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(lock.entries.first().unwrap().name(), "forge-std");
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert!(remappings.contains("forge-std"));
    let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
    assert!(gitignore.contains("/dependencies"));
    let foundry_config = fs::read_to_string(dir.join("foundry.toml")).unwrap();
    assert!(foundry_config.contains("libs = [\"dependencies\"]"));
}

#[tokio::test]
async fn test_init_no_remappings() {
    let dir = testdir!();
    run_git_command(
        ["clone", "--recursive", "https://github.com/foundry-rs/forge-template.git", "."],
        Some(&dir),
    )
    .await
    .unwrap();
    let contents = r"[soldeer]
remappings_generate = false

[dependencies]
";
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command =
        Init::builder().clean(true).config_location(ConfigLocation::Soldeer).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    assert!(!dir.join("remappings.txt").exists());
}

#[tokio::test]
async fn test_init_no_gitignore() {
    let dir = testdir!();
    run_git_command(
        ["clone", "--recursive", "https://github.com/foundry-rs/forge-template.git", "."],
        Some(&dir),
    )
    .await
    .unwrap();
    fs::remove_file(dir.join(".gitignore")).unwrap();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command =
        Init::builder().clean(true).config_location(ConfigLocation::Soldeer).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    assert!(!dir.join(".gitignore").exists());
}

#[tokio::test]
async fn test_init_select_foundry_location() {
    let dir = testdir!();

    let cmd: Command =
        Init::builder().clean(true).config_location(ConfigLocation::Foundry).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");

    let config_path = dir.join("foundry.toml");
    assert!(config_path.exists());

    let contents = r#"[profile.default]
src = "src"
out = "out"
libs = ["dependencies"]

[dependencies]
forge-std = "1.9.4"

# See more config options https://github.com/foundry-rs/foundry/blob/master/crates/config/README.md#all-options
"#;
    assert_eq!(fs::read_to_string(config_path).unwrap(), contents);
}

#[tokio::test]
async fn test_init_select_soldeer_location() {
    let dir = testdir!();

    let cmd: Command =
        Init::builder().clean(true).config_location(ConfigLocation::Soldeer).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");

    let config_path = dir.join("soldeer.toml");
    assert!(config_path.exists());

    let contents = r#"[dependencies]
forge-std = "1.9.4"
"#;
    assert_eq!(fs::read_to_string(config_path).unwrap(), contents);
}
