use super::{validate_dependency, Result};
use crate::{
    config::{add_to_config, get_config_path, read_config_deps, read_soldeer_config, Dependency},
    errors::{InstallError, LockError},
    install::{ensure_dependencies_dir, install_dependencies, install_dependency, Progress},
    lock::{add_to_lockfile, generate_lockfile_contents, read_lockfile},
    remappings::{add_to_remappings, update_remappings},
    DEPENDENCY_DIR, LOCK_FILE,
};
use clap::Parser;
use cliclack::{
    log::{remark, success, warning},
    multi_progress, outro,
};
use std::fs;

/// Install a dependency
#[derive(Debug, Clone, Parser)]
#[clap(
    long_about = "Install a dependency

You can install a dependency from the Soldeer repository, a custom URL pointing to a zip file, or from Git using a Git link.
**Important:** The `~` symbol when specifying the dependency is crucial to differentiate between the name and the version that needs to be installed.
- **Example from Soldeer repository:**
  soldeer install @openzeppelin-contracts~2.3.0
- **Example from a custom URL:**
  soldeer install @openzeppelin-contracts~2.3.0 https://github.com/OpenZeppelin/openzeppelin-contracts/archive/refs/tags/v5.0.2.zip
- **Example from Git:**
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git
- **Example from Git with a specified commit:**
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git --rev 05f218fb6617932e56bf5388c3b389c3028a7b73",
    after_help = "For more information, read the README.md"
)]
pub struct Install {
    /// The dependency name and version, separated by a tilde.
    ///
    /// If not present, this command will perform `soldeer update`
    #[arg(value_parser = validate_dependency, value_name = "DEPENDENCY~VERSION")]
    pub dependency: Option<String>,

    /// The URL to the dependency zip file, if not from the Soldeer repository
    ///
    /// Example: https://my-domain/dep.zip
    #[arg(value_name = "URL")]
    pub remote_url: Option<String>,

    /// The revision of the dependency, if from Git
    #[arg(long)]
    pub rev: Option<String>,

    /// If set, this command will delete the existing remappings and re-create them
    #[arg(short = 'g', long, default_value_t = false)]
    pub regenerate_remappings: bool,

    /// If set, this command will install the recursive dependencies (via submodules or via
    /// soldeer)
    #[arg(short = 'd', long, default_value_t = false)]
    pub recursive_deps: bool,

    /// Perform a clean install by re-installing the dependencies
    #[arg(long, default_value_t = false)]
    pub clean: bool,
}

pub(crate) async fn install_command(cmd: Install) -> Result<()> {
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
    match cmd.dependency {
        None => {
            let (locks, lockfile_content) = read_lockfile()?;
            success("Done reading lockfile")?;
            if cmd.clean {
                remark("Flag `--clean` was set, re-installing all dependencies")?;
                fs::remove_dir_all(DEPENDENCY_DIR.as_path()).map_err(|e| {
                    InstallError::IOError { path: DEPENDENCY_DIR.to_path_buf(), source: e }
                })?;
                ensure_dependencies_dir()?;
            }
            let multi = multi_progress("Installing dependencies");
            let progress = Progress::new(&multi, dependencies.len() as u64);
            progress.start_all();
            let new_locks = install_dependencies(
                &dependencies,
                &locks,
                config.recursive_deps,
                progress.clone(),
            )
            .await?;
            progress.stop_all();
            multi.stop();
            let new_lockfile_content = generate_lockfile_contents(new_locks);
            if !locks.is_empty() && new_lockfile_content != lockfile_content {
                warning("Warning: the lock file is out of sync with the dependencies. Consider running `soldeer update` to re-generate the lockfile.")?;
            } else if locks.is_empty() {
                fs::write(LOCK_FILE.as_path(), new_lockfile_content).map_err(LockError::IOError)?;
            }
            update_remappings(&config, &config_path).await?;
            success("Updated remappings")?;
        }
        Some(dependency) => {
            let mut dep = Dependency::from_name_version(&dependency, cmd.remote_url, cmd.rev)?;
            if dependencies.iter().any(|d| d.name() == dep.name() && d.version() == dep.version()) {
                outro(format!("{dep} is already installed"))?;
                return Ok(());
            }
            let multi = multi_progress(format!("Installing {dep}"));
            let progress = Progress::new(&multi, 1);
            progress.start_all();
            let lock =
                install_dependency(&dep, None, config.recursive_deps, progress.clone()).await?;
            progress.stop_all();
            multi.stop();
            // for GIT deps, we need to add the commit hash before adding them to the
            // config.
            if let Some(git_dep) = dep.as_git_mut() {
                git_dep.rev = Some(lock.checksum.clone());
            }
            add_to_config(&dep, &config_path)?;
            success("Dependency added to config")?;
            add_to_lockfile(lock)?;
            success("Dependency added to lockfile")?;
            add_to_remappings(dep, &config, &config_path).await?;
            success("Dependency added to remappings")?;
        }
    }
    Ok(())
}
