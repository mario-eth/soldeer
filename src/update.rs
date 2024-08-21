use crate::{
    config::Dependency,
    errors::UpdateError,
    install::{install_dependency, Progress},
    lock::{format_install_path, GitLockEntry, LockEntry},
    registry::get_latest_supported_version,
    utils::run_git_command,
};
use tokio::task::JoinSet;

pub type Result<T> = std::result::Result<T, UpdateError>;

pub async fn update_dependencies(
    dependencies: &[Dependency],
    locks: &[LockEntry],
    recursive_deps: bool,
    progress: Progress,
) -> Result<Vec<LockEntry>> {
    let mut set = JoinSet::new();
    for dep in dependencies {
        set.spawn({
            let d = dep.clone();
            let p = progress.clone();
            let lock = locks.iter().find(|l| l.name() == dep.name()).cloned();
            async move { update_dependency(&d, lock.as_ref(), recursive_deps, p).await }
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
    lock: Option<&LockEntry>,
    recursive_deps: bool,
    progress: Progress,
) -> Result<LockEntry> {
    match dependency {
        Dependency::Git(ref dep) if dep.rev.is_none() => {
            // we handle the git case in a special way because we don't need to re-clone the repo
            // update to the latest commit (git pull)
            let path = match lock {
                Some(lock) => lock.install_path(),
                None => dependency.install_path().await.unwrap_or_else(|| {
                    format_install_path(dependency.name(), dependency.version_req())
                }),
            };
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
            let new_lock = GitLockEntry::builder()
                .name(&dep.name)
                .version(&dep.version_req)
                .git(&dep.git)
                .rev(commit)
                .build()
                .into();
            progress.increment_all();
            Ok(new_lock)
        }
        Dependency::Git(ref dep) if dep.rev.is_some() => {
            // check integrity against the existing version since we can't update to a new rev
            let lock = match lock {
                Some(lock) => lock,
                None => &GitLockEntry::builder()
                    .name(&dep.name)
                    .version(&dep.version_req)
                    .git(&dep.git)
                    .rev(dep.rev.clone().expect("rev field should be present"))
                    .build()
                    .into(),
            };
            let new_lock =
                install_dependency(dependency, Some(lock), None, recursive_deps, progress).await?;
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
            let new_lock =
                install_dependency(dependency, None, force_version, recursive_deps, progress)
                    .await?;
            Ok(new_lock)
        }
    }
}
