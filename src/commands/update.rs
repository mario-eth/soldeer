use std::fs;

use super::Result;
use crate::{
    config::{get_config_path, read_config_deps, read_soldeer_config, update_deps, Dependency},
    errors::LockError,
    install::{ensure_dependencies_dir, Progress as InstallProgress},
    lock::generate_lockfile_contents,
    remappings::update_remappings,
    update::{update_dependencies, Progress},
    LOCK_FILE,
};
use clap::Parser;
use cliclack::{log::success, multi_progress};

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

// TODO: add a parameter for a dependency name, where we would only update that particular
// dependency

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
    ensure_dependencies_dir()?;
    let dependencies: Vec<Dependency> = read_config_deps(Some(&config_path))?;
    let multi = multi_progress("Updating dependencies");
    let install_progress = InstallProgress::new(&multi, dependencies.len() as u64);
    let progress = Progress::new(&install_progress, dependencies.len() as u64);
    progress.start_all();
    let new_items =
        update_dependencies(&dependencies, config.recursive_deps, progress.clone()).await?;
    progress.stop_all();
    multi.stop();

    let (new_deps, new_locks): (Vec<_>, Vec<_>) = new_items.into_iter().unzip();

    // update config file
    update_deps(&new_deps, &config_path)?;
    success("Updated config file")?;

    let new_lockfile_content = generate_lockfile_contents(new_locks);
    fs::write(LOCK_FILE.as_path(), new_lockfile_content).map_err(LockError::IOError)?;
    success("Updated lockfile")?;

    update_remappings(&config, &config_path).await?;
    success("Updated remappings")?;
    Ok(())
}
