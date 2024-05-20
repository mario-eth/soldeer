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
use crate::utils::get_current_working_dir;
use crate::versioning::push_version;
use lazy_static::lazy_static;
use regex::Regex;
use std::env;
use std::path::PathBuf;
use yansi::Paint;

pub const BASE_URL: &str = "https://api.soldeer.xyz";

// pub static DEPENDENCY_DIR: PathBuf = Lazy::new(|| env::current_dir().unwrap());

lazy_static! {
    pub static ref DEPENDENCY_DIR: PathBuf =
        get_current_working_dir().unwrap().join("dependencies/");
    pub static ref LOCK_FILE: PathBuf = get_current_working_dir().unwrap().join("soldeer.lock");
}

#[derive(Debug)]
pub struct FOUNDRY {
    remappings: bool,
}

#[tokio::main]
pub async fn run(command: Subcommands) -> Result<(), SoldeerError> {
    match command {
        Subcommands::Install(install) => {
            println!("{}", Paint::green("🦌 Running soldeer install 🦌\n"));
            if !install.dependency.contains('~') {
                return Err(SoldeerError {
                    message: format!(
                        "Dependency {} does not specify a version. \nThe format should be [DEPENDENCY]~[VERSION]",
                        install.dependency
                    ),
                });
            }
            let dependency_name: String =
                install.dependency.split('~').collect::<Vec<&str>>()[0].to_string();
            let dependency_version: String =
                install.dependency.split('~').collect::<Vec<&str>>()[1].to_string();
            let dependency_url: String;
            let mut custom_url = false;
            if install.remote_url.is_some() {
                custom_url = true;
                let remote_url = install.remote_url.unwrap();
                let mut dependencies: Vec<Dependency> = Vec::new();
                dependency_url = remote_url.clone();
                dependencies.push(Dependency {
                    name: dependency_name.clone(),
                    version: dependency_version.clone(),
                    url: dependency_url.clone(),
                });

                match lock_check(&dependencies) {
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
                let mut dependencies: Vec<Dependency> = vec![Dependency {
                    name: dependency_name.clone(),
                    version: dependency_version.clone(),
                    url: String::new(),
                }];
                match lock_check(&dependencies) {
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
                                "Error downloading a dependency {}~{}. \nCheck if the dependency name and version are correct.\nIf you are not sure check https://soldeer.xyz.",
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
                    match janitor::cleanup_dependency(&dependency_name, &dependency_version) {
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

            match config::add_to_config(
                &dependency_name,
                &dependency_version,
                &dependency_url,
                custom_url,
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
            match janitor::cleanup_dependency(&dependency_name, &dependency_version) {
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
            println!("{}", Paint::green("🦌 Running soldeer push 🦌\n"));
            let dependency_name: String =
                push.dependency.split('~').collect::<Vec<&str>>()[0].to_string();
            let dependency_version: String =
                push.dependency.split('~').collect::<Vec<&str>>()[1].to_string();

            let path = push.path.unwrap_or(
                get_current_working_dir()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string(),
            );
            let regex = Regex::new(r"^[@|a-z0-9][a-z0-9-]*[a-z0-9]$").unwrap();

            if !regex.is_match(&dependency_name) {
                return Err(SoldeerError{message:format!("Dependency name {} is not valid, you can use only alphanumeric characters `-` and `@`", &dependency_name)});
            }
            match push_version(&dependency_name, &dependency_version, PathBuf::from(path)).await {
                Ok(_) => {}
                Err(err) => {
                    return Err(SoldeerError {
                        message: format!(
                            "Dependency {}~{} could not be pushed. \nCause: {}",
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
