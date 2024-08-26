use super::{validate_dependency, Result};
use crate::{
    config::{
        add_to_config, read_config_deps, read_soldeer_config, Dependency, GitIdentifier, Paths,
    },
    errors::{InstallError, LockError},
    install::{ensure_dependencies_dir, install_dependencies, install_dependency, Progress},
    lock::{add_to_lockfile, generate_lockfile_contents, read_lockfile},
    remappings::{edit_remappings, RemappingsAction},
};
use clap::Parser;
use cliclack::{
    log::{remark, success, warning},
    multi_progress, outro,
};
use std::fs;

/// Install a dependency
#[derive(Debug, Clone, Default, Parser)]
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
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git --rev 05f218fb6617932e56bf5388c3b389c3028a7b73
- **Example from Git with a specified tag:**
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git --tag my-tag
- **Example from Git with a specified branch:**
  soldeer install @openzeppelin-contracts~2.3.0 git@github.com:OpenZeppelin/openzeppelin-contracts.git --branch my-branch",
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
    #[arg(value_name = "URL", requires = "dependency")]
    pub remote_url: Option<String>,

    /// A Git revision
    #[arg(long, group = "identifier", requires = "remote_url")]
    pub rev: Option<String>,

    /// A Git tag
    #[arg(long, group = "identifier", requires = "remote_url")]
    pub tag: Option<String>,

    /// A Git branch
    #[arg(long, group = "identifier", requires = "remote_url")]
    pub branch: Option<String>,

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

pub(crate) async fn install_command(paths: &Paths, cmd: Install) -> Result<()> {
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
    match cmd.dependency {
        None => {
            let (locks, lockfile_content) = read_lockfile(&paths.lock)?;
            success("Done reading lockfile")?;
            if cmd.clean {
                remark("Flag `--clean` was set, re-installing all dependencies")?;
                fs::remove_dir_all(&paths.dependencies).map_err(|e| InstallError::IOError {
                    path: paths.dependencies.clone(),
                    source: e,
                })?;
                ensure_dependencies_dir(&paths.dependencies)?;
            }
            let multi = multi_progress("Installing dependencies");
            let progress = Progress::new(&multi, dependencies.len() as u64);
            progress.start_all();
            let new_locks = install_dependencies(
                &dependencies,
                &locks,
                &paths.dependencies,
                config.recursive_deps,
                progress.clone(),
            )
            .await?;
            progress.stop_all();
            multi.stop();
            let new_lockfile_content = generate_lockfile_contents(new_locks);
            if !lockfile_content.is_empty() && new_lockfile_content != lockfile_content {
                warning("Warning: the lock file is out of sync with the dependencies. Consider running `soldeer update` to re-generate the lockfile.")?;
            } else if lockfile_content.is_empty() {
                fs::write(&paths.lock, new_lockfile_content).map_err(LockError::IOError)?;
            }
            edit_remappings(&RemappingsAction::None, &config, paths)?;
            success("Updated remappings")?;
        }
        Some(dependency) => {
            let identifier = match (cmd.rev, cmd.branch, cmd.tag) {
                (Some(rev), None, None) => Some(GitIdentifier::from_rev(&rev)),
                (None, Some(branch), None) => Some(GitIdentifier::from_branch(&branch)),
                (None, None, Some(tag)) => Some(GitIdentifier::from_tag(&tag)),
                (None, None, None) => None,
                _ => unreachable!("clap should prevent this"),
            };
            let mut dep = Dependency::from_name_version(&dependency, cmd.remote_url, identifier)?;
            if dependencies
                .iter()
                .any(|d| d.name() == dep.name() && d.version_req() == dep.version_req())
            {
                outro(format!("{dep} is already installed"))?;
                return Ok(());
            }
            let multi = multi_progress(format!("Installing {dep}"));
            let progress = Progress::new(&multi, 1);
            progress.start_all();
            let lock = install_dependency(
                &dep,
                None,
                &paths.dependencies,
                None,
                config.recursive_deps,
                progress.clone(),
            )
            .await?;
            progress.stop_all();
            multi.stop();
            // for git deps, we need to add the commit hash before adding them to the
            // config, unless a branch/tag was specified
            if let Some(git_dep) = dep.as_git_mut() {
                if git_dep.identifier.is_none() {
                    git_dep.identifier = Some(GitIdentifier::from_rev(
                        &lock.as_git().expect("lock entry should be of type git").rev,
                    ));
                }
            }
            add_to_config(&dep, &paths.config)?;
            success("Dependency added to config")?;
            add_to_lockfile(lock, &paths.lock)?;
            success("Dependency added to lockfile")?;
            edit_remappings(&RemappingsAction::Add(dep), &config, paths)?;
            success("Dependency added to remappings")?;
        }
    }
    Ok(())
}
