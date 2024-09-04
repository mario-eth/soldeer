//! Soldeer is a package manager for Solidity projects
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
pub use crate::{commands::Subcommands, errors::SoldeerError};
use cliclack::{intro, log::step, outro, outro_cancel};
use config::Paths;
use std::env;

mod auth;
pub mod commands;
mod config;
mod download;
pub mod errors;
mod install;
mod lock;
mod push;
mod registry;
mod remappings;
mod update;
mod utils;

#[tokio::main]
pub async fn run(command: Subcommands) -> Result<(), SoldeerError> {
    let paths = Paths::new()?;
    match command {
        Subcommands::Init(init) => {
            intro("🦌 Soldeer Init 🦌")?;
            step("Initialize Foundry project to use Soldeer")?;
            commands::init::init_command(&paths, init).await.map_err(|e| {
                outro_cancel("An error occurred during initialization").ok();
                e
            })?;
            outro("Done initializing!")?;
        }
        Subcommands::Install(cmd) => {
            intro("🦌 Soldeer Install 🦌")?;
            commands::install::install_command(&paths, cmd).await.map_err(|e| {
                outro_cancel("An error occurred during install").ok();
                e
            })?;
            outro("Done installing!")?;
        }
        Subcommands::Update(cmd) => {
            intro("🦌 Soldeer Update 🦌")?;
            commands::update::update_command(&paths, cmd).await.map_err(|e| {
                outro_cancel("An error occurred during the update").ok();
                e
            })?;
            outro("Done updating!")?;
        }
        Subcommands::Uninstall(cmd) => {
            intro("🦌 Soldeer Uninstall 🦌")?;
            commands::uninstall::uninstall_command(&paths, &cmd).map_err(|e| {
                outro_cancel("An error occurred during uninstall").ok();
                e
            })?;
            outro("Done uninstalling!")?;
        }
        Subcommands::Login(_) => {
            intro("🦌 Soldeer Login 🦌")?;
            commands::login::login_command().await.map_err(|e| {
                outro_cancel("An error occurred during login").ok();
                e
            })?;
            outro("Done logging in!")?;
        }
        Subcommands::Push(cmd) => {
            intro("🦌 Soldeer Push 🦌")?;
            commands::push::push_command(cmd).await.map_err(|e| {
                outro_cancel("An error occurred during push").ok();
                e
            })?;
            outro("Done!")?;
        }
        Subcommands::Version(_) => {
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            println!("soldeer {VERSION}");
        }
    }
    Ok(())
}
