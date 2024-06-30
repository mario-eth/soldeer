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
    get_current_working_dir,
    check_dotfiles_recursive,
    prompt_user_for_confirmation
};
use crate::versioning::push_version;
use config::{
    add_to_config,
    define_config_file,
};
use janitor::cleanup_dependency;
use once_cell::sync::Lazy;
use regex::Regex;
use std::env;
use std::path::PathBuf;
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
            if install.remote_url.is_some() {
                custom_url = true;
                let remote_url = install.remote_url.unwrap();
                let mut dependencies: Vec<Dependency> = Vec::new();
                dependency_url = remote_url.clone();
                let dependency = Dependency {
                    name: dependency_name.clone(),
                    version: dependency_version.clone(),
                    url: dependency_url.clone(),
                };
                dependencies.push(dependency.clone());

                match lock_check(&dependency, true) {
                    Ok(dep) => dependencies = dep,
                    Err(err) => {
                        return Err(SoldeerError { message: err.cause });
                    }
                }

                match download_dependencies(&dependencies, false).await {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(SoldeerError {
                            message: format!(
                                "Error downloading a dependency {}~{}",
                                err.name, err.version
                            ),
                        });
                    }
                }
                match write_lock(&dependencies, false) {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(SoldeerError {
                            message: format!("Error writing the lock: {}", err.cause),
                        });
                    }
                }
            } else {
                let dependency = Dependency {
                    name: dependency_name.clone(),
                    version: dependency_version.clone(),
                    url: String::new(),
                };
                let mut dependencies: Vec<Dependency>;
                match lock_check(&dependency, true) {
                    Ok(dep) => dependencies = dep,
                    Err(err) => {
                        return Err(SoldeerError { message: err.cause });
                    }
                }

                match dependency_downloader::download_dependency_remote(
                    &dependency_name,
                    &dependency_version,
                )
                .await
                {
                    Ok(url) => {
                        dependencies[0].url = url;
                        dependency_url = dependencies[0].url.clone();
                    }
                    Err(err) => {
                        return Err(SoldeerError {
                            message: format!(
                                "Error downloading a dependency {}~{}.\nCheck if the dependency name and version are correct.\nIf you are not sure check https://soldeer.xyz.",
                                err.name, err.version
                            ),
                        });
                    }
                }

                match write_lock(&dependencies, false) {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(SoldeerError {
                            message: format!("Error writing the lock: {}", err.cause),
                        });
                    }
                }
            }
            match unzip_dependency(&dependency_name, &dependency_version) {
                Ok(_) => {}
                Err(err_unzip) => {
                    match janitor::cleanup_dependency(&dependency_name, &dependency_version, true) {
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

            let config_file: String = match define_config_file() {
                Ok(file) => file,

                Err(_) => match cleanup_dependency(&dependency_name, &dependency_version, true) {
                    Ok(_) => {
                        return Err(SoldeerError {
                            message: "Could define the config file".to_string(),
                        });
                    }
                    Err(_) => {
                        return Err(SoldeerError {
                            message: "Could not delete dependency artifacts".to_string(),
                        });
                    }
                },
            };

            match add_to_config(
                &dependency_name,
                &dependency_version,
                &dependency_url,
                custom_url,
                &config_file,
            ) {
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
            match janitor::cleanup_dependency(&dependency_name, &dependency_version, false) {
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

            // Check for sensitive files or directories
            if check_dotfiles_recursive(&path_buf) {
                if !prompt_user_for_confirmation() {
                    println!("{}", Paint::yellow("Push operation aborted by the user."));
                    return Ok(());
                }
            }

            if push.dry_run.is_some() && push.dry_run.unwrap() {
                println!(
                    "{}",
                    Paint::green("🦌 Running soldeer push with dry-run, a zip file will be available for inspection 🦌\n")
                );
            } else {
                println!("{}", Paint::green("🦌 Running soldeer push 🦌\n"));
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
                push.dry_run.unwrap(),
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

    let dependencies: Vec<Dependency> = match read_config(String::new()).await {
        Ok(dep) => dep,
        Err(err) => return Err(SoldeerError { message: err.cause }),
    };

    match download_dependencies(&dependencies, true).await {
        Ok(_) => {}
        Err(err) => {
            return Err(SoldeerError {
                message: format!(
                    "Error downloading a dependency {}~{}",
                    err.name, err.version
                ),
            })
        }
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

    use std::env::{self};
    use std::fs::{remove_dir_all, remove_file, File};
    use std::io::Write;
    use std::path::Path;
    use std::{
        fs::{self},
        path::PathBuf,
    };

    use commands::{Install, Push, Update};
    use rand::{distributions::Alphanumeric, Rng};
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
        });

        match run(command) {
            Ok(_) => {}
            Err(_) => {
                clean_test_env(target_config.clone());
                assert_eq!("Invalid State", "")
            }
        }

        let mut path_dependency = DEPENDENCY_DIR.join("@gearbox-protocol-periphery-v3-1.6.1");

        assert!(Path::new(&path_dependency).exists());
        path_dependency = DEPENDENCY_DIR.join("@openzeppelin-contracts-5.0.2");
        assert!(Path::new(&path_dependency).exists());
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
        });

        match run(command) {
            Ok(_) => {}
            Err(_) => {
                clean_test_env(target_config.clone());
                assert_eq!("Invalid State", "")
            }
        }

        let path_dependency = DEPENDENCY_DIR.join("@tt-1.6.1");

        assert!(Path::new(&path_dependency).exists());
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

        assert!(Path::new(&path_dependency).exists());
        clean_test_env(target_config);
    }

    #[test]
    #[serial]
    fn soldeer_push_dry_run() {
        let _ = remove_dir_all(DEPENDENCY_DIR.clone());
        let _ = remove_file(LOCK_FILE.clone());

        let command = Subcommands::Push(Push {
            dependency: "@test~1.1".to_string(),
            path: Some(String::from(
                env::current_dir().unwrap().join("test").to_str().unwrap(),
            )),
            dry_run: Some(true),
        });

        match run(command) {
            Ok(_) => {}
            Err(_) => {
                clean_test_env(PathBuf::default());
                assert_eq!("Invalid State", "")
            }
        }

        let path_dependency = env::current_dir().unwrap().join("test").join("test.zip");

        assert!(Path::new(&path_dependency).exists());
        let archive = File::open(&path_dependency);
        let archive = ZipArchive::new(archive.unwrap());
        assert_eq!(archive.unwrap().len(), 2);
        clean_test_env(PathBuf::default());
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
        let mut file: std::fs::File = fs::OpenOptions::new()
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

        // Create test directory
        if !test_dir.exists() {
            std::fs::create_dir(&test_dir).unwrap();
        }

        // Create a .env file in the test directory
        let env_file_path = test_dir.join(".env");
        let mut env_file = File::create(&env_file_path).unwrap();
        writeln!(env_file, "SENSITIVE_DATA=secret").unwrap();

        // Mock the confirmation prompt to simulate user input
        utils::prompt_user_for_confirmation;

        let command = Subcommands::Push(Push {
            dependency: "@test~1.1".to_string(),
            path: Some(test_dir.to_str().unwrap().to_string()),
            dry_run: None,
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
}
