#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
use crate::{
    auth::login,
    config::{delete_config, read_config_deps, remappings_txt, Dependency},
    dependency_downloader::{
        delete_dependency_files, download_dependencies, unzip_dependencies, unzip_dependency,
    },
    janitor::{cleanup_after, healthcheck_dependencies},
    lock::{lock_check, remove_lock, write_lock},
    utils::{check_dotfiles_recursive, get_current_working_dir, prompt_user_for_confirmation},
    versioning::push_version,
};
pub use crate::{commands::Subcommands, errors::SoldeerError};
use config::{
    add_to_config, get_config_path, read_soldeer_config, remappings_foundry, GitDependency,
    HttpDependency, RemappingsAction, RemappingsLocation,
};
use dependency_downloader::download_dependency;
use janitor::cleanup_dependency;
use lock::LockWriteMode;
use once_cell::sync::Lazy;
use remote::get_latest_forge_std_dependency;
use std::{env, path::PathBuf};
use utils::{get_url_type, UrlType};
use versioning::validate_name;
use yansi::Paint as _;

mod auth;
pub mod commands;
mod config;
mod dependency_downloader;
pub mod errors;
mod janitor;
mod lock;
mod remote;
mod utils;
mod versioning;

pub static DEPENDENCY_DIR: Lazy<PathBuf> =
    Lazy::new(|| get_current_working_dir().join("dependencies/"));
pub static LOCK_FILE: Lazy<PathBuf> = Lazy::new(|| get_current_working_dir().join("soldeer.lock"));
pub static SOLDEER_CONFIG_FILE: Lazy<PathBuf> =
    Lazy::new(|| get_current_working_dir().join("soldeer.toml"));
pub static FOUNDRY_CONFIG_FILE: Lazy<PathBuf> =
    Lazy::new(|| get_current_working_dir().join("foundry.toml"));

