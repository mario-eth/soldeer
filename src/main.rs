mod config;
mod dependency_downloader;
mod janitor;
mod utils;

use std::process::exit;

use crate::config::{get_foundry_setup, read_config, remappings, Dependency};
use crate::dependency_downloader::{download_dependencies, unzip_dependencies, unzip_dependency};
use crate::janitor::{cleanup_after, healthcheck_dependencies};
use clap::{Parser, Subcommand};

const REMOTE_REPOSITORY: &str =
    "https://raw.githubusercontent.com/mario-eth/soldeer-versions/main/all_dependencies.toml";

#[derive(Debug)]
pub struct FOUNDRY {
    remappings: bool,
}

#[derive(Parser, Debug)]
#[clap(
    name = "soldeer",
    author = "m4rio.eth",
    version,
    about = "A minimal solidity dependency manager"
)]
struct Args {
    #[clap(subcommand)]
    command: Subcommands,
}

#[derive(Debug, Subcommand)]
enum Subcommands {
    Install(Install),
    Update(Update),
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Install a dependency from soldeer repository or from a custom url that points to a zip file. Example: dependency~version. the `~` is very important to differentiate between the name and the version that needs to be installed.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer install <DEPENDENCY>~<VERSION> [URL]"
)]
pub struct Install {
    #[clap(required = true)]
    dependency: String,
    #[clap(required = false)]
    remote_url: Option<String>,
}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Update dependencies by reading the config file",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer update"
)]
pub struct Update {}

#[derive(Debug, Clone, Parser)]
pub struct Help {}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // check the foundry setup, in case we have a foundry.toml, then the foundry.toml will be used for `sdependencies`
    let f_setup_vec: Vec<bool> = get_foundry_setup();
    let foundry_setup: FOUNDRY = FOUNDRY {
        remappings: f_setup_vec[0],
    };

    match args.command {
        Subcommands::Install(install) => {
            let dependency_name: String =
                install.dependency.split('~').collect::<Vec<&str>>()[0].to_string();
            let dependency_version: String =
                install.dependency.split('~').collect::<Vec<&str>>()[1].to_string();
            let dependency_url: String;
            let mut remote_url: String = REMOTE_REPOSITORY.to_string();
            if install.remote_url.is_some() {
                remote_url = install.remote_url.unwrap();
                let mut dependencies: Vec<Dependency> = Vec::new();
                dependency_url = remote_url.clone();
                dependencies.push(Dependency {
                    name: dependency_name.clone(),
                    version: dependency_version.clone(),
                    url: dependency_url.clone(),
                });
                if download_dependencies(&dependencies, true).await.is_err() {
                    eprintln!("Error downloading dependencies");
                    exit(500);
                }
            } else {
                match dependency_downloader::download_dependency_remote(
                    &dependency_name,
                    &dependency_version,
                    &remote_url,
                )
                .await
                {
                    Ok(url) => {
                        dependency_url = url;
                    }
                    Err(err) => {
                        eprintln!("Error downloading dependency: {:?}", err);
                        exit(500);
                    }
                }
            }
            match unzip_dependency(&dependency_name, &dependency_version) {
                Ok(_) => {}
                Err(err) => {
                    eprintln!("Error unzipping dependency: {:?}", err);
                    exit(500);
                }
            }
            // TODO this is kinda junky written, need to refactor and a better TOML writer
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
            if foundry_setup.remappings {
                remappings();
            }
        }
    }
}
