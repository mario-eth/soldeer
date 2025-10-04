//! High-level commands for the Soldeer CLI
#![cfg_attr(docsrs, feature(doc_cfg))]
pub use crate::commands::{Args, Command};
use clap::builder::PossibleValue;
pub use clap_verbosity_flag::Verbosity;
use clap_verbosity_flag::log::Level;
use commands::CustomLevel;
use derive_more::derive::FromStr;
use soldeer_core::{Result, config::Paths};
use std::{
    env,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use utils::{get_config_location, intro, outro, outro_cancel, step};

pub mod commands;
pub mod utils;

static TUI_ENABLED: AtomicBool = AtomicBool::new(true);

/// The location where the Soldeer config should be stored.
///
/// This is a new type so we can implement the `ValueEnum` trait for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, FromStr)]
pub struct ConfigLocation(soldeer_core::config::ConfigLocation);

impl clap::ValueEnum for ConfigLocation {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self(soldeer_core::config::ConfigLocation::Foundry),
            Self(soldeer_core::config::ConfigLocation::Soldeer),
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(match self.0 {
            soldeer_core::config::ConfigLocation::Foundry => PossibleValue::new("foundry"),
            soldeer_core::config::ConfigLocation::Soldeer => PossibleValue::new("soldeer"),
        })
    }
}

impl From<ConfigLocation> for soldeer_core::config::ConfigLocation {
    fn from(value: ConfigLocation) -> Self {
        value.0
    }
}

impl From<soldeer_core::config::ConfigLocation> for ConfigLocation {
    fn from(value: soldeer_core::config::ConfigLocation) -> Self {
        Self(value)
    }
}

pub async fn run(command: Command, verbosity: Verbosity<CustomLevel>) -> Result<()> {
    if env::var("RUST_LOG").is_ok() {
        env_logger::builder().try_init().ok(); // init logger if possible (not already initialized)
        TUI_ENABLED.store(false, Ordering::Relaxed);
    } else {
        match verbosity.log_level() {
            Some(level) if level > Level::Error => {
                // the user requested structure logging (-v[v*])
                // init logger if possible (not already initialized)
                env_logger::Builder::new()
                    .filter_level(verbosity.log_level_filter())
                    .try_init()
                    .ok();
                TUI_ENABLED.store(false, Ordering::Relaxed);
            }
            Some(_) => TUI_ENABLED.store(true, Ordering::Relaxed),
            _ => TUI_ENABLED.store(false, Ordering::Relaxed),
        }
    }
    match command {
        Command::Init(cmd) => {
            intro!("🦌 Soldeer Init 🦌");
            step!("Initialize Foundry project to use Soldeer");
            // for init, we always use the current dir as root, unless specified by env
            let root = env::var("SOLDEER_PROJECT_ROOT")
                .map(|p| {
                    if p.is_empty() {
                        env::current_dir().expect("could not determine current directory")
                    } else {
                        PathBuf::from(p)
                    }
                })
                .unwrap_or(env::current_dir()?);
            let paths = Paths::with_root_and_config(
                &root,
                Some(get_config_location(&root, cmd.config_location)?),
            )?;
            commands::init::init_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel!("An error occurred during initialization");
            })?;
            outro!("Done initializing!");
        }
        Command::Install(cmd) => {
            intro!("🦌 Soldeer Install 🦌");
            let root = Paths::get_root_path();
            let paths = Paths::with_root_and_config(
                &root,
                Some(get_config_location(&root, cmd.config_location)?),
            )?;
            commands::install::install_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel!("An error occurred during install");
            })?;
            outro!("Done installing!");
        }
        Command::Update(cmd) => {
            intro!("🦌 Soldeer Update 🦌");
            let root = Paths::get_root_path();
            let paths = Paths::with_root_and_config(
                &root,
                Some(get_config_location(&root, cmd.config_location)?),
            )?;
            commands::update::update_command(&paths, cmd).await.inspect_err(|_| {
                outro_cancel!("An error occurred during the update");
            })?;
            outro!("Done updating!");
        }
        Command::Uninstall(cmd) => {
            intro!("🦌 Soldeer Uninstall 🦌");
            let root = Paths::get_root_path();
            let paths =
                Paths::with_root_and_config(&root, Some(get_config_location(&root, None)?))?;
            commands::uninstall::uninstall_command(&paths, &cmd).inspect_err(|_| {
                outro_cancel!("An error occurred during uninstall");
            })?;
            outro!("Done uninstalling!");
        }
        Command::Login(cmd) => {
            intro!("🦌 Soldeer Login 🦌");
            commands::login::login_command(cmd).await.inspect_err(|_| {
                outro_cancel!("An error occurred during login");
            })?;
            outro!("Done logging in!");
        }
        Command::Push(cmd) => {
            intro!("🦌 Soldeer Push 🦌");
            commands::push::push_command(cmd).await.inspect_err(|_| {
                outro_cancel!("An error occurred during push");
            })?;
            outro!("Done!");
        }
        Command::Version(_) => {
            const VERSION: &str = env!("CARGO_PKG_VERSION");
            println!("soldeer {VERSION}");
        }
    }
    Ok(())
}
