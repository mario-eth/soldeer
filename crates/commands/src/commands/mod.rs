pub use clap::{Parser, Subcommand};

pub mod init;
pub mod install;
pub mod login;
pub mod push;
pub mod uninstall;
pub mod update;

/// A minimal Solidity dependency manager
#[derive(Parser, Debug)]
#[clap(name = "soldeer", author = "m4rio.eth", version)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Subcommands,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Subcommands {
    Init(init::Init),
    Install(install::Install),
    Update(update::Update),
    Login(login::Login),
    Push(push::Push),
    Uninstall(uninstall::Uninstall),
    Version(Version),
}

fn validate_dependency(dep: &str) -> std::result::Result<String, String> {
    if dep.split('~').count() != 2 {
        return Err("The dependency should be in the format <DEPENDENCY>~<VERSION>".to_string());
    }
    Ok(dep.to_string())
}

/// Display the version of Soldeer
#[derive(Debug, Clone, Default, Parser)]
pub struct Version {}
