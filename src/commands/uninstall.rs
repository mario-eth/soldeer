use super::Result;
use crate::{
    config::{delete_from_config, get_config_path, read_soldeer_config},
    download::delete_dependency_files_sync,
    lock::remove_lock,
    remappings::remove_from_remappings,
    SoldeerError,
};
use clap::Parser;
use cliclack::log::success;

/// Uninstall a dependency
#[derive(Debug, Clone, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Uninstall {
    /// The dependency name. Specifying a version is not necessary.
    pub dependency: String,
}

pub(crate) fn uninstall_command(cmd: &Uninstall) -> Result<()> {
    let config_path = get_config_path()?;
    let config = read_soldeer_config(Some(&config_path))?;
    success("Done reading config")?;

    // delete from the config file and return the dependency
    let dependency = delete_from_config(&cmd.dependency, &config_path)?;
    success("Dependency removed from config file")?;

    // deleting the files
    delete_dependency_files_sync(&dependency)
        .map_err(|e| SoldeerError::DownloadError { dep: dependency.to_string(), source: e })?;
    success("Dependency removed from disk")?;

    remove_lock(&dependency)?;
    success("Dependency removed from lockfile")?;

    remove_from_remappings(dependency, &config, &config_path)?;
    success("Dependency removed from remappings")?;

    Ok(())
}
