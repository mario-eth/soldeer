#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
use crate::{
    auth::login,
    commands::Subcommands,
    config::{delete_config, read_config, remappings, Dependency},
    dependency_downloader::{
        delete_dependency_files, download_dependencies, unzip_dependencies, unzip_dependency,
    },
    errors::SoldeerError,
    janitor::{cleanup_after, healthcheck_dependencies},
    lock::{lock_check, remove_lock, write_lock},
    utils::{check_dotfiles_recursive, get_current_working_dir, prompt_user_for_confirmation},
    versioning::push_version,
};
use config::{add_to_config, get_config_path, GitDependency, HttpDependency};
use dependency_downloader::download_dependency;
use janitor::cleanup_dependency;
use lock::LockWriteMode;
use once_cell::sync::Lazy;
use regex::Regex;
use remote::get_latest_forge_std_dependency;
use std::{env, path::PathBuf};
use utils::{get_dependency_type, DependencyType};
use yansi::Paint;

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
            Paint::green("🦌 Running Soldeer init 🦌\n");
            Paint::green("Initializes a new Soldeer project in foundry\n");

            if init.clean.is_some() && init.clean.unwrap() {
                match config::remove_forge_lib() {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(SoldeerError { message: err.to_string() }); // TODO: derive
                                                                               // SoldeerError from
                                                                               // module errors
                                                                               // automatically,
                                                                               // will enable use of
                                                                               // ? operator
                    }
                }
            }

            let dependency: Dependency = match get_latest_forge_std_dependency().await {
                Ok(dep) => dep,
                Err(err) => {
                    return Err(SoldeerError { message: err.to_string() });
                }
            };
            match install_dependency(dependency).await {
                Ok(_) => {}
                Err(err) => return Err(err),
            }
        }
        Subcommands::Install(install) => {
            let Some(dependency) = install.dependency else {
                return update().await; // TODO: instead, check which dependencies do not match the
                                       // integrity checksum and install those
            };

            Paint::green("🦌 Running Soldeer install 🦌\n");
            let (dependency_name, dependency_version) =
                dependency.split_once('~').expect("dependency string should have name and version");

            let dep = match install.remote_url {
                Some(url) => match get_dependency_type(&url) {
                    DependencyType::Git => Dependency::Git(GitDependency {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        git: url,
                        rev: install.rev,
                    }),
                    DependencyType::Http => Dependency::Http(HttpDependency {
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

            match install_dependency(dep).await {
                Ok(_) => {}
                Err(err) => return Err(err),
            }
        }
        Subcommands::Update(_) => {
            return update().await;
        }
        Subcommands::Login(_) => {
            Paint::green("🦌 Running Soldeer login 🦌\n");
            match login().await {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError { message: err.cause });
                }
            }
        }
        Subcommands::Push(push) => {
            let path = push.path.unwrap_or(get_current_working_dir());
            let dry_run = push.dry_run.is_some() && push.dry_run.unwrap();
            let skip_warnings = push.skip_warnings.unwrap_or(false);

            // Check for sensitive files or directories
            if !dry_run &&
                !skip_warnings &&
                check_dotfiles_recursive(&path) &&
                !prompt_user_for_confirmation()
            {
                Paint::yellow("Push operation aborted by the user.");
                return Ok(());
            }

            if dry_run {
                println!(
            "{}",
            Paint::green("🦌 Running Soldeer push with dry-run, a zip file will be available for inspection 🦌\n")
        );
            } else {
                Paint::green("🦌 Running Soldeer push 🦌\n");
            }

            if skip_warnings {
                println!(
                    "{}",
                    Paint::yellow("Warning: Skipping sensitive file checks as requested.")
                );
            }

            let dependency_name: String =
                push.dependency.split('~').collect::<Vec<&str>>()[0].to_string();
            let dependency_version: String =
                push.dependency.split('~').collect::<Vec<&str>>()[1].to_string();
            let regex = Regex::new(r"^[@|a-z0-9][a-z0-9-]*[a-z0-9]$").unwrap();

            if !regex.is_match(&dependency_name) {
                return Err(SoldeerError{message:format!("Dependency name {} is not valid, you can use only alphanumeric characters `-` and `@`", &dependency_name)});
            }
            match push_version(&dependency_name, &dependency_version, path, dry_run).await {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError {
                        message: format!(
                            "Dependency {}~{} could not be pushed.\nCause: {}",
                            dependency_name, dependency_version, err.cause
                        ),
                    });
                }
            }
        }

        Subcommands::Uninstall(uninstall) => {
            // define the config file
            let path = match get_config_path() {
                Ok(path) => path,
                Err(_) => {
                    return Err(SoldeerError {
                        message: "Could not remove the dependency from the config file".to_string(),
                    });
                }
            };

            // delete from the config file and return the dependency
            let dependency = match delete_config(&uninstall.dependency, &path) {
                Ok(d) => d,
                Err(err) => {
                    return Err(SoldeerError { message: err.to_string() });
                }
            };

            // deleting the files
            let _ = delete_dependency_files(&dependency).is_ok();

            // removing the dependency from the lock file
            match remove_lock(&dependency) {
                Ok(d) => d,
                Err(err) => {
                    return Err(SoldeerError { message: err.cause });
                }
            };
        }

        Subcommands::VersionDryRun(_) => {
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            Paint::cyan(&format!("Current Soldeer {}", VERSION));
        }
    }
    Ok(())
}

