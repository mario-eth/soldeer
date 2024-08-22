use super::Result;
use crate::{
    config::{delete_from_config, read_soldeer_config, Paths},
    download::delete_dependency_files_sync,
    lock::remove_lock,
    remappings::{edit_remappings, RemappingsAction},
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

pub(crate) fn uninstall_command(paths: &Paths, cmd: &Uninstall) -> Result<()> {
    let config = read_soldeer_config(&paths.config)?;
    success("Done reading config")?;

    // delete from the config file and return the dependency
    let dependency = delete_from_config(&cmd.dependency, &paths.config)?;
    success("Dependency removed from config file")?;

    // deleting the files
    delete_dependency_files_sync(&dependency, &paths.dependencies)
        .map_err(|e| SoldeerError::DownloadError { dep: dependency.to_string(), source: e })?;
    success("Dependency removed from disk")?;

    remove_lock(&dependency, &paths.lock)?;
    success("Dependency removed from lockfile")?;

    edit_remappings(&RemappingsAction::Remove(dependency), &config, paths)?;
    success("Dependency removed from remappings")?;

    Ok(())
}
