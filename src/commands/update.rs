use super::Result;
use crate::config::{get_config_path, read_config_deps, read_soldeer_config, Dependency};
use clap::Parser;
use cliclack::log::success;

/// Update dependencies by reading the config file
#[derive(Debug, Clone, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Update {
    /// If set, this command will delete the existing remappings and re-create them
    #[arg(short = 'g', long, default_value_t = false)]
    pub regenerate_remappings: bool,

    /// If set, this command will install the recursive dependencies (via submodules or via
    /// soldeer)
    #[arg(short = 'd', long, default_value_t = false)]
    pub recursive_deps: bool,
}

pub(crate) async fn update_command(cmd: Update) -> Result<()> {
    let config_path = get_config_path()?;
    let mut config = read_soldeer_config(Some(&config_path))?;
    if cmd.regenerate_remappings {
        config.remappings_regenerate = true;
    }
    if cmd.recursive_deps {
        config.recursive_deps = true;
    }
    success("Done reading config")?;
    let dependencies: Vec<Dependency> = read_config_deps(Some(&config_path))?;
    // TODO: update dependencies

    Ok(())
}
