//! High-level commands for the Soldeer CLI
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
pub use crate::commands::{Args, Command};
use cliclack::{intro, log::step, outro, outro_cancel};
use soldeer_core::{config::Paths, Result};
use std::env;

pub mod commands;

/// The location where the Soldeer config should be stored.
///
/// We re-implement the type from `soldeer_core` to avoid requiring the clap dependency to derive
/// `ValueEnum`. Because of Rust's orphan rules, we can't implement `ValueEnum` for a type from
/// another crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum ConfigLocation {
    /// Store config inside the `foundry.toml` file.
    Foundry,

    /// Store config inside the `soldeer.toml` file.
    Soldeer,
}

impl From<ConfigLocation> for soldeer_core::config::ConfigLocation {
    fn from(value: ConfigLocation) -> Self {
        match value {
            ConfigLocation::Foundry => Self::Foundry,
            ConfigLocation::Soldeer => Self::Soldeer,
        }
    }
}

pub async fn run(command: Command) -> Result<()> {
    match command {
        Command::Init(cmd) => {
            intro("🦌 Soldeer Init 🦌")?;
            step("Initialize Foundry project to use Soldeer")?;
            let paths = Paths::with_config(cmd.config_location.map(Into::into))?;
            commands::init::init_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during initialization").ok();
            })?;
            outro("Done initializing!")?;
        }
        Command::Install(cmd) => {
            intro("🦌 Soldeer Install 🦌")?;
            let paths = Paths::with_config(cmd.config_location.map(Into::into))?;
            commands::install::install_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during install").ok();
            })?;
            outro("Done installing!")?;
        }
        Command::Update(cmd) => {
            intro("🦌 Soldeer Update 🦌")?;
            let paths = Paths::with_config(cmd.config_location.map(Into::into))?;
            commands::update::update_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during the update").ok();
            })?;
            outro("Done updating!")?;
        }
        Command::Uninstall(cmd) => {
            intro("🦌 Soldeer Uninstall 🦌")?;
            let paths = Paths::new()?;
            commands::uninstall::uninstall_command(&paths, &cmd).inspect_err(|_| {
                outro_cancel("An error occurred during uninstall").ok();
            })?;
            outro("Done uninstalling!")?;
        }
        Command::Login(cmd) => {
            intro("🦌 Soldeer Login 🦌")?;
            commands::login::login_command(cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during login").ok();
            })?;
            outro("Done logging in!")?;
        }
        Command::Push(cmd) => {
            intro("🦌 Soldeer Push 🦌")?;
            commands::push::push_command(cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during push").ok();
            })?;
            outro("Done!")?;
        }
        Command::Version(_) => {
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            println!("soldeer {VERSION}");
        }
    }
    Ok(())
}
