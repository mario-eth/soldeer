#![allow(clippy::unwrap_used)]
use mockito::Matcher;
use soldeer_commands::{Command, Verbosity, commands::install::Install, run};
use soldeer_core::{
    config::{ConfigLocation, read_config_deps},
    download::download_file,
    lock::read_lockfile,
    push::zip_file,
};
use std::{
    fs::{self},
    path::{Path, PathBuf},
};
use temp_env::async_with_vars;
use testdir::testdir;

fn check_install(dir: &Path, name: &str, version_req: &str) {
    assert!(dir.join("dependencies").exists());
    let mut config_path = dir.join("soldeer.toml");
    if !config_path.exists() {
        config_path = dir.join("foundry.toml");
    }
    let (deps, _) = read_config_deps(config_path).unwrap();
    assert_eq!(deps.first().unwrap().name(), name);
    let remappings = fs::read_to_string(dir.join("remappings.txt")).unwrap();
    assert!(remappings.contains(name));
    let lock = read_lockfile(dir.join("soldeer.lock")).unwrap();
    assert_eq!(lock.entries.first().unwrap().name(), name);
    let version = lock.entries.first().unwrap().version();
    assert!(version.starts_with(version_req));
    assert!(dir.join("dependencies").join(format!("{name}-{version}")).exists());
}

fn create_zip_monorepo(testdir: &Path) -> PathBuf {
    let root = testdir.join("monorepo");
    fs::create_dir(&root).unwrap();
    let contracts = root.join("contracts");
    fs::create_dir(&contracts).unwrap();
    let mut files = Vec::new();
    files.push(root.join("README.md"));
    fs::write(
        files.last().unwrap(),
        "Root of the repo is here, foundry project is under `contracts`",
    )
    .unwrap();
    files.push(contracts.join("foundry.toml"));
    fs::write(
        files.last().unwrap(),
        r#"[profile.default]
libs = ["dependencies"]
remappings = ["forge-std/=dependencies/forge-std-1.11.0/src/"]

[dependencies]
forge-std = "1.11.0"

[soldeer]
remappings_location = "config"
recursive_deps = true"#,
    )
    .unwrap();

    zip_file(&root, &files, "test").unwrap() // zip is inside the `monorepo` folder
}

#[tokio::test]
async fn test_install_registry_any_version() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder().dependency("@openzeppelin-contracts~5").build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "@openzeppelin-contracts", "5");
}

#[tokio::test]
async fn test_install_registry_wildcard() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder().dependency("solady~*").build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    check_install(&dir, "@openzeppelin-contracts", "4.9.5");
}