#[tokio::main]
pub async fn run(command: Subcommands) -> Result<(), SoldeerError> {
    match command {
        Subcommands::Init(init) => {
            println!("{}", "🦌 Running Soldeer init 🦌".green());
            println!("{}", "Initializes a new Soldeer project in foundry".green());

            if init.clean {
                config::remove_forge_lib()?;
            }

            let dependency: Dependency = get_latest_forge_std_dependency().await.map_err(|e| {
                SoldeerError::DownloadError { dep: "forge-std".to_string(), source: e }
            })?;
            install_dependency(dependency, true, false).await?;
        }
        Subcommands::Install(install) => {
            let regenerate_remappings = install.regenerate_remappings;
            let Some(dependency) = install.dependency else {
                return update(regenerate_remappings, install.recursive_deps).await; // TODO: instead, check which
                                                                                    // dependencies
                                                                                    // do
                                                                                    // not match the
                                                                                    // integrity checksum and install those
            };

            println!("{}", "🦌 Running Soldeer install 🦌".green());
            let (dependency_name, dependency_version) =
                dependency.split_once('~').expect("dependency string should have name and version");

            let dep = match install.remote_url {
                Some(url) => match get_url_type(&url) {
                    UrlType::Git => Dependency::Git(GitDependency {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        git: url,
                        rev: install.rev,
                    }),
                    UrlType::Http => Dependency::Http(HttpDependency {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        url: Some(url),
                        checksum: None,
                    }),
                },
                None => Dependency::Http(HttpDependency {
                    name: dependency_name.to_string(),
                    version: dependency_version.to_string(),
                    url: None,
                    checksum: None,
                }),
            };

            install_dependency(dep, regenerate_remappings, install.recursive_deps).await?;
        }
        Subcommands::Update(update_args) => {
            return update(update_args.regenerate_remappings, update_args.recursive_deps).await;
        }
        Subcommands::Login(_) => {
            println!("{}", "🦌 Running Soldeer login 🦌".green());
            login().await?;
        }
        Subcommands::Push(push) => {
            let path = push.path.unwrap_or(get_current_working_dir());
            let dry_run = push.dry_run;
            let skip_warnings = push.skip_warnings;

            // Check for sensitive files or directories
            if !dry_run &&
                !skip_warnings &&
                check_dotfiles_recursive(&path) &&
                !prompt_user_for_confirmation()
            {
                println!("{}", "Push operation aborted by the user.".yellow());
                return Ok(());
            }

            if dry_run {
                println!(
                    "{}",
                    "🦌 Running Soldeer push with dry-run, a zip file will be available for inspection 🦌".green()
                );
            } else {
                println!("{}", "🦌 Running Soldeer push 🦌".green());
            }

            if skip_warnings {
                println!("{}", "Warning: Skipping sensitive file checks as requested.".yellow());
            }

            let (dependency_name, dependency_version) = push
                .dependency
                .split_once('~')
                .expect("dependency string should have name and version");

            validate_name(dependency_name)?;

            push_version(dependency_name, dependency_version, path, dry_run).await?;
        }

        Subcommands::Uninstall(uninstall) => {
            // define the config file
            let config_path = get_config_path()?;

            // delete from the config file and return the dependency
            let dependency = delete_config(&uninstall.dependency, &config_path)?;

            // deleting the files
            delete_dependency_files(&dependency).map_err(|e| SoldeerError::DownloadError {
                dep: dependency.to_string(),
                source: e,
            })?;

            // removing the dependency from the lock file
            remove_lock(&dependency)?;

            let config = read_soldeer_config(Some(config_path.clone()))?;

            if config.remappings_generate {
                if config_path.to_string_lossy().contains("foundry.toml") {
                    match config.remappings_location {
                        RemappingsLocation::Txt => {
                            remappings_txt(
                                &RemappingsAction::Remove(dependency),
                                &config_path,
                                &config,
                            )
                            .await?
                        }
                        RemappingsLocation::Config => {
                            remappings_foundry(
                                &RemappingsAction::Remove(dependency),
                                &config_path,
                                &config,
                            )
                            .await?
                        }
                    }
                } else {
                    remappings_txt(&RemappingsAction::Remove(dependency), &config_path, &config)
                        .await?;
                }
            }
        }

        Subcommands::Version(_) => {
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            println!("{}", format!("Current Soldeer {}", VERSION).cyan());
        }
    }
    Ok(())
}

async fn install_dependency(
    mut dependency: Dependency,
    regenerate_remappings: bool,
    recursive_deps: bool,
) -> Result<(), SoldeerError> {
    lock_check(&dependency, true)?;

    let config_path = match get_config_path() {
        Ok(file) => file,
        Err(e) => {
            cleanup_dependency(&dependency, true)?;
            return Err(e.into());
        }
    };
    add_to_config(&dependency, &config_path)?;

    let mut config = read_soldeer_config(Some(config_path.clone()))?;
    if regenerate_remappings {
        config.remappings_regenerate = regenerate_remappings;
    }

    if recursive_deps {
        config.recursive_deps = recursive_deps;
    }

    let result = download_dependency(&dependency, false, config.recursive_deps)
        .await
        .map_err(|e| SoldeerError::DownloadError { dep: dependency.to_string(), source: e })?;
    match dependency {
        Dependency::Http(ref mut dep) => {
            dep.checksum = Some(result.hash);
            dep.url = Some(result.url);
        }
        Dependency::Git(ref mut dep) => dep.rev = Some(result.hash),
    }

    write_lock(&[dependency.clone()], LockWriteMode::Append)?;

    if let Dependency::Http(dep) = &dependency {
        if let Err(e) = unzip_dependency(dep) {
            cleanup_dependency(&dependency, true)?;
            return Err(SoldeerError::DownloadError { dep: dependency.to_string(), source: e });
        }
    }

    janitor::healthcheck_dependency(&dependency)?;

    janitor::cleanup_dependency(&dependency, false)?;

    if config.remappings_generate {
        if config_path.to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(&RemappingsAction::Add(dependency), &config_path, &config)
                        .await?
                }
                RemappingsLocation::Config => {
                    remappings_foundry(&RemappingsAction::Add(dependency), &config_path, &config)
                        .await?
                }
            }
        } else {
            remappings_txt(&RemappingsAction::Add(dependency), &config_path, &config).await?;
        }
    }

    Ok(())
}

