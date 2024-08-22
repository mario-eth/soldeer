use std::fs;

use super::Result;
use crate::{
    config::{add_to_config, get_config_path, read_soldeer_config},
    install::{ensure_dependencies_dir, install_dependency, Progress},
    lock::add_to_lockfile,
    registry::get_latest_forge_std,
    remappings::add_to_remappings,
    utils::remove_forge_lib,
    PROJECT_ROOT,
};
use clap::Parser;
use cliclack::{
    log::{remark, success},
    multi_progress,
};

/// Initialize a new Soldeer project for use with Foundry
#[derive(Debug, Clone, Default, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Init {
    /// Clean the Foundry project by removing .gitmodules and the lib directory
    #[arg(long, default_value_t = false)]
    pub clean: bool,
}

pub(crate) async fn init_command(cmd: Init) -> Result<()> {
    if cmd.clean {
        remark("Flag `--clean` was set, removing `lib` dir and submodules")?;
        remove_forge_lib().await?;
    }

    let config_path = get_config_path()?;
    let config = read_soldeer_config(Some(&config_path))?;
    success("Done reading config")?;
    ensure_dependencies_dir()?;
    let dependency = get_latest_forge_std().await?;
    let multi = multi_progress(format!("Installing {dependency}"));
    let progress = Progress::new(&multi, 1);
    progress.start_all();
    let lock = install_dependency(&dependency, None, None, false, progress.clone()).await.map_err(
        |e| {
            multi.error(&e);
            e
        },
    )?;
    progress.stop_all();
    multi.stop();
    add_to_config(&dependency, &config_path)?;
    success("Dependency added to config")?;
    add_to_lockfile(lock)?;
    success("Dependency added to lockfile")?;
    add_to_remappings(dependency, &config, &config_path).await?;
    success("Dependency added to remappings")?;

    let gitignore_path = PROJECT_ROOT.join(".gitignore");
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
