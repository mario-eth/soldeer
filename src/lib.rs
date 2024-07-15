#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

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

use crate::auth::login;
use crate::commands::Subcommands;
use crate::config::{
    get_foundry_setup,
    read_config,
    remappings,
    Dependency,
};
use crate::dependency_downloader::{
    download_dependencies,
    unzip_dependencies,
    unzip_dependency,
};
use crate::errors::SoldeerError;
use crate::janitor::{
    cleanup_after,
    healthcheck_dependencies,
};
use crate::lock::{
    lock_check,
    write_lock,
};
use crate::utils::{
    check_dotfiles_recursive,
    get_current_working_dir,
    prompt_user_for_confirmation,
};
use crate::versioning::push_version;
use config::{
    add_to_config,
    define_config_file,
};
use dependency_downloader::download_dependency;
use janitor::cleanup_dependency;
use once_cell::sync::Lazy;
use regex::Regex;
use remote::get_dependency_url_remote;
use std::env;
use std::path::PathBuf;
use utils::get_download_tunnel;
use yansi::Paint;

pub static DEPENDENCY_DIR: Lazy<PathBuf> =
    Lazy::new(|| get_current_working_dir().join("dependencies/"));
pub static LOCK_FILE: Lazy<PathBuf> = Lazy::new(|| get_current_working_dir().join("soldeer.lock"));
pub static SOLDEER_CONFIG_FILE: Lazy<PathBuf> =
    Lazy::new(|| get_current_working_dir().join("soldeer.toml"));
pub static FOUNDRY_CONFIG_FILE: Lazy<PathBuf> =
    Lazy::new(|| get_current_working_dir().join("foundry.toml"));

#[derive(Debug)]
pub struct FOUNDRY {
    remappings: bool,
}