async fn install_dependency(mut dependency: Dependency) -> Result<(), SoldeerError> {
    match lock_check(&dependency, true) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError { message: err.cause });
        }
    }

    let result = match download_dependency(&dependency).await {
        Ok(h) => h,
        Err(err) => {
            return Err(SoldeerError { message: err.to_string() });
        }
    };
    match dependency {
        Dependency::Http(ref mut dep) => {
            dep.checksum = Some(result.hash);
            dep.url = Some(result.url);
        }
        Dependency::Git(ref mut dep) => dep.rev = Some(result.hash),
    }

    match write_lock(&[dependency.clone()], LockWriteMode::Append) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError { message: format!("Error writing the lock: {}", err.cause) });
        }
    }

    if let Dependency::Http(dep) = &dependency {
        match unzip_dependency(&dep.name, &dep.version) {
            Ok(_) => {}
            Err(err_unzip) => {
                match janitor::cleanup_dependency(&dependency, true) {
                    Ok(_) => {}
                    Err(err_cleanup) => {
                        return Err(SoldeerError {
                            message: format!(
                                "Error cleaning up dependency {}~{}",
                                err_cleanup.name, err_cleanup.version
                            ),
                        })
                    }
                }
                return Err(SoldeerError { message: err_unzip.to_string() });
            }
        }
    }

    let config_file = match get_config_path() {
        Ok(file) => file,

        Err(_) => match cleanup_dependency(&dependency, true) {
            Ok(_) => {
                return Err(SoldeerError {
                    message: "Could not define the config file".to_string(),
                });
            }
            Err(_) => {
                return Err(SoldeerError {
                    message: "Could not delete dependency artifacts".to_string(),
                });
            }
        },
    };

    match add_to_config(&dependency, &config_file) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError { message: err.to_string() });
        }
    }

    match janitor::healthcheck_dependency(&dependency) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!("Error health-checking dependency {}~{}", err.name, err.version),
            });
        }
    }

    match janitor::cleanup_dependency(&dependency, false) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!("Error cleaning up dependency {}~{}", err.name, err.version),
            });
        }
    }

    // TODO: check the config to know whether we should write remappings
    remappings().await.map_err(|e| SoldeerError { message: e.to_string() })?;
    Ok(())
}

