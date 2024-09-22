//! Update dependencies to the latest version.
use crate::{
    config::{Dependency, GitIdentifier},
    errors::UpdateError,
    install::install_dependency,
    lock::{format_install_path, GitLockEntry, LockEntry},
    registry::get_latest_supported_version,
    utils::run_git_command,
};
use std::path::Path;
use tokio::task::JoinSet;

#[cfg(feature = "cli")]
use crate::install::Progress;

pub type Result<T> = std::result::Result<T, UpdateError>;

/// Update the dependencies to a new version.
///
/// This function spawns a task for each dependency and waits for all of them to finish.
///
/// For Git dependencies without a ref or with a
/// [`GitIdentifier::Branch`] ref, the function will update
/// the dependency to the latest commit with `git pull`.
///
/// For Git dependencies with a [`GitIdentifier::Rev`] or [`GitIdentifier::Tag`] ref, the function
/// will reset the repo to the ref if the integrity check fails. An update is not really possible in
/// this case.
///
/// For HTTP dependencies, the function will install the latest version of the dependency according
/// to the version requirement in the config file. If the version requirement is not a semver range,
/// the function will install the latest version from the registry.
pub async fn update_dependencies(
    dependencies: &[Dependency],
    locks: &[LockEntry],
    deps_path: impl AsRef<Path>,
    recursive_deps: bool,
    #[cfg(feature = "cli")] progress: Progress,
) -> Result<Vec<LockEntry>> {
    let mut set = JoinSet::new();
    for dep in dependencies {
        set.spawn({
            let d = dep.clone();
            #[cfg(feature = "cli")]
            let p = progress.clone();

            let lock = locks.iter().find(|l| l.name() == dep.name()).cloned();
            let paths = deps_path.as_ref().to_path_buf();
            async move {
                update_dependency(
                    &d,
                    lock.as_ref(),
                    &paths,
                    recursive_deps,
                    #[cfg(feature = "cli")]
                    p,
                )
                .await
            }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res??);
    }
    Ok(results)
}

/// Update a single dependency to a new version.
///
/// For Git dependencies without a ref or with a
/// [`GitIdentifier::Branch`] ref, the function will update
/// the dependency to the latest commit with `git pull`.
///
/// For Git dependencies with a [`GitIdentifier::Rev`] or [`GitIdentifier::Tag`] ref, the function
/// will reset the repo to the ref if the integrity check fails. An update is not really possible in
/// this case.
///
/// For HTTP dependencies, the function will install the latest version of the dependency according
/// to the version requirement in the config file. If the version requirement is not a semver range,
/// the function will install the latest version from the registry.
pub async fn update_dependency(
    dependency: &Dependency,
    lock: Option<&LockEntry>,
    deps: impl AsRef<Path>,
    recursive_deps: bool,
    #[cfg(feature = "cli")] progress: Progress,
) -> Result<LockEntry> {
    match dependency {
        Dependency::Git(ref dep)
            if matches!(dep.identifier, None | Some(GitIdentifier::Branch(_))) =>
        {
            // we handle the git case in a special way because we don't need to re-clone the repo
            // update to the latest commit (git pull)
            let path = match lock {
                Some(lock) => lock.install_path(&deps),
                None => dependency.install_path(&deps).await.unwrap_or_else(|| {
                    format_install_path(dependency.name(), dependency.version_req(), &deps)
                }),
            };
            run_git_command(&["reset", "--hard", "HEAD"], Some(&path)).await?;
            run_git_command(&["clean", "-fd"], Some(&path)).await?;
            let old_commit = run_git_command(&["rev-parse", "--verify", "HEAD"], Some(&path))
                .await?
                .trim()
                .to_string();

            if let Some(GitIdentifier::Branch(ref branch)) = dep.identifier {
                // checkout the desired branch
                run_git_command(&["checkout", branch], Some(&path)).await?;
            } else {
                // necessarily `None` because of the match above
                // checkout the default branch
                let branch = run_git_command(
                    &["symbolic-ref", "refs/remotes/origin/HEAD", "--short"],
                    Some(&path),
                )
                .await?
                .trim_start_matches("origin/")
                .trim()
                .to_string();
                run_git_command(&["checkout", &branch], Some(&path)).await?;
            }
            // pull the latest commits
            run_git_command(&["pull"], Some(&path)).await?;
            let commit = run_git_command(&["rev-parse", "--verify", "HEAD"], Some(&path))
                .await?
                .trim()
                .to_string();
            if commit != old_commit {
                #[cfg(feature = "cli")]
                progress.log(format!("Updating {dependency} from {old_commit:.7} to {commit:.7}"));
            }
            let new_lock = GitLockEntry::builder()
                .name(&dep.name)
                .version(&dep.version_req)
                .git(&dep.git)
                .rev(commit)
                .build()
                .into();
            #[cfg(feature = "cli")]
            progress.increment_all();

            Ok(new_lock)
        }
        Dependency::Git(ref dep) if dep.identifier.is_some() => {
            // check integrity against the existing version since we can't update to a new rev
            let lock = match lock {
                Some(lock) => lock,
                None => &GitLockEntry::builder()
                    .name(&dep.name)
                    .version(&dep.version_req)
                    .git(&dep.git)
                    .rev(dep.identifier.as_ref().expect("identifier should be present").to_string())
                    .build()
                    .into(),
            };
            let new_lock = install_dependency(
                dependency,
                Some(lock),
                &deps,
                None,
                recursive_deps,
                #[cfg(feature = "cli")]
                progress,
            )
            .await?;
            Ok(new_lock)
        }
        _ => {
            // for http dependencies, we simply install them as if there was no lock entry

            // to show which version we update to, we already need to know the new version, so we
            // can pass it to `install_dependency` to spare us from another call to the
            // registry
            let force_version = match (dependency.url(), lock) {
                (None, Some(lock)) => {
                    let new_version = get_latest_supported_version(dependency).await?;
                    if lock.version() != new_version {
                        #[cfg(feature = "cli")]
                        progress.log(format!(
                            "Updating {} from {} to {new_version}",
                            dependency.name(),
                            lock.version(),
                        ));
                    }
                    Some(new_version)
                }
                _ => None,
            };
            let new_lock = install_dependency(
                dependency,
                None,
                &deps,
                force_version,
                recursive_deps,
                #[cfg(feature = "cli")]
                progress,
            )
            .await?;
            Ok(new_lock)
        }
    }
}
