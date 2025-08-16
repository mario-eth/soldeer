pub use clap::{Parser, Subcommand};
use clap_verbosity_flag::{LogLevel, VerbosityFilter};
use derive_more::derive::From;

pub mod init;
pub mod install;
pub mod login;
pub mod push;
pub mod uninstall;
pub mod update;

#[derive(Copy, Clone, Debug, Default)]
pub struct CustomLevel;

impl LogLevel for CustomLevel {
    fn default_filter() -> VerbosityFilter {
        VerbosityFilter::Error
    }

    fn verbose_help() -> Option<&'static str> {
        Some("Use structured logging and increase verbosity")
    }

    fn verbose_long_help() -> Option<&'static str> {
        Some(
            r#"Use structured logging and increase verbosity

Pass multiple times to increase the logging level (e.g. -v, -vv, -vvv).
If omitted, then a pretty TUI output will be used.
Otherwise:
- 1 (-v): print logs with level error and warning
- 2 (-vv): print logs with level info
- 3 (-vvv): print logs with level debug
- 4 (-vvvv): print logs with level trace
"#,
        )
    }

    fn quiet_help() -> Option<&'static str> {
        Some("Disable logs and output, or reduce verbosity")
    }
}

/// A minimal Solidity dependency manager
#[derive(Parser, Debug, bon::Builder)]
#[clap(name = "soldeer", author = "m4rio.eth", version)]
#[non_exhaustive]
pub struct Args {
    #[clap(subcommand)]
    pub command: Command,

    /// Test
    #[command(flatten)]
    pub verbose: clap_verbosity_flag::Verbosity<CustomLevel>,
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
        return Err("The dependency should be in the format <DEPENDENCY>[~<VERSION>]".to_string());
    }
    Ok(dep.to_string())
}
