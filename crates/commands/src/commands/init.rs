use crate::ConfigLocation;
use clap::Parser;
use cliclack::{
    log::{remark, success},
    multi_progress,
};
use soldeer_core::{
    config::{add_to_config, read_soldeer_config, update_config_libs, Paths},
    install::{ensure_dependencies_dir, install_dependency, Progress},
    lock::add_to_lockfile,
    registry::get_latest_version,
    remappings::{edit_remappings, RemappingsAction},
    utils::remove_forge_lib,
    Result,
};
use std::fs;

/// Convert a Foundry project to use Soldeer
#[derive(Debug, Clone, Default, Parser, bon::Builder)]
#[builder(on(String, into))]
#[clap(after_help = "For more information, read the README.md")]
#[non_exhaustive]
pub struct Init {
    /// Clean the Foundry project by removing .gitmodules and the lib directory
    #[arg(long, default_value_t = false)]
    #[builder(default)]
    pub clean: bool,

    /// Specify the config location.
    ///
    /// This prevents prompting the user if the automatic detection can't determine the config
    /// location.
    #[arg(long, value_enum)]
    pub config_location: Option<ConfigLocation>,
}

pub(crate) async fn init_command(paths: &Paths, cmd: Init) -> Result<()> {
    if cmd.clean {
        remark("Flag `--clean` was set, removing `lib` dir and submodules")?;
        remove_forge_lib(&paths.root).await?;
    }
    let config = read_soldeer_config(&paths.config)?;
    success("Done reading config")?;
    ensure_dependencies_dir(&paths.dependencies)?;
    let dependency = get_latest_version("forge-std").await?;
    let multi = multi_progress(format!("Installing {dependency}"));
    let progress = Progress::new(&multi, 1);
    progress.start_all();
    let lock =
        install_dependency(&dependency, None, &paths.dependencies, None, false, progress.clone())
            .await
            .inspect_err(|e| {
                multi.error(e);
            })?;
    progress.stop_all();
    multi.stop();
    add_to_config(&dependency, &paths.config)?;
    let foundry_config = paths.root.join("foundry.toml");
    if foundry_config.exists() {
        update_config_libs(foundry_config)?;
    }
    success("Dependency added to config")?;
    add_to_lockfile(lock, &paths.lock)?;
    success("Dependency added to lockfile")?;
    edit_remappings(&RemappingsAction::Add(dependency), &config, paths)?;
    success("Dependency added to remappings")?;

    let gitignore_path = paths.root.join(".gitignore");
    if gitignore_path.exists() {
        let mut gitignore = fs::read_to_string(&gitignore_path)?;
        if !gitignore.contains("dependencies") {
            gitignore.push_str("\n\n# Soldeer\n/dependencies\n");
            fs::write(&gitignore_path, gitignore)?;
        }
    }
    success("Added `dependencies` to .gitignore")?;

    Ok(())
}
