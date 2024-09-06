//! High-level commands for the Soldeer CLI
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
pub use crate::commands::{Args, Subcommands};
use cliclack::{intro, log::step, outro, outro_cancel};
use soldeer_core::{config::Paths, Result};
use std::env;

pub mod commands;

pub async fn run(command: Subcommands) -> Result<()> {
    let paths = Paths::new()?;
    match command {
        Subcommands::Init(init) => {
            intro("🦌 Soldeer Init 🦌")?;
            step("Initialize Foundry project to use Soldeer")?;
            commands::init::init_command(&paths, init).await.inspect_err(|_| {
                outro_cancel("An error occurred during initialization").ok();
            })?;
            outro("Done initializing!")?;
        }
        Subcommands::Install(cmd) => {
            intro("🦌 Soldeer Install 🦌")?;
            commands::install::install_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during install").ok();
            })?;
            outro("Done installing!")?;
        }
        Subcommands::Update(cmd) => {
            intro("🦌 Soldeer Update 🦌")?;
            commands::update::update_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during the update").ok();
            })?;
            outro("Done updating!")?;
        }
        Subcommands::Uninstall(cmd) => {
            intro("🦌 Soldeer Uninstall 🦌")?;
            commands::uninstall::uninstall_command(&paths, &cmd).inspect_err(|_| {
                outro_cancel("An error occurred during uninstall").ok();
            })?;
            outro("Done uninstalling!")?;
        }
        Subcommands::Login(_) => {
            intro("🦌 Soldeer Login 🦌")?;
            commands::login::login_command().await.inspect_err(|_| {
                outro_cancel("An error occurred during login").ok();
            })?;
            outro("Done logging in!")?;
        }
        Subcommands::Push(cmd) => {
            intro("🦌 Soldeer Push 🦌")?;
            commands::push::push_command(cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during push").ok();
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
