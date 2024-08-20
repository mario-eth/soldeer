use super::Result;
use crate::auth::login;
use clap::Parser;

/// Log into the central repository to push the dependencies
#[derive(Debug, Clone, Default, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Login {}

pub(crate) async fn login_command() -> Result<()> {
    login().await?;
    Ok(())
}
