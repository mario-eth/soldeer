use crate::ConfigLocation;
use clap::Parser;
use cliclack::{log::success, multi_progress};
use soldeer_core::{
    config::{read_config_deps, read_soldeer_config, Dependency, Paths},
    errors::LockError,
    install::{ensure_dependencies_dir, Progress},
    lock::{generate_lockfile_contents, read_lockfile},
    remappings::{edit_remappings, RemappingsAction},
    update::update_dependencies,
    Result,
};
use std::fs;

/// Update dependencies by reading the config file
#[derive(Debug, Clone, Default, Parser, bon::Builder)]
#[builder(on(String, into))]
#[clap(after_help = "For more information, read the README.md")]
#[non_exhaustive]
pub struct Update {
    /// If set, this command will delete the existing remappings and re-create them
    #[arg(short = 'g', long, default_value_t = false)]
    #[builder(default)]
    pub regenerate_remappings: bool,

    /// If set, this command will install the dependencies recursively (via submodules or via
    /// soldeer)
    #[arg(short = 'd', long, default_value_t = false)]
    #[builder(default)]
    pub recursive_deps: bool,

    /// Specify the config location without prompting.
    ///
    /// This prevents prompting the user if the automatic detection can't determine the config
    /// location.
    #[arg(long, value_enum)]
    pub config_location: Option<ConfigLocation>,
}

// TODO: add a parameter for a dependency name, where we would only update that particular
// dependency

pub(crate) async fn update_command(paths: &Paths, cmd: Update) -> Result<()> {
    let mut config = read_soldeer_config(&paths.config)?;
    if cmd.regenerate_remappings {
        config.remappings_regenerate = true;
    }
    if cmd.recursive_deps {
        config.recursive_deps = true;
    }
    success("Done reading config")?;
    ensure_dependencies_dir(&paths.dependencies)?;
    let dependencies: Vec<Dependency> = read_config_deps(&paths.config)?;
    let lockfile = read_lockfile(&paths.lock)?;
    success("Done reading lockfile")?;
    let multi = multi_progress("Updating dependencies");
    let progress = Progress::new(&multi, dependencies.len() as u64);
    progress.start_all();
    let new_locks = update_dependencies(
        &dependencies,
        &lockfile.entries,
        &paths.dependencies,
        config.recursive_deps,
        progress.clone(),
    )
    .await?;
    progress.stop_all();
    multi.stop();

    let new_lockfile_content = generate_lockfile_contents(new_locks);
    fs::write(&paths.lock, new_lockfile_content).map_err(LockError::IOError)?;
    success("Updated lockfile")?;

    edit_remappings(&RemappingsAction::Update, &config, paths)?;
    success("Updated remappings")?;
    Ok(())
}
