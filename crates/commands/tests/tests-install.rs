use soldeer_commands::{commands::install::Install, run, Command, ConfigLocation};
use soldeer_core::{config::read_config_deps, download::download_file, lock::read_lockfile};
use std::{
    fs::{self},
    path::Path,
};
use temp_env::async_with_vars;
use testdir::testdir;

#[allow(clippy::unwrap_used)]
fn check_install(dir: &Path, name: &str, version_req: &str) {
    assert!(dir.join("dependencies").exists());
    let mut config_path = dir.join("soldeer.toml");
    if !config_path.exists() {
        config_path = dir.join("foundry.toml");
    }
    let deps = read_config_deps(config_path).unwrap();
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
    let cmd: Command = Install::builder().dependency("@openzeppelin-contracts~5").build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "@openzeppelin-contracts", "5");
}

#[tokio::test]
async fn test_install_registry_wildcard() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder().dependency("solady~*").build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "solady", "");
}

#[tokio::test]
async fn test_install_registry_specific_version() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command =
        Install::builder().dependency("@openzeppelin-contracts~4.9.5").build().into();
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
    let cmd: Command = Install::builder().dependency("mylib~1.0.0").remote_url("https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip").build().into();
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
    assert!(&dir.join("dependencies").join("mylib-1.0.0").join("README.md").exists());
}

#[tokio::test]
async fn test_install_git_main() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder()
        .dependency("mylib~0.1.0")
        .remote_url("https://github.com/beeb/test-repo.git")
        .build()
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
    assert!(&dir.join("dependencies").join("mylib-0.1.0").join("foo.txt").exists());
}

#[tokio::test]
async fn test_install_git_commit() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder()
        .dependency("mylib~0.1.0")
        .remote_url("https://github.com/beeb/test-repo.git")
        .rev("78c2f6a1a54db26bab6c3f501854a1564eb3707f")
        .build()
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
    assert!(!&dir.join("dependencies").join("mylib-1.0.0").join("foo.txt").exists());
}

#[tokio::test]
async fn test_install_git_tag() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder()
        .dependency("mylib~0.1.0")
        .remote_url("https://github.com/beeb/test-repo.git")
        .tag("v0.1.0")
        .build()
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
    assert!(!&dir.join("dependencies").join("mylib-1.0.0").join("foo.txt").exists());
}

#[tokio::test]
async fn test_install_git_branch() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder()
        .dependency("mylib~dev")
        .remote_url("https://github.com/beeb/test-repo.git")
        .branch("dev")
        .build()
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
    assert!(!&dir.join("dependencies").join("mylib-1.0.0").join("test.txt").exists());
}

#[tokio::test]
async fn test_install_foundry_config() {
    let dir = testdir!();
    fs::write(dir.join("foundry.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder().dependency("@openzeppelin-contracts~5").build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "@openzeppelin-contracts", "5");
}

#[tokio::test]
async fn test_install_foundry_remappings() {
    let dir = testdir!();
    let contents = r#"[profile.default]

[soldeer]
remappings_location = "config"

[dependencies]
"@openzeppelin-contracts" = "5"
"#;
    fs::write(dir.join("foundry.toml"), contents).unwrap();
    let cmd: Command = Install::builder().build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("foundry.toml")).unwrap();
    assert!(config.contains(
        "remappings = [\"@openzeppelin-contracts-5/=dependencies/@openzeppelin-contracts-5.1.0/\"]"
    ));
}

#[tokio::test]
async fn test_install_missing_no_lock() {
    let dir = testdir!();
    let contents = r#"[dependencies]
"@openzeppelin-contracts" = "5.0.2"
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Install::builder().build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "@openzeppelin-contracts", "5.0.2");
}

#[tokio::test]
async fn test_install_missing_with_lock() {
    let dir = testdir!();
    let contents = r#"[dependencies]
mylib = "1.1"
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let lock = r#"[[dependencies]]
name = "mylib"
version = "1.1.0"
url = "https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip"
checksum = "94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468"
integrity = "f3c628f3e9eae4db14fe14f9ab29e49a0107c47b8ee956e4cee57b616b493fc2"
"#;
    fs::write(dir.join("soldeer.lock"), lock).unwrap();
    let cmd: Command = Install::builder().build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "mylib", "1.1");
}

#[tokio::test]
async fn test_install_second_time() {
    let dir = testdir!();
    let contents = r#"[dependencies]
mylib = "1.1"
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();

    // get zip file locally for mock
    let zip_file = download_file(
        "https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip",
        &dir,
        "tmp",
    )
    .await
    .unwrap();

    // serve the file with mock server
    let mut server = mockito::Server::new_async().await;
    let mock = server.mock("GET", "/file.zip").with_body_from_file(zip_file).create_async().await;
    let mock = mock.expect(1); // download link should be called exactly once

    let lock = format!(
        r#"[[dependencies]]
name = "mylib"
version = "1.1.0"
url = "{}/file.zip"
checksum = "94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468"
integrity = "f3c628f3e9eae4db14fe14f9ab29e49a0107c47b8ee956e4cee57b616b493fc2"
"#,
        server.url()
    );
    fs::write(dir.join("soldeer.lock"), lock).unwrap();
    let cmd: Command = Install::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd.clone()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.assert(); // download link was called

    // second install
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    mock.assert(); // download link was not called a second time
}

#[tokio::test]
async fn test_install_clean() {
    let dir = testdir!();
    let contents = r#"[dependencies]
"@openzeppelin-contracts" = "5.0.2"
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let test_path = dir.join("dependencies").join("foo");
    fs::create_dir_all(&test_path).unwrap();
    fs::write(test_path.join("foo.txt"), "test").unwrap();
    let cmd: Command = Install::builder().clean(true).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    assert!(!test_path.exists());
}

