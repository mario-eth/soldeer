pub use clap::{Parser, Subcommand};
use derive_more::derive::From;

pub mod init;
pub mod install;
pub mod login;
pub mod push;
pub mod uninstall;
pub mod update;

/// A minimal Solidity dependency manager
#[derive(Parser, Debug, bon::Builder)]
#[clap(name = "soldeer", author = "m4rio.eth", version)]
#[non_exhaustive]
pub struct Args {
    #[clap(subcommand)]
    pub command: Command,
}

/// The available commands for Soldeer
#[derive(Debug, Clone, Subcommand, From)]
#[non_exhaustive]
pub enum Command {
    Init(init::Init),
    Install(install::Install),
    Update(update::Update),
    Login(login::Login),
    Push(push::Push),
    Uninstall(uninstall::Uninstall),
    Version(Version),
}

/// Display the version of Soldeer
#[derive(Debug, Clone, Default, Parser)]
#[non_exhaustive]
pub struct Version {}

fn validate_dependency(dep: &str) -> std::result::Result<String, String> {
    if dep.split('~').count() != 2 {
        return Err("The dependency should be in the format <DEPENDENCY>~<VERSION>".to_string());
    }
    Ok(dep.to_string())
}