async fn update() -> Result<(), SoldeerError> {
    Paint::green("🦌 Running Soldeer update 🦌\n");

    let mut dependencies: Vec<Dependency> = match read_config(None) {
        Ok(dep) => dep,
        Err(err) => return Err(SoldeerError { message: err.to_string() }),
    };

    let results = match download_dependencies(&dependencies, true).await {
        Ok(h) => h,
        Err(err) => return Err(SoldeerError { message: err.to_string() }),
    };

    dependencies.iter_mut().zip(results.into_iter()).for_each(|(dependency, result)| {
        match dependency {
            Dependency::Http(ref mut dep) => {
                dep.checksum = Some(result.hash);
                dep.url = Some(result.url);
            }
            Dependency::Git(ref mut dep) => dep.rev = Some(result.hash),
        }
    });

    match unzip_dependencies(&dependencies) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError { message: err.to_string() });
        }
    }

    match healthcheck_dependencies(&dependencies) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!("Error health-checking dependencies {}~{}", err.name, err.version),
            });
        }
    }

    match write_lock(&dependencies, LockWriteMode::Replace) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError { message: format!("Error writing the lock: {}", err.cause) });
        }
    }

    match cleanup_after(&dependencies) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!("Error cleanup dependencies {}~{}", err.name, err.version),
            });
        }
    }

    // TODO: check the config to know whether we should write remappings
    remappings().await.map_err(|e| SoldeerError { message: e.to_string() })?;
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command =
            Subcommands::Install(Install { dependency: None, remote_url: None, rev: None });

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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command =
            Subcommands::Install(Install { dependency: None, remote_url: None, rev: None });

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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Update(Update {});

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
"@dep2" = {version = "2", git = "git@gitlab.com:mario4582928/Mario.git", rev="22868f426bd4dd0e682b5ec5f9bd55507664240c" }
"@dep3" = {version = "3.3", git = "git@gitlab.com:mario4582928/Mario.git", rev="7a0663eaf7488732f39550be655bad6694974cb3" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Update(Update {});

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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command =
            Subcommands::Install(Install { dependency: None, remote_url: None, rev: None });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                clean_test_env(target_config.clone());
                // can not generalize as diff systems return various dns errors
                assert!(err.message.contains("error sending request for url"))
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
            dry_run: Some(true),
            skip_warnings: None,
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
            dry_run: None,
            skip_warnings: None,
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
            dry_run: None,
            skip_warnings: Some(true),
        });

        match run(command) {
            Ok(_) => {
                println!("Push command succeeded as expected");
            }
            Err(e) => {
                clean_test_env(PathBuf::default());

                // Check if the error is due to not being logged in
                if e.message.contains("You are not logged in") {
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Install(Install {
            dependency: Some("forge-std~1.9.1".to_string()),
            remote_url: Option::None,
            rev: None,
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Install(Install {
            dependency: Some("forge-std~1.9.1".to_string()),
            remote_url: Some("https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip".to_string()),
            rev: None,
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Install(Install {
            dependency: Some("forge-std~1.9.1".to_string()),
            remote_url: Some("https://github.com/foundry-rs/forge-std.git".to_string()),
            rev: None,
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Install(Install {
            dependency: Some("forge-std~1.9.1".to_string()),
            remote_url: Some("git@github.com:foundry-rs/forge-std.git".to_string()),
            rev: None,
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Install(Install {
            dependency: Some("forge-std~1.9.1".to_string()),
            remote_url: Some("git@github.com:foundry-rs/forge-std.git".to_string()),
            rev: Some("3778c3cb8e4244cb5a1c3ef3ce1c71a3683e324a".to_string()),
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Init(Init { clean: None });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                println!("{:?}", err);
                clean_test_env(target_config.clone());
                assert_eq!("Invalid State", "")
            }
        }

        let path_dependency = DEPENDENCY_DIR.join("forge-std-1.9.1");
        let lock_test = get_current_working_dir().join("test").join("soldeer.lock");
        assert!(path_dependency.exists());
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

        let path_dependency = DEPENDENCY_DIR.join("forge-std-1.9.1");
        let lock_test = get_current_working_dir().join("test").join("soldeer.lock");

        //remove it just in case
        let _ = remove_file(&submodules_path);
        let _ = remove_dir_all(&lib_path);
        let _ = remove_file(&lock_test);
        let _ = remove_dir_all(&path_dependency);

        let mut file: std::fs::File =
            fs::OpenOptions::new().create_new(true).write(true).open(&submodules_path).unwrap();
        if let Err(e) = write!(file, "this is a test file") {
            eprintln!("Couldn't write to the config file: {}", e);
        }
        let _ = create_dir_all(&lib_path);

        let target_config = define_config(true);
        let content = String::new();
        write_to_config(&target_config, &content);

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Init(Init { clean: Some(true) });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                println!("{:?}", err);
                clean_test_env(target_config.clone());
                assert_eq!("Invalid State", "")
            }
        }

        assert!(path_dependency.exists());
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
        env::set_var("config_file", path.to_string_lossy().to_string());
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
}
