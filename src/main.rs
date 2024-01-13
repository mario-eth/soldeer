mod auth;
mod config;
mod dependency_downloader;
mod janitor;
mod lock;
mod utils;
mod versioning;

use std::process::exit;

use crate::auth::login;
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
use crate::versioning::push_version;
use clap::{
    Parser,
    Subcommand,
};

const REMOTE_REPOSITORY: &str =
    "https://raw.githubusercontent.com/mario-eth/soldeer-versions/main/all_dependencies.toml";

pub const BASE_URL: &str = "http://localhost:3000";
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
    Login(Login),
    Push(Push),
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

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Login into the central repository to push the dependencies.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer login"
)]
pub struct Login {}

#[derive(Debug, Clone, Parser)]
#[clap(
    about = "Push a dependency to the central repository.",
    after_help = "For more information, read the README.md",
    override_usage = "soldeer push <DEPENDENCY>~<VERSION>"
)]
pub struct Push {
    #[clap(required = true)]
    dependency: String,
}

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
                    url: remote_url.clone(),
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
                        &remote_url,
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
        Subcommands::Login(_) => {
            login().await;
        }
        Subcommands::Push(push) => {
            let dependency_name: String =
                push.dependency.split('~').collect::<Vec<&str>>()[0].to_string();
            let dependency_version: String =
                push.dependency.split('~').collect::<Vec<&str>>()[1].to_string();
            let _ = push_version(dependency_name, dependency_version).await;
        }
    }
}