#[tokio::test]
async fn test_install_recursive_deps() {
    let dir = testdir!();
    let contents = r#"[dependencies]
foo = { version = "0.1.0", git = "https://github.com/foundry-rs/forge-template.git" }
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Install::builder().recursive_deps(true).build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let dep_path = dir.join("dependencies").join("foo-0.1.0");
    assert!(dep_path.exists());
    let sub_dirs_path = dep_path.join("lib");
    assert!(sub_dirs_path.exists());
    assert!(sub_dirs_path.join("forge-std").join("src").exists());
}

#[tokio::test]
async fn test_install_regenerate_remappings() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    fs::write(dir.join("remappings.txt"), "foo=bar").unwrap();
    let cmd: Command = Install::builder()
        .dependency("@openzeppelin-contracts~5")
        .regenerate_remappings(true)
        .build()
        .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert!(!remappings.contains("foo=bar"));
    assert!(remappings.contains("@openzeppelin-contracts"));
}

#[tokio::test]
async fn test_add_remappings() {
    let dir = testdir!();

    let contents = r#"[profile.default]
src = "src"
out = "out"
libs = ["dependencies"]

# See more config options https://github.com/foundry-rs/foundry/blob/master/crates/config/README.md#all-options

[soldeer]
remappings_generate = true
remappings_prefix = "@custom-f@"
remappings_location = "config"
remappings_regenerate = true

[dependencies]
"#;

    fs::write(dir.join("foundry.toml"), contents).unwrap();
    let cmd: Command = Install::builder().dependency("forge-std~1.8.1").build().into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");

    let updated_contents = r#"[profile.default]
src = "src"
out = "out"
libs = ["dependencies"]
remappings = ["@custom-f@forge-std-1.8.1/=dependencies/forge-std-1.8.1/"]

# See more config options https://github.com/foundry-rs/foundry/blob/master/crates/config/README.md#all-options

[soldeer]
remappings_generate = true
remappings_prefix = "@custom-f@"
remappings_location = "config"
remappings_regenerate = true

[dependencies]
forge-std = "1.8.1"
"#;
    assert_eq!(updated_contents, fs::read_to_string(dir.join("foundry.toml")).unwrap());
}

#[tokio::test]
async fn test_modifying_remappings_prefix_config() {
    let dir = testdir!();

    let contents = r#"[profile.default]
remappings = ["@custom-f@forge-std-1.8.1/=dependencies/forge-std-1.8.1/"]

[soldeer]
remappings_prefix = "!custom-f!"
remappings_regenerate = true
remappings_location = "config"

[dependencies]
"#;

    fs::write(dir.join("foundry.toml"), contents).unwrap();
    let cmd: Command = Install::builder().dependency("forge-std~1.8.1").build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd.clone()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    let expected = r#"[profile.default]
remappings = ["!custom-f!forge-std-1.8.1/=dependencies/forge-std-1.8.1/"]

[soldeer]
remappings_prefix = "!custom-f!"
remappings_regenerate = true
remappings_location = "config"

[dependencies]
forge-std = "1.8.1"
"#;

    assert_eq!(expected, fs::read_to_string(dir.join("foundry.toml")).unwrap());
}

#[tokio::test]
async fn test_modifying_remappings_prefix_txt() {
    let dir = testdir!();

    let contents = r#"[profile.default]

[soldeer]
remappings_prefix = "!custom-f!"
remappings_regenerate = true

[dependencies]
"#;
    fs::write(
        dir.join("remappings.txt"),
        "@custom-f@forge-std-1.8.1/=dependencies/forge-std-1.8.1/",
    )
    .unwrap();
    fs::write(dir.join("foundry.toml"), contents).unwrap();
    let cmd: Command = Install::builder().dependency("forge-std~1.8.1").build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd.clone()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    let updated_contents = r#"!custom-f!forge-std-1.8.1/=dependencies/forge-std-1.8.1/
"#;

    assert_eq!(updated_contents, fs::read_to_string(dir.join("remappings.txt")).unwrap());
}

#[tokio::test]
async fn test_install_new_foundry_no_dependency_tag() {
    let dir = testdir!();
    let contents = r#"[profile.default]
libs = ["lib"]
"#;
    fs::write(dir.join("foundry.toml"), contents).unwrap();
    let cmd: Command = Install::builder()
        .dependency("@openzeppelin-contracts~5")
        .config_location(ConfigLocation::Foundry)
        .build()
        .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("foundry.toml")).unwrap();
    let content = r#"[profile.default]
libs = ["lib", "dependencies"]

[dependencies]
"@openzeppelin-contracts" = "5"
"#;
    assert_eq!(config, content);
}

#[tokio::test]
async fn test_install_new_soldeer_no_soldeer_toml() {
    let dir = testdir!();

    let cmd: Command = Install::builder()
        .dependency("@openzeppelin-contracts~5")
        .config_location(ConfigLocation::Soldeer)
        .build()
        .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("soldeer.toml")).unwrap();
    let content = r#"[dependencies]
"@openzeppelin-contracts" = "5"
"#;
    assert_eq!(config, content);
}

#[tokio::test]
async fn test_install_new_soldeer_no_dependency_tag() {
    let dir = testdir!();
    let contents = r#"[soldeer]
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Install::builder()
        .dependency("@openzeppelin-contracts~5")
        .config_location(ConfigLocation::Soldeer)
        .build()
        .into();
    let res =
        async_with_vars([("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))], run(cmd))
            .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("soldeer.toml")).unwrap();
    let content = r#"[soldeer]

[dependencies]
"@openzeppelin-contracts" = "5"
"#;
    assert_eq!(config, content);
}
