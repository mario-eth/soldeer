use crate::{
    ConfigLocation,
    utils::{Progress, success, warning},
};
use clap::Parser;
use soldeer_core::{
    Result,
    config::{Paths, read_config_deps, read_soldeer_config},
    errors::LockError,
    install::{InstallProgress, ensure_dependencies_dir},
    lock::{generate_lockfile_contents, read_lockfile},
    remappings::{RemappingsAction, edit_remappings},
    update::update_dependencies,
};
use std::fs;

/// Update dependencies by reading the config file
#[derive(Debug, Clone, Default, Parser, bon::Builder)]
#[allow(clippy::duplicated_attributes)]
#[builder(on(String, into), on(ConfigLocation, into))]
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
    success!("Done reading config");
    ensure_dependencies_dir(&paths.dependencies)?;
    let (dependencies, warnings) = read_config_deps(&paths.config)?;
    for w in warnings {
        warning!(format!("Config warning: {w}"));
    }

    let lockfile = read_lockfile(&paths.lock)?;
    success!("Done reading lockfile");
    let (progress, monitor) = InstallProgress::new();
    let bars = Progress::new("Updating dependencies", dependencies.len(), monitor);
    bars.start_all();
    let new_locks = update_dependencies(
        &dependencies,
        &lockfile.entries,
        &paths.dependencies,
        config.recursive_deps,
        progress,
    )
    .await?;
    bars.stop_all();

    let new_lockfile_content = generate_lockfile_contents(new_locks);
    fs::write(&paths.lock, new_lockfile_content).map_err(LockError::IOError)?;
    success!("Updated lockfile");

    edit_remappings(&RemappingsAction::Update, &config, paths)?;
    success!("Updated remappings");
    Ok(())
}
