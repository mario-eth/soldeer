use crate::{
    config::{Dependency, HttpDependency},
    errors::UpdateError,
    install::{install_dependency, Progress as InstallProgress, PROGRESS_TEMPLATE},
    lock::LockEntry,
    registry::{get_all_versions_descending, Versions},
    utils::run_git_command,
};
use cliclack::{progress_bar, ProgressBar};
use semver::VersionReq;
use std::fmt::Display;
use tokio::task::JoinSet;

pub type Result<T> = std::result::Result<T, UpdateError>;

#[derive(Clone)]
pub struct Progress {
    pub install_progress: InstallProgress,
    pub get_versions: ProgressBar,
}

impl Progress {
    pub fn new(install_progress: &InstallProgress, deps: u64) -> Self {
        let get_version_pb =
            install_progress.multi.add(progress_bar(deps).with_template(PROGRESS_TEMPLATE));
        Self { install_progress: install_progress.clone(), get_versions: get_version_pb }
    }

    pub fn start_all(&self) {
        self.install_progress.start_all();
        self.get_versions.start("Retieving all versions...");
    }

    pub fn stop_all(&self) {
        self.install_progress.stop_all();
        self.get_versions.stop("Done retrieving versions");
    }

    pub fn log(&self, msg: impl Display) {
        self.install_progress.multi.println(msg);
    }
}

pub async fn update_dependencies(
    dependencies: &[Dependency],
    recursive_deps: bool,
    progress: Progress,
) -> Result<Vec<(Dependency, LockEntry)>> {
    let mut set = JoinSet::new();
    for dep in dependencies {
        set.spawn({
            let d = dep.clone();
            let p = progress.clone();
            async move { update_dependency(&d, recursive_deps, p).await }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res??);
    }
    Ok(results)
}

pub async fn update_dependency(
    dependency: &Dependency,
    recursive_deps: bool,
    progress: Progress,
) -> Result<(Dependency, LockEntry)> {
    // we can't update dependencies that are http with a custom URL or git dependencies with a
    // commit hash
    let new_dependency = match dependency {
        Dependency::Http(dep) if dep.url.is_some() => {
            progress.log(format!("{dependency} has a custom URL, version can't be updated"));
            dependency.clone()
        }
        Dependency::Git(dep) if dep.rev.is_some() => {
            progress.log(format!("{dependency} is a git dependency, rev can't be updated"));
            dependency.clone()
        }
        Dependency::Git(_) => dependency.clone(),
        Dependency::Http(_) => {
            let new_version = match get_all_versions_descending(dependency.name()).await? {
                Versions::Semver(all_versions) => {
                    match dependency.version().parse::<VersionReq>() {
                        Ok(req) => {
                            let new_version = all_versions
                                .iter()
                                .find(|version| req.matches(version))
                                .ok_or(UpdateError::NoMatchingVersion {
                                    dependency: dependency.name().to_string(),
                                    version_req: dependency.version().to_string(),
                                })?;
                            new_version.to_string()
                        }
                        Err(_) => {
                            // we can't check which version is newer, so we just take the latest one
                            all_versions
                                .into_iter()
                                .next()
                                .map(|v| v.to_string())
                                .expect("there should be at least 1 version")
                        }
                    }
                }
                Versions::NonSemver(all_versions) => {
                    // we can't check which version is newer, so we just take the latest one
                    all_versions.into_iter().next().expect("there should be at least 1 version")
                }
            };
            if new_version != dependency.version() {
                progress.log(format!(
                    "Updating {} from {} to {new_version}",
                    dependency.name(),
                    dependency.version(),
                ));
            }
            Dependency::Http(HttpDependency {
                name: dependency.name().to_string(),
                version: new_version,
                url: None,
            })
        }
    };
    progress.get_versions.inc(1);

    match new_dependency {
        Dependency::Git(ref dep) if dep.rev.is_none() => {
            // we handle the git case in a special way because we don't need to re-clone the repo
            // update to the latest commit (git pull)
            let path = dependency.install_path();
            run_git_command(&["reset", "--hard", "HEAD"], Some(&path)).await?;
            run_git_command(&["clean", "-fd"], Some(&path)).await?;
            let old_commit = run_git_command(&["rev-parse", "--verify", "HEAD"], Some(&path))
                .await?
                .trim()
                .to_string();
            run_git_command(&["pull"], Some(&path)).await?;
            let commit = run_git_command(&["rev-parse", "--verify", "HEAD"], Some(&path))
                .await?
                .trim()
                .to_string();
            if commit != old_commit {
                progress.log(format!("Updating {dependency} from {old_commit:.7} to {commit:.7}"));
            }
            let lock = LockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .source(&dep.git)
                .checksum(commit)
                .build();
            progress.install_progress.increment_all();
            Ok((new_dependency, lock))
        }
        Dependency::Git(ref dep) if dep.rev.is_some() => {
            // check integrity against the existing version since we can't update to a new rev
            let lock = LockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .source(&dep.git)
                .checksum(dep.rev.clone().expect("rev field should be present"))
                .build();
            let new_lock = install_dependency(
                &new_dependency,
                Some(&lock),
                recursive_deps,
                progress.install_progress,
            )
            .await?;
            Ok((new_dependency, new_lock))
        }
        _ => {
            // for http dependencies, we simply re-install them
            let lock = install_dependency(
                &new_dependency,
                None,
                recursive_deps,
                progress.install_progress,
            )
            .await?;
            Ok((new_dependency, lock))
        }
    }
}