#[tokio::main]
pub async fn run(command: Subcommands) -> Result<(), SoldeerError> {
    match command {
        Subcommands::Install(install) => {
            if install.dependency.is_none() {
                return update().await;
            }
            println!("{}", Paint::green("🦌 Running soldeer install 🦌\n"));
            let dependency = install.dependency.unwrap();
            if !dependency.contains('~') {
                return Err(SoldeerError {
                    message: format!(
                        "Dependency {} does not specify a version.\nThe format should be [DEPENDENCY]~[VERSION]",
                        dependency
                    ),
                });
            }
            let dependency_name: String =
                dependency.split('~').collect::<Vec<&str>>()[0].to_string();
            let dependency_version: String =
                dependency.split('~').collect::<Vec<&str>>()[1].to_string();
            let dependency_url: String;

            let mut custom_url = false;
            let mut via_git = false;

            if install.remote_url.is_some() {
                custom_url = true;

                let remote_url = install.remote_url.unwrap();
                via_git = get_download_tunnel(&remote_url) == "git";
                dependency_url = remote_url.clone();
            } else {
                dependency_url =
                    match get_dependency_url_remote(&dependency_name, &dependency_version).await {
                        Ok(url) => url,
                        Err(err) => {
                            return Err(SoldeerError {
                                message: format!(
                                    "Error downloading a dependency {}~{}. Cause {}",
                                    err.name, err.version, err.cause
                                ),
                            });
                        }
                    };
            }

            // retrieve the commit in case it's sent when using git
            let mut hash = String::new();
            if via_git && install.rev.is_some() {
                hash = install.rev.unwrap();
            } else if !via_git && install.rev.is_some() {
                return Err(SoldeerError {
                    message: format!("Error unknown param {}", install.rev.unwrap()),
                });
            }

            let mut dependency = Dependency {
                name: dependency_name.clone(),
                version: dependency_version.clone(),
                url: dependency_url.clone(),
                hash,
            };

            match lock_check(&dependency, true) {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError { message: err.cause });
                }
            }

            dependency.hash = match download_dependency(&dependency).await {
                Ok(h) => h,
                Err(err) => {
                    return Err(SoldeerError {
                        message: format!(
                            "Error downloading a dependency {}~{}. Cause: {}",
                            err.name, err.version, err.cause
                        ),
                    });
                }
            };

            match write_lock(&[dependency.clone()], false) {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError {
                        message: format!("Error writing the lock: {}", err.cause),
                    });
                }
            }

            if !via_git {
                match unzip_dependency(&dependency.name, &dependency.version) {
                    Ok(_) => {}
                    Err(err_unzip) => {
                        match janitor::cleanup_dependency(
                            &dependency.name,
                            &dependency.version,
                            true,
                            false,
                        ) {
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
                        return Err(SoldeerError {
                            message: format!(
                                "Error downloading a dependency {}~{}",
                                err_unzip.name, err_unzip.version
                            ),
                        });
                    }
                }
            }

            let config_file: String = match define_config_file() {
                Ok(file) => file,

                Err(_) => {
                    match cleanup_dependency(&dependency.name, &dependency.version, true, via_git) {
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
                    }
                }
            };

            match add_to_config(&dependency, custom_url, &config_file, via_git) {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError { message: err.cause });
                }
            }

            match janitor::healthcheck_dependency(&dependency_name, &dependency_version) {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError {
                        message: format!(
                            "Error health-checking dependency {}~{}",
                            err.name, err.version
                        ),
                    });
                }
            }

            match janitor::cleanup_dependency(&dependency_name, &dependency_version, false, via_git)
            {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError {
                        message: format!(
                            "Error cleaning up dependency {}~{}",
                            err.name, err.version
                        ),
                    });
                }
            }
            // check the foundry setup, in case we have a foundry.toml, then the foundry.toml will be used for `dependencies`
            let f_setup_vec: Vec<bool> = match get_foundry_setup() {
                Ok(setup) => setup,
                Err(err) => return Err(SoldeerError { message: err.cause }),
            };
            let foundry_setup: FOUNDRY = FOUNDRY {
                remappings: f_setup_vec[0],
            };

            if foundry_setup.remappings {
                match remappings().await {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(SoldeerError { message: err.cause });
                    }
                }
            }
        }
        Subcommands::Update(_) => {
            return update().await;
        }
        Subcommands::Login(_) => {
            println!("{}", Paint::green("🦌 Running soldeer login 🦌\n"));
            match login().await {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError { message: err.cause });
                }
            }
        }
        Subcommands::Push(push) => {
            let path = push
                .path
                .unwrap_or(get_current_working_dir().to_str().unwrap().to_string());
            let path_buf = PathBuf::from(&path);
            let dry_run = push.dry_run.is_some() && push.dry_run.unwrap();
            let skip_warnings = push.skip_warnings.unwrap_or(false);

            // Check for sensitive files or directories
            if !dry_run
                && !skip_warnings
                && check_dotfiles_recursive(&path_buf)
                && !prompt_user_for_confirmation()
            {
                println!("{}", Paint::yellow("Push operation aborted by the user."));
                return Ok(());
            }

            if dry_run {
                println!(
            "{}",
            Paint::green("🦌 Running soldeer push with dry-run, a zip file will be available for inspection 🦌\n")
        );
            } else {
                println!("{}", Paint::green("🦌 Running soldeer push 🦌\n"));
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
            match push_version(
                &dependency_name,
                &dependency_version,
                PathBuf::from(&path),
                dry_run,
            )
            .await
            {
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

        Subcommands::VersionDryRun(_) => {
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            println!("{}", Paint::cyan(&format!("Current Soldeer {}", VERSION)));
        }
    }
    Ok(())
}

async fn update() -> Result<(), SoldeerError> {
    println!("{}", Paint::green("🦌 Running soldeer update 🦌\n"));

    let mut dependencies: Vec<Dependency> = match read_config(String::new()).await {
        Ok(dep) => dep,
        Err(err) => return Err(SoldeerError { message: err.cause }),
    };

    let hashes = match download_dependencies(&dependencies, true).await {
        Ok(h) => h,
        Err(err) => {
            return Err(SoldeerError {
                message: format!(
                    "Error downloading a dependency {}~{}. Cause: {}",
                    err.name, err.version, err.cause
                ),
            })
        }
    };

    for (index, dependency) in dependencies.iter_mut().enumerate() {
        dependency.hash.clone_from(&hashes[index]);
    }

    match unzip_dependencies(&dependencies) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!("Error unzipping dependency {}~{}", err.name, err.version),
            });
        }
    }

    match healthcheck_dependencies(&dependencies) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!(
                    "Error health-checking dependencies {}~{}",
                    err.name, err.version
                ),
            });
        }
    }

    match write_lock(&dependencies, true) {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!("Error writing the lock: {}", err.cause),
            });
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

    // check the foundry setup, in case we have a foundry.toml, then the foundry.toml will be used for `dependencies`
    let f_setup_vec: Vec<bool> = match get_foundry_setup() {
        Ok(f_setup) => f_setup,
        Err(err) => {
            return Err(SoldeerError { message: err.cause });
        }
    };
    let foundry_setup: FOUNDRY = FOUNDRY {
        remappings: f_setup_vec[0],
    };

    if foundry_setup.remappings {
        match remappings().await {
            Ok(_) => {}
            Err(err) => {
                return Err(SoldeerError { message: err.cause });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    use std::env::{
        self,
    };
    use std::fs::{
        create_dir_all,
        remove_dir,
        remove_dir_all,
        remove_file,
        File,
    };
    use std::io::Write;
    use std::path::Path;
    use std::{
        fs::{
            self,
        },
        path::PathBuf,
    };

    use commands::{
        Install,
        Push,
        Update,
    };
    use rand::{
        distributions::Alphanumeric,
        Rng,
    };
    use serial_test::serial;
    use zip::ZipArchive; // 0.8

    use super::*;

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

        let command = Subcommands::Install(Install {
            dependency: None,
            remote_url: None,
            rev: None,
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

        env::set_var("base_url", "https://api.soldeer.xyz");

        let command = Subcommands::Install(Install {
            dependency: None,
            remote_url: None,
            rev: None,
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
        let path_dependency = DEPENDENCY_DIR
            .join("@dep1-1")
            .join("token")
            .join("ERC20")
            .join("ERC20.sol");
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

        let command = Subcommands::Install(Install {
            dependency: None,
            remote_url: None,
            rev: None,
        });

        match run(command) {
            Ok(_) => {}
            Err(err) => {
                clean_test_env(target_config.clone());
                // can not generalize as diff systems return various dns errors
                assert!(err
                    .message
                    .contains("Error downloading a dependency will-fail~1"))
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
        let path_dependency = env::current_dir()
            .unwrap()
            .join("test")
            .join("custom_dry_run");

        if path_dependency.exists() {
            let _ = remove_dir_all(&path_dependency);
        }

        let _ = create_dir_all(&path_dependency);

        create_random_file(path_dependency.as_path(), ".txt".to_string());
        create_random_file(path_dependency.as_path(), ".txt".to_string());

        let command = Subcommands::Push(Push {
            dependency: "@test~1.1".to_string(),
            path: Some(String::from(path_dependency.to_str().unwrap())),
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
        let mut file: File = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target_file)
            .unwrap();
        if let Err(e) = write!(file, "{}", content) {
            eprintln!("Couldn't write to the config file: {}", e);
        }
    }

    fn define_config(foundry: bool) -> PathBuf {
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        let mut target = format!("foundry{}.toml", s);
        if !foundry {
            target = format!("soldeer{}.toml", s);
        }

        let path = env::current_dir().unwrap().join("test").join(target);
        env::set_var("config_file", path.clone().to_str().unwrap());
        path
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
            path: Some(test_dir.to_str().unwrap().to_string()),
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
        let test_dir = env::current_dir()
            .unwrap()
            .join("test")
            .join("test_push_skip_sensitive");

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
            path: Some(test_dir.to_str().unwrap().to_string()),
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
        let test_dir = env::current_dir()
            .unwrap()
            .join("test")
            .join("install_http");

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

        let path_dependency = DEPENDENCY_DIR
            .join("forge-std-1.9.1")
            .join("src")
            .join("Test.sol");
        assert!(path_dependency.exists());
        clean_test_env(target_config);
    }

    #[test]
    #[serial]
    fn install_dependency_custom_url_chooses_http() {
        let _ = remove_dir_all(DEPENDENCY_DIR.clone());
        let _ = remove_file(LOCK_FILE.clone());
        let test_dir = env::current_dir()
            .unwrap()
            .join("test")
            .join("install_http");

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

        let path_dependency = DEPENDENCY_DIR
            .join("forge-std-1.9.1")
            .join("src")
            .join("Test.sol");
        assert!(path_dependency.exists());
        clean_test_env(target_config);
    }

    #[test]
    #[serial]
    fn install_dependency_custom_git_httpurl_chooses_git() {
        let _ = remove_dir_all(DEPENDENCY_DIR.clone());
        let _ = remove_file(LOCK_FILE.clone());
        let test_dir = env::current_dir()
            .unwrap()
            .join("test")
            .join("install_http");

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

        let path_dependency = DEPENDENCY_DIR
            .join("forge-std-1.9.1")
            .join("src")
            .join("Test.sol");
        assert!(path_dependency.exists());
        clean_test_env(target_config);
    }

    #[test]
    #[serial]
    fn install_dependency_custom_git_giturl_chooses_git() {
        let _ = remove_dir_all(DEPENDENCY_DIR.clone());
        let _ = remove_file(LOCK_FILE.clone());
        let test_dir = env::current_dir()
            .unwrap()
            .join("test")
            .join("install_http");

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

        let path_dependency = DEPENDENCY_DIR
            .join("forge-std-1.9.1")
            .join("src")
            .join("Test.sol");
        assert!(path_dependency.exists());
        clean_test_env(target_config);
    }

    #[test]
    #[serial]
    fn install_dependency_custom_git_giturl_custom_commit() {
        let _ = remove_dir_all(DEPENDENCY_DIR.clone());
        let _ = remove_file(LOCK_FILE.clone());
        let test_dir = env::current_dir()
            .unwrap()
            .join("test")
            .join("install_http");

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

        let mut path_dependency = DEPENDENCY_DIR
            .join("forge-std-1.9.1")
            .join("src")
            .join("mocks")
            .join("MockERC721.sol");
        assert!(!path_dependency.exists()); // this should not exists at that commit
        path_dependency = DEPENDENCY_DIR
            .join("forge-std-1.9.1")
            .join("src")
            .join("Test.sol");
        assert!(path_dependency.exists()); // this should exists at that commit
        clean_test_env(target_config);
    }

    fn create_random_file(target_dir: &Path, extension: String) -> String {
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        let target = target_dir.join(format!("random{}.{}", s, extension));
        let mut file: std::fs::File = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .unwrap();
        if let Err(e) = write!(file, "this is a test file") {
            eprintln!("Couldn't write to the config file: {}", e);
        }
        String::from(target.to_str().unwrap())
    }
}
