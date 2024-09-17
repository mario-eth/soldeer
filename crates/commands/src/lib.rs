//! High-level commands for the Soldeer CLI
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
pub use crate::commands::{Args, Command};
use cliclack::{intro, log::step, outro, outro_cancel};
use soldeer_core::{config::Paths, Result};
use std::env;

pub mod commands;

pub async fn run(command: Command) -> Result<()> {
    let paths = Paths::new()?;
    match command {
        Command::Init(init) => {
            intro("🦌 Soldeer Init 🦌")?;
            step("Initialize Foundry project to use Soldeer")?;
            commands::init::init_command(&paths, init).await.inspect_err(|_| {
                outro_cancel("An error occurred during initialization").ok();
            })?;
            outro("Done initializing!")?;
        }
        Command::Install(cmd) => {
            intro("🦌 Soldeer Install 🦌")?;
            commands::install::install_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during install").ok();
            })?;
            outro("Done installing!")?;
        }
        Command::Update(cmd) => {
            intro("🦌 Soldeer Update 🦌")?;
            commands::update::update_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel("An error occurred during the update").ok();
            })?;
            outro("Done updating!")?;
        }
        Command::Uninstall(cmd) => {
            intro("🦌 Soldeer Uninstall 🦌")?;
            commands::uninstall::uninstall_command(&paths, &cmd).inspect_err(|_| {
                outro_cancel("An error occurred during uninstall").ok();
            })?;
            outro("Done uninstalling!")?;
        }
        Command::Login(_) => {
            intro("🦌 Soldeer Login 🦌")?;
            commands::login::login_command().await.inspect_err(|_| {
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
