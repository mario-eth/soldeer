mod auth;
pub mod commands;
mod config;
mod dependency_downloader;
mod janitor;
mod lock;
mod remote;
mod utils;
mod versioning;

use crate::auth::login;
use crate::commands::{
    Args,
    Subcommands,
};
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
use regex::Regex;
use std::{
    path::PathBuf,
    process::exit,
};

pub const BASE_URL: &str = "https://api.soldeer.xyz";

#[derive(Debug)]
pub struct FOUNDRY {
    remappings: bool,
}

#[tokio::main]
pub async fn run(args: Args) {
    println!("Running Soldeer...");
    match args.command {
        Subcommands::Install(install) => {
            let dependency_name: String =
                install.dependency.split('~').collect::<Vec<&str>>()[0].to_string();
            let dependency_version: String =
                install.dependency.split('~').collect::<Vec<&str>>()[1].to_string();
            let dependency_url: String;
            if install.remote_url.is_some() {
                let remote_url = install.remote_url.unwrap();
                let mut dependencies: Vec<Dependency> = Vec::new();
                dependency_url = remote_url.clone();
                dependencies.push(Dependency {
                    name: dependency_name.clone(),
                    version: dependency_version.clone(),
                    url: dependency_url.clone(),
                });
                println!("Checking lock file...");
                dependencies = lock_check(&dependencies).unwrap();
                if dependencies.is_empty() {
                    eprintln!(
                        "Dependency {}-{} already installed",
                        dependency_name, dependency_version
                    );
                    exit(500);
                }
                if download_dependencies(&dependencies, false).await.is_err() {
                    eprintln!("Error downloading dependencies");
                    exit(500);
                }
                let _ = write_lock(&dependencies);
            } else {
                let mut dependencies: Vec<Dependency> = Vec::new();
                dependencies = lock_check(&dependencies).unwrap();
                dependencies.push(Dependency {
                    name: dependency_name.clone(),
                    version: dependency_version.clone(),
                    url: String::new(),
                });
                dependencies = lock_check(&dependencies).unwrap();
                if dependencies.is_empty() {
                    eprintln!(
                        "Dependency {}-{} already installed",
                        dependency_name, dependency_version
                    );
                    exit(500);
                }

                match {
                    dependency_downloader::download_dependency_remote(
                        &dependency_name,
                        &dependency_version,
                    )
                    .await
                } {
                    Ok(url) => {
                        dependencies[0].url = url;
                        dependency_url = dependencies[0].url.clone();
                    }
                    Err(err) => {
                        eprintln!("Error downloading dependency: {:?}", err);
                        exit(500);
                    }
                }
                let _ = write_lock(&dependencies);
            }
            match unzip_dependency(&dependency_name, &dependency_version) {
                Ok(_) => {}
                Err(err) => {
                    eprintln!("Error unzipping dependency: {:?}", err);
                    match janitor::cleanup_dependency(&dependency_name, &dependency_version) {
                        Ok(_) => {}
                        Err(err) => {
                            eprintln!("Error cleanup dependency: {:?}", err);
                            exit(500);
                        }
                    }
                    exit(500);
                }
            }

            config::add_to_config(&dependency_name, &dependency_version, &dependency_url);

            match janitor::healthcheck_dependency(&dependency_name, &dependency_version) {
                Ok(_) => {}
                Err(err) => {
                    eprintln!("Error health-checking dependency: {:?}", err);
                    exit(500);
                }
            }
            match janitor::cleanup_dependency(&dependency_name, &dependency_version) {
                Ok(_) => {}
                Err(err) => {
                    eprintln!("Error cleanup dependency: {:?}", err);
                    exit(500);
                }
            }
            // check the foundry setup, in case we have a foundry.toml, then the foundry.toml will be used for `sdependencies`
            let f_setup_vec: Vec<bool> = get_foundry_setup();
            let foundry_setup: FOUNDRY = FOUNDRY {
                remappings: f_setup_vec[0],
            };

            if foundry_setup.remappings {
                remappings();
            }
        }
        Subcommands::Update(_) => {
            let dependencies: Vec<Dependency> = read_config(String::new());

            if download_dependencies(&dependencies, true).await.is_err() {
                eprintln!("Error downloading dependencies");
                exit(500);
            }
            let result: Result<(), zip_extract::ZipExtractError> =
                unzip_dependencies(&dependencies);
            if result.is_err() {
                eprintln!("Error unzipping dependencies: {:?}", result.err().unwrap());
                exit(500);
            }
            let result: Result<(), janitor::MissingDependencies> =
                healthcheck_dependencies(&dependencies);
            if result.is_err() {
                eprintln!(
                    "Error health-checking dependencies {:?}",
                    result.err().unwrap().name
                );
                exit(500);
            }
            let result: Result<(), janitor::MissingDependencies> = cleanup_after(&dependencies);
            if result.is_err() {
                eprintln!(
                    "Error cleanup dependencies {:?}",
                    result.err().unwrap().name
                );
                exit(500);
            }
            // check the foundry setup, in case we have a foundry.toml, then the foundry.toml will be used for `sdependencies`
            let f_setup_vec: Vec<bool> = get_foundry_setup();
            let foundry_setup: FOUNDRY = FOUNDRY {
                remappings: f_setup_vec[0],
            };

            if foundry_setup.remappings {
                remappings();
            }
        }
        Subcommands::Login(_) => {
            login().await;
        }
        Subcommands::Push(push) => {
            println!("Pushing dependency...");
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
            let regex = Regex::new(r"^[@|a-z][a-z0-9-]*[a-z]$").unwrap();

            if !regex.is_match(&dependency_name) {
                // TODO need to work on this to accept only @ at the beginning and - in the middle
                println!(
                    "Dependency name {} is not valid, you can use only alphanumeric characters `-` and `@`",
                    dependency_name
                );
                exit(500);
            }
            let _ = push_version(dependency_name, dependency_version, PathBuf::from(path)).await;
        }
    }
}
