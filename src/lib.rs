#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
pub use crate::{commands::Subcommands, errors::SoldeerError};
use cliclack::{intro, log::step, outro, outro_cancel};
use config::Paths;
use std::env;

mod auth;
pub mod commands;
mod config;
mod download;
pub mod errors;
mod install;
mod lock;
mod push;
mod registry;
mod remappings;
mod update;
mod utils;

#[tokio::main]
pub async fn run(command: Subcommands) -> Result<(), SoldeerError> {
    let paths = Paths::new()?;
    match command {
        Subcommands::Init(init) => {
            intro("🦌 Soldeer Init 🦌")?;
            step("Initialize Foundry project to use Soldeer")?;
            commands::init::init_command(&paths, init).await.map_err(|e| {
                outro_cancel("An error occurred during initialization").ok();
                e
            })?;
            outro("Done initializing!")?;
        }
        Subcommands::Install(cmd) => {
            intro("🦌 Soldeer Install 🦌")?;
            commands::install::install_command(&paths, cmd).await.map_err(|e| {
                outro_cancel("An error occurred during installation").ok();
                e
            })?;
            outro("Done installing!")?;
        }
        Subcommands::Update(cmd) => {
            intro("🦌 Soldeer Update 🦌")?;
            commands::update::update_command(&paths, cmd).await.map_err(|e| {
                outro_cancel("An error occurred during the update").ok();
                e
            })?;
            outro("Done updating!")?;
        }
        Subcommands::Uninstall(cmd) => {
            intro("🦌 Soldeer Uninstall 🦌")?;
            commands::uninstall::uninstall_command(&paths, &cmd).map_err(|e| {
                outro_cancel("An error occurred during uninstallation").ok();
                e
            })?;
            outro("Done uninstalling!")?;
        }
        Subcommands::Login(_) => {
            intro("🦌 Soldeer Login 🦌")?;
            commands::login::login_command().await.map_err(|e| {
                outro_cancel("An error occurred during login").ok();
                e
            })?;
            outro("Done logging in!")?;
        }
        Subcommands::Push(cmd) => {
            intro("🦌 Soldeer Push 🦌")?;
            commands::push::push_command(cmd).await.map_err(|e| {
                outro_cancel("An error occurred during push").ok();
                e
            })?;
            outro("Done!")?;
        }
        Subcommands::Version(_) => {
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            println!("soldeer {VERSION}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /* #[test]
        #[serial]
        fn soldeer_install_moves_to_update_no_custom_link() {
            let paths = Paths::new().unwrap();
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "@gearbox-protocol-periphery-v3" = "1.6.1"
    "@openzeppelin-contracts" = "5.0.2"
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: None,
                remote_url: None,
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let mut path_dependency = DEPENDENCY_DIR.join("@gearbox-protocol-periphery-v3-1.6.1");

            assert!(path_dependency.exists());
            path_dependency = DEPENDENCY_DIR.join("@openzeppelin-contracts-5.0.2");
            assert!(path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_install_from_git_no_rev_adds_rev_to_config() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("test~1".to_string()),
                remote_url: Some("https://gitlab.com/mario4582928/Mario.git".to_string()),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("test-1");

            assert!(path_dependency.exists());

            let expected_content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    test = { version = "1", git = "https://gitlab.com/mario4582928/Mario.git", rev = "22868f426bd4dd0e682b5ec5f9bd55507664240c" }
    "#;
            assert_eq!(expected_content, fs::read_to_string(&target_config).unwrap());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_install_from_git_with_rev_adds_rev_to_config() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("test~1".to_string()),
                remote_url: Some("https://gitlab.com/mario4582928/Mario.git".to_string()),
                rev: Some("2fd642069600f0b8da3e1897fad42b2c53c6e927".to_string()),
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("test-1");

            assert!(path_dependency.exists());

            let expected_content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    test = { version = "1", git = "https://gitlab.com/mario4582928/Mario.git", rev = "2fd642069600f0b8da3e1897fad42b2c53c6e927" }
    "#;
            assert_eq!(expected_content, fs::read_to_string(&target_config).unwrap());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_install_moves_to_update_custom_link() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "@tt" = {version = "1.6.1", url = "https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip"}
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: None,
                remote_url: None,
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("@tt-1.6.1");

            assert!(path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_update_success() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "@tt" = {version = "1.6.1", url = "https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip"}
    forge-std = { version = "1.8.1" }
    solmate = "6.7.0"
    mario = { version = "1.0", git = "https://gitlab.com/mario4582928/Mario.git", rev = "22868f426bd4dd0e682b5ec5f9bd55507664240c" }
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command =
                Subcommands::Update(Update { regenerate_remappings: false, recursive_deps: false });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("@tt-1.6.1");

            assert!(path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_update_success_with_soldeer_config() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let _ = remove_file(REMAPPINGS_FILE.as_path());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "@tt" = {version = "1.6.1", url = "https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip"}
    forge-std = { version = "1.8.1" }
    solmate = "6.7.0"
    mario = { version = "1.0", git = "https://gitlab.com/mario4582928/mario-soldeer-dependency.git", rev = "9800b422749c438fb59f289f3c2d5b1a173707ea" }

    [soldeer]
    remappings_generate = true
    remappings_regenerate = true
    remappings_location = "config"
    remappings_prefix = "@custom@"
    recursive_deps = true
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command =
                Subcommands::Update(Update { regenerate_remappings: false, recursive_deps: false });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("@tt-1.6.1");
            assert!(path_dependency.exists());

            let path_dependency = DEPENDENCY_DIR.join("forge-std-1.8.1");
            assert!(path_dependency.exists());

            let path_dependency = DEPENDENCY_DIR.join("solmate-6.7.0");
            assert!(path_dependency.exists());

            let path_dependency = DEPENDENCY_DIR.join("mario-1.0");
            assert!(path_dependency.exists());

            let expected_remappings = "@custom@@tt-1.6.1/=dependencies/@tt-1.6.1/
    @custom@forge-std-1.8.1/=dependencies/forge-std-1.8.1/
    @custom@mario-1.0/=dependencies/mario-1.0/
    @custom@solmate-6.7.0/=dependencies/solmate-6.7.0/
    ";
            assert_eq!(expected_remappings, fs::read_to_string(REMAPPINGS_FILE.as_path()).unwrap());

            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_update_with_git_and_http_success() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "@dep1" = {version = "1", url = "https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip"}
    "@dep2" = {version = "2", git = "https://gitlab.com/mario4582928/Mario.git", rev="22868f426bd4dd0e682b5ec5f9bd55507664240c" }
    "@dep3" = {version = "3.3", git = "https://gitlab.com/mario4582928/Mario.git", rev="7a0663eaf7488732f39550be655bad6694974cb3" }
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command =
                Subcommands::Update(Update { regenerate_remappings: false, recursive_deps: false });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            // http dependency should be there
            let path_dependency =
                DEPENDENCY_DIR.join("@dep1-1").join("token").join("ERC20").join("ERC20.sol");
            assert!(path_dependency.exists());

            // git dependency should be there without specified revision
            let path_dependency = DEPENDENCY_DIR.join("@dep2-2").join("JustATest3.md");
            assert!(path_dependency.exists());

            // git dependency should be there with specified revision
            let path_dependency = DEPENDENCY_DIR.join("@dep3-3.3").join("JustATest2.md");
            assert!(path_dependency.exists());

            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_update_dependencies_fails_when_one_dependency_fails() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "@gearbox-protocol-periphery-v3" = "1.6.1"
    "@openzeppelin-contracts" = "5.0.2"
    "will-not-fail" = {version = "1", url = "https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip"}
    "will-fail" = {version = "1", url="https://will-not-work"}
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: None,
                remote_url: None,
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    clean_test_env(target_config.clone());
                    // can not generalize as diff systems return various dns errors
                    assert!(err.to_string().contains("error sending request for url"))
                }
            }

            let mut path_dependency = DEPENDENCY_DIR.join("@gearbox-protocol-periphery-v3-1.6.1");

            assert!(!path_dependency.exists());
            path_dependency = DEPENDENCY_DIR.join("@openzeppelin-contracts-5.0.2");
            assert!(!path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn soldeer_push_dry_run() {
            // in case this exists we clean it before setting up the tests
            let path_dependency = env::current_dir().unwrap().join("test").join("custom_dry_run");

            if path_dependency.exists() {
                let _ = remove_dir_all(&path_dependency);
            }

            let _ = create_dir_all(&path_dependency);

            create_random_file(path_dependency.as_path(), ".txt".to_string());
            create_random_file(path_dependency.as_path(), ".txt".to_string());

            let command = Subcommands::Push(Push {
                dependency: "@test~1.1".to_string(),
                path: Some(path_dependency.clone()),
                dry_run: true,
                skip_warnings: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(PathBuf::default());
                    assert_eq!("Invalid State", "")
                }
            }

            let archive = File::open(path_dependency.join("custom_dry_run.zip"));
            let archive = ZipArchive::new(archive.unwrap());

            assert!(path_dependency.exists());
            assert_eq!(archive.unwrap().len(), 2);

            let _ = remove_dir_all(path_dependency);
        }

        #[test]
        #[serial]
        fn push_prompts_user_on_sensitive_files() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let test_dir = env::current_dir().unwrap().join("test_push_sensitive");
            let _ = remove_dir(&test_dir);
            let _ = create_dir_all(&test_dir);

            // Create a .env file in the test directory
            let env_file_path = test_dir.join(".env");
            let mut env_file = File::create(&env_file_path).unwrap();
            writeln!(env_file, "SENSITIVE_DATA=secret").unwrap();

            let command = Subcommands::Push(Push {
                dependency: "@test~1.1".to_string(),
                path: Some(test_dir.clone()),
                dry_run: false,
                skip_warnings: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(PathBuf::default());
                    assert_eq!("Invalid State", "")
                }
            }

            // Check if the .env file exists
            assert!(env_file_path.exists());

            // Clean up
            let _ = remove_file(&env_file_path);
            let _ = remove_dir_all(&test_dir);
        }

        #[test]
        #[serial]
        fn push_skips_warning_on_sensitive_files() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let test_dir = env::current_dir().unwrap().join("test").join("test_push_skip_sensitive");

            // Create test directory
            if !test_dir.exists() {
                std::fs::create_dir(&test_dir).unwrap();
            }

            // Create a .env file in the test directory
            let env_file_path = test_dir.join(".env");
            let mut env_file = File::create(&env_file_path).unwrap();
            writeln!(env_file, "SENSITIVE_DATA=secret").unwrap();

            let command = Subcommands::Push(Push {
                dependency: "@test~1.1".to_string(),
                path: Some(test_dir.clone()),
                dry_run: false,
                skip_warnings: true,
            });

            match run(command) {
                Ok(_) => {
                    println!("Push command succeeded as expected");
                }
                Err(e) => {
                    clean_test_env(PathBuf::default());

                    // Check if the error is due to not being logged in
                    if e.to_string().contains("you are not connected") {
                        println!(
                            "Test skipped: User not logged in. This test requires a logged-in state."
                        );
                        return;
                    }

                    // If it's a different error, fail the test
                    panic!("Push command failed unexpectedly: {:?}", e);
                }
            }

            // Check if the .env file still exists (it should)
            assert!(
                env_file_path.exists(),
                "The .env file should still exist after the push operation"
            );

            // Clean up
            let _ = remove_file(&env_file_path);
            let _ = remove_dir_all(&test_dir);
        }

        #[test]
        #[serial]
        fn install_dependency_remote_url() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let test_dir = env::current_dir().unwrap().join("test").join("install_http");

            // Create test directory
            if !test_dir.exists() {
                std::fs::create_dir(&test_dir).unwrap();
            }

            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("forge-std~1.9.1".to_string()),
                remote_url: Option::None,
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("forge-std-1.9.1").join("src").join("Test.sol");
            assert!(path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn install_dependency_custom_url_chooses_http() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let test_dir = env::current_dir().unwrap().join("test").join("install_http");

            // Create test directory
            if !test_dir.exists() {
                std::fs::create_dir(&test_dir).unwrap();
            }

            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("forge-std~1.9.1".to_string()),
                remote_url: Some("https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip".to_string()),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("forge-std-1.9.1").join("src").join("Test.sol");
            assert!(path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn install_dependency_custom_git_httpurl_chooses_git() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let test_dir = env::current_dir().unwrap().join("test").join("install_http");

            // Create test directory
            if !test_dir.exists() {
                std::fs::create_dir(&test_dir).unwrap();
            }

            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("forge-std~1.9.1".to_string()),
                remote_url: Some("https://github.com/foundry-rs/forge-std.git".to_string()),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("forge-std-1.9.1").join("src").join("Test.sol");
            assert!(path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn install_dependency_custom_git_giturl_chooses_git() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let test_dir = env::current_dir().unwrap().join("test").join("install_http");

            // Create test directory
            if !test_dir.exists() {
                std::fs::create_dir(&test_dir).unwrap();
            }

            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("forge-std~1.9.1".to_string()),
                remote_url: Some("https://github.com/foundry-rs/forge-std.git".to_string()),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dependency = DEPENDENCY_DIR.join("forge-std-1.9.1").join("src").join("Test.sol");
            assert!(path_dependency.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn install_dependency_custom_git_giturl_custom_commit() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let test_dir = env::current_dir().unwrap().join("test").join("install_http");

            // Create test directory
            if !test_dir.exists() {
                std::fs::create_dir(&test_dir).unwrap();
            }

            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("forge-std~1.9.1".to_string()),
                remote_url: Some("https://github.com/foundry-rs/forge-std.git".to_string()),
                rev: Some("3778c3cb8e4244cb5a1c3ef3ce1c71a3683e324a".to_string()),
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let mut path_dependency =
                DEPENDENCY_DIR.join("forge-std-1.9.1").join("src").join("mocks").join("MockERC721.sol");
            assert!(!path_dependency.exists()); // this should not exists at that commit
            path_dependency = DEPENDENCY_DIR.join("forge-std-1.9.1").join("src").join("Test.sol");
            assert!(path_dependency.exists()); // this should exists at that commit
            clean_test_env(target_config);
        }

        /* #[test]
           #[serial]
           fn soldeer_init_should_install_forge() {
               let _ = remove_dir_all(DEPENDENCY_DIR.clone());
               let _ = remove_file(LOCK_FILE.clone());

               let target_config = define_config(true);
               let content = String::new();
               write_to_config(&target_config, &content);

               unsafe {
                   // became unsafe in Rust 1.80
                   env::set_var("base_url", "https://api.soldeer.xyz");
               }

               let command = Subcommands::Init(Init { clean: false });

               match run(command) {
                   Ok(_) => {}
                   Err(err) => {
                       println!("Error occurred {:?}", err);
                       clean_test_env(target_config.clone());
                       assert_eq!("Invalid State", "")
                   }
               }

               let lock_test = get_current_working_dir().join("test").join("soldeer.lock");
               assert!(find_forge_std_path().exists());
               assert!(lock_test.exists());
               clean_test_env(target_config);
           }

           #[test]
           #[serial]
           fn soldeer_init_clean_should_delete_git_submodules() {
               let _ = remove_dir_all(DEPENDENCY_DIR.clone());
               let _ = remove_file(LOCK_FILE.clone());

               let submodules_path = get_current_working_dir().join(".gitmodules");
               let lib_path = get_current_working_dir().join("lib");
               let lock_test = get_current_working_dir().join("test").join("soldeer.lock");

               //remove it just in case
               let _ = remove_file(&submodules_path);
               let _ = remove_dir_all(&lib_path);
               let _ = remove_file(&lock_test);

               fs::write(&submodules_path, "this is a test file").unwrap();
               let _ = create_dir_all(&lib_path);

               let target_config = define_config(true);
               let content = String::new();
               write_to_config(&target_config, &content);

               unsafe {
                   // became unsafe in Rust 1.80
                   env::set_var("base_url", "https://api.soldeer.xyz");
               }

               let command = Subcommands::Init(Init { clean: true });

               match run(command) {
                   Ok(_) => {}
                   Err(err) => {
                       println!("Error occurred {:?}", err);
                       clean_test_env(target_config.clone());
                       assert_eq!("Invalid State", "")
                   }
               }

               assert!(find_forge_std_path().exists());
               assert!(lock_test.exists());
               assert!(!submodules_path.exists());
               assert!(!lib_path.exists());
               clean_test_env(target_config);
               let _ = remove_file(submodules_path);
               let _ = remove_dir_all(lib_path);
           }
        */
        #[test]
        #[serial]
        fn download_dependency_with_subdependencies_on_soldeer_success_arg_config() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("dep1~1.0".to_string()),
                remote_url: Some(
                    "https://gitlab.com/mario4582928/mario-soldeer-dependency.git".to_string(),
                ),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: true,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dir = DEPENDENCY_DIR.join("dep1-1.0");
            assert!(path_dir.exists());
            let path_dir = DEPENDENCY_DIR
                .join("dep1-1.0")
                .join("dependencies")
                .join("@openzeppelin-contracts-5.0.2")
                .join("token");
            assert!(path_dir.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn download_dependency_with_subdependencies_on_soldeer_success_soldeer_config() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]

    [soldeer]
    recursive_deps = true
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("dep1~1.0".to_string()),
                remote_url: Some(
                    "https://gitlab.com/mario4582928/mario-soldeer-dependency.git".to_string(),
                ),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dir = DEPENDENCY_DIR.join("dep1-1.0");
            assert!(path_dir.exists());
            let path_dir = DEPENDENCY_DIR
                .join("dep1-1.0")
                .join("dependencies")
                .join("@openzeppelin-contracts-5.0.2")
                .join("token");
            assert!(path_dir.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn download_dependency_with_subdependencies_on_git_success_arg_config() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("dep1~1.0".to_string()),
                remote_url: Some("https://gitlab.com/mario4582928/mario-git-submodule.git".to_string()),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: true,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dir = DEPENDENCY_DIR.join("dep1-1.0");
            assert!(path_dir.exists());
            let path_dir =
                DEPENDENCY_DIR.join("dep1-1.0").join("lib").join("mario").join("foundry.toml");
            assert!(path_dir.exists());
            clean_test_env(target_config);
        }

        #[test]
        #[serial]
        fn download_dependency_with_subdependencies_on_git_success_soldeer_config() {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            let content = r#"
    # Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

    [profile.default]
    script = "script"
    solc = "0.8.26"
    src = "src"
    test = "test"
    libs = ["dependencies"]

    [dependencies]

    [soldeer]
    recursive_deps = true
    "#;

            let target_config = define_config(true);

            write_to_config(&target_config, content);

            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("base_url", "https://api.soldeer.xyz");
            }

            let command = Subcommands::Install(Install {
                dependency: Some("dep1~1.0".to_string()),
                remote_url: Some("https://gitlab.com/mario4582928/mario-git-submodule.git".to_string()),
                rev: None,
                regenerate_remappings: false,
                recursive_deps: false,
                clean: false,
            });

            match run(command) {
                Ok(_) => {}
                Err(err) => {
                    println!("Error occurred {:?}", err);
                    clean_test_env(target_config.clone());
                    assert_eq!("Invalid State", "")
                }
            }

            let path_dir = DEPENDENCY_DIR.join("dep1-1.0");
            assert!(path_dir.exists());
            let path_dir =
                DEPENDENCY_DIR.join("dep1-1.0").join("lib").join("mario").join("foundry.toml");
            assert!(path_dir.exists());
            clean_test_env(target_config);
        }

        fn clean_test_env(target_config: PathBuf) {
            let _ = remove_dir_all(DEPENDENCY_DIR.clone());
            let _ = remove_file(LOCK_FILE.clone());
            if target_config != PathBuf::default() {
                let _ = remove_file(&target_config);
                let parent = target_config.parent();
                let lock = parent.unwrap().join("soldeer.lock");
                let _ = remove_file(lock);
            }
        }

        fn write_to_config(target_file: &PathBuf, content: &str) {
            if target_file.exists() {
                let _ = remove_file(target_file);
            }
            let mut file: File =
                fs::OpenOptions::new().create_new(true).write(true).open(target_file).unwrap();
            if let Err(e) = write!(file, "{}", content) {
                eprintln!("Couldn't write to the config file: {}", e);
            }
        }

        fn define_config(foundry: bool) -> PathBuf {
            let s: String =
                rand::thread_rng().sample_iter(&Alphanumeric).take(7).map(char::from).collect();
            let mut target = format!("foundry{}.toml", s);
            if !foundry {
                target = format!("Soldeer{}.toml", s);
            }

            let path = env::current_dir().unwrap().join("test").join(target);
            unsafe {
                // became unsafe in Rust 1.80
                env::set_var("config_file", path.to_string_lossy().to_string());
            }
            path
        }

        fn create_random_file(target_dir: &Path, extension: String) -> String {
            let s: String =
                rand::thread_rng().sample_iter(&Alphanumeric).take(7).map(char::from).collect();
            let target = target_dir.join(format!("random{}.{}", s, extension));
            let mut file: std::fs::File =
                fs::OpenOptions::new().create_new(true).write(true).open(&target).unwrap();
            if let Err(e) = write!(file, "this is a test file") {
                eprintln!("Couldn't write to the config file: {}", e);
            }
            String::from(target.to_str().unwrap())
        } */
}
