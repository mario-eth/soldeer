use clap::Parser;
use soldeer_core::{auth::login, Result};

/// Log into the central repository to push packages
#[derive(Debug, Clone, Default, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Login {}

pub(crate) async fn login_command() -> Result<()> {
    login().await?;
    Ok(())
}