async fn update(regenerate_remappings: bool, recursive_deps: bool) -> Result<(), SoldeerError> {
    println!("{}", "🦌 Running Soldeer update 🦌".green());

    let config_path = get_config_path()?;
    let mut config = read_soldeer_config(Some(config_path.clone()))?;
    if regenerate_remappings {
        config.remappings_regenerate = regenerate_remappings;
    }

    if recursive_deps {
        config.recursive_deps = recursive_deps;
    }

    let mut dependencies: Vec<Dependency> = read_config_deps(None)?;

    let results = download_dependencies(&dependencies, true, config.recursive_deps)
        .await
        .map_err(|e| SoldeerError::DownloadError { dep: String::new(), source: e })?;

    dependencies.iter_mut().zip(results.into_iter()).for_each(|(dependency, result)| {
        match dependency {
            Dependency::Http(ref mut dep) => {
                dep.checksum = Some(result.hash);
                dep.url = Some(result.url);
            }
            Dependency::Git(ref mut dep) => dep.rev = Some(result.hash),
        }
    });

    unzip_dependencies(&dependencies)
        .map_err(|e| SoldeerError::DownloadError { dep: String::new(), source: e })?;

    healthcheck_dependencies(&dependencies)?;

    write_lock(&dependencies, LockWriteMode::Replace)?;

    cleanup_after(&dependencies)?;

    if config.remappings_generate {
        if config_path.to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(&RemappingsAction::None, &config_path, &config).await?
                }
                RemappingsLocation::Config => {
                    remappings_foundry(&RemappingsAction::None, &config_path, &config).await?
                }
            }
        } else {
            remappings_txt(&RemappingsAction::None, &config_path, &config).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commands::{Init, Install, Push, Update};
    use rand::{distributions::Alphanumeric, Rng};
    use serial_test::serial;
    use std::{
        env::{self},
        fs::{
            create_dir_all, remove_dir, remove_dir_all, remove_file, File, {self},
        },
        io::Write,
        path::{Path, PathBuf},
    };
    use zip::ZipArchive; // 0.8

    #[test]
    #[serial]
    fn soldeer_install_moves_to_update_no_custom_link() {
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
        });

        match run(command) {
            Ok(_) => {}
            Err(_) => {
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
        });

        match run(command) {
            Ok(_) => {}
            Err(_) => {
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
            Err(_) => {
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
                println!("Err {:?}", err);
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
            Err(_) => {
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
            Err(_) => {
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
        });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                println!("Err {}", err);
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
            recursive_deps: false
        });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                println!("Err {}", err);
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
        });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                println!("Err {}", err);
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
        });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                println!("Err {}", err);
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
        });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                println!("Err {}", err);
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

    #[test]
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
                println!("{:?}", err);
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

        let mut file: std::fs::File =
            fs::OpenOptions::new().create_new(true).write(true).open(&submodules_path).unwrap();
        if let Err(e) = write!(file, "this is a test file") {
            eprintln!("Couldn't write to the config file: {}", e);
        }
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
                println!("{:?}", err);
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
    }

    fn find_forge_std_path() -> PathBuf {
        for entry in fs::read_dir(DEPENDENCY_DIR.clone()).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() &&
                path.file_name().unwrap().to_string_lossy().starts_with("forge-std-")
            {
                return path;
            }
        }
        panic!("could not find forge-std folder in dependency dir");
    }
}