#[tokio::test]
async fn test_install_custom_http() {
    let dir = testdir!();
    fs::write(dir.join("soldeer.toml"), "[dependencies]\n").unwrap();
    let cmd: Command = Install::builder().dependency("mylib~1.0.0")
        .zip_url("https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip")
        .build()
        .into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
        .git_url("https://github.com/beeb/test-repo.git")
        .build()
        .into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
        .git_url("https://github.com/beeb/test-repo.git")
        .rev("78c2f6a1a54db26bab6c3f501854a1564eb3707f")
        .build()
        .into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
        .git_url("https://github.com/beeb/test-repo.git")
        .tag("v0.1.0")
        .build()
        .into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
        .git_url("https://github.com/beeb/test-repo.git")
        .branch("dev")
        .build()
        .into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
"@openzeppelin-contracts" = "5.1.0"
"#;
    fs::write(dir.join("foundry.toml"), contents).unwrap();
    let cmd: Command = Install::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("foundry.toml")).unwrap();
    assert!(config.contains(
        "remappings = [\"@openzeppelin-contracts-5.1.0/=dependencies/@openzeppelin-contracts-5.1.0/\"]"
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
        run(cmd.clone(), Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.assert(); // download link was called

    // second install
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.assert(); // download link was not called a second time
}

#[tokio::test]
async fn test_install_private_second_time() {
    let dir = testdir!();
    let contents = r#"[dependencies]
test-private = "0.1.0"
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
    let data = format!(
        r#"{{"data":[{{"created_at":"2025-09-28T12:36:09.526660Z","deleted":false,"id":"0440c261-8cdf-4738-9139-c4dc7b0c7f3e","internal_name":"test-private/0_1_0_28-09-2025_12:36:08_test-private.zip","private":true,"project_id":"14f419e7-2d64-49e4-86b9-b44b36627786","url":"{}/file.zip","version":"0.1.0"}}],"status":"success"}}"#,
        server.url()
    );
    server.mock("GET", "/file.zip").with_body_from_file(zip_file).create_async().await;
    server
        .mock("GET", "/api/v1/revision-cli")
        .match_query(Matcher::Any)
        .with_header("content-type", "application/json")
        .with_body(data)
        .create_async()
        .await;

    let lock = r#"[[dependencies]]
name = "test-private"
version = "0.1.0"
checksum = "94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468"
integrity = "f3c628f3e9eae4db14fe14f9ab29e49a0107c47b8ee956e4cee57b616b493fc2"
"#;
    fs::write(dir.join("soldeer.lock"), lock).unwrap();
    let cmd: Command = Install::builder().build().into();
    let res = async_with_vars(
        [
            ("SOLDEER_API_URL", Some(server.url().as_str())),
            ("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref())),
        ],
        run(cmd.clone(), Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    // second install
    let res = async_with_vars(
        [
            ("SOLDEER_API_URL", Some(server.url().as_str())),
            ("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref())),
        ],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
}

#[tokio::test]
async fn test_install_add_existing_reinstall() {
    let dir = testdir!();
    let contents = r#"[dependencies]
"@openzeppelin-contracts" = "5.0.2"
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Install::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok());

    // remove dependencies folder and lockfile
    fs::remove_dir_all(dir.join("dependencies")).unwrap();
    fs::remove_file(dir.join("soldeer.lock")).unwrap();

    // re-add the same dep, should re-install it
    let cmd: Command =
        Install::builder().dependency("@openzeppelin-contracts~5.0.2").build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok());
    let dep_path = dir.join("dependencies").join("@openzeppelin-contracts-5.0.2");
    assert!(dep_path.exists());
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    let dep_path = dir.join("dependencies").join("foo-0.1.0");
    assert!(dep_path.exists());
    let sub_dirs_path = dep_path.join("lib");
    assert!(sub_dirs_path.exists());
    assert!(sub_dirs_path.join("forge-std").join("src").exists());
}

#[tokio::test]
async fn test_install_recursive_deps_soldeer() {
    let dir = testdir!();
    // this template uses soldeer to install forge-std
    let contents = r#"[dependencies]
foo = { version = "0.1.0", git = "https://github.com/beeb/forge-template.git" }
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Install::builder().recursive_deps(true).build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    let dep_path = dir.join("dependencies").join("foo-0.1.0");
    assert!(dep_path.exists());
    let sub_dirs_path = dep_path.join("dependencies");
    assert!(sub_dirs_path.exists());
    assert!(sub_dirs_path.join("forge-std-1.9.7").join("src").exists());
}

#[tokio::test]
async fn test_install_recursive_deps_nested() {
    let dir = testdir!();
    let contents = r#"[dependencies]
"@uniswap-permit2" = { version = "1.0.0", url = "https://github.com/Uniswap/permit2/archive/cc56ad0f3439c502c246fc5cfcc3db92bb8b7219.zip" }
"#;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let cmd: Command = Install::builder().recursive_deps(true).build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    let paths = [
        "@uniswap-permit2-1.0.0/lib/forge-std/src",
        "@uniswap-permit2-1.0.0/lib/forge-gas-snapshot/dependencies/forge-std-1.9.2/src",
        "@uniswap-permit2-1.0.0/lib/openzeppelin-contracts/lib/erc4626-tests/ERC4626.test.sol",
        "@uniswap-permit2-1.0.0/lib/openzeppelin-contracts/lib/forge-std/src",
        "@uniswap-permit2-1.0.0/lib/openzeppelin-contracts/lib/halmos-cheatcodes/src",
        "@uniswap-permit2-1.0.0/lib/solmate/lib/ds-test/src",
    ];
    for path in paths {
        let dep_path = dir.join("dependencies").join(path);
        assert!(dep_path.exists());
    }
}

#[tokio::test]
async fn test_install_recursive_project_root() {
    let dir = testdir!();
    let zip_path = create_zip_monorepo(&dir);

    let contents = r#"[dependencies]
mylib = { version = "1.0.0", project_root = "contracts" }

[soldeer]
recursive_deps = true
"#;

    // serve the dependency which uses foundry in a `contracts` subfolder
    let mut server = mockito::Server::new_async().await;
    server.mock("GET", "/file.zip").with_body_from_file(zip_path).create_async().await;
    fs::write(dir.join("soldeer.toml"), contents).unwrap();
    let lock = format!(
        r#"[[dependencies]]
name = "mylib"
version = "1.0.0"
url = "{}/file.zip"
checksum = "7c38e8c60000be4724f2ad39f05b0a8f3758e9fec008ceb315a0f24b2aa98295"
integrity = "e629088e5b74df78f116a24c328a64fd002b4e42449607b6ca78f9afb799374d"
"#,
        server.url()
    );
    fs::write(dir.join("soldeer.lock"), lock).unwrap();

    let cmd: Command = Install::builder().build().into();
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd.clone(), Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    // check that we recursively installed all deps
    assert!(dir.join("dependencies/mylib-1.0.0/contracts/dependencies/forge-std-1.11.0").is_dir());
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
libs = ["dependencies"]
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
        run(cmd.clone(), Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");

    let expected = r#"[profile.default]
libs = ["dependencies"]
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
        run(cmd.clone(), Verbosity::default()),
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
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
    let res = async_with_vars(
        [("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().as_ref()))],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    let config = fs::read_to_string(dir.join("soldeer.toml")).unwrap();
    let content = r#"[soldeer]

[dependencies]
"@openzeppelin-contracts" = "5"
"#;
    assert_eq!(config, content);
}
