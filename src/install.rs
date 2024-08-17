use crate::{
    config::{Dependency, GitDependency, HttpDependency},
    download::{clone_repo, download_file, unzip_file},
    errors::InstallError,
    lock::LockEntry,
    remote::get_dependency_url_remote,
    utils::{get_url_type, hash_file, hash_folder, run_git_command},
};
use std::path::Path;
use tokio::fs;
use yansi::Paint as _;

pub type Result<T> = std::result::Result<T, InstallError>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DependencyStatus {
    Missing,
    FailedIntegrity,
    Installed,
}

pub async fn install_dependency(
    dependency: &Dependency,
    lock: Option<&LockEntry>,
) -> Result<LockEntry> {
    if let Some(lock) = lock {
        match check_dependency_integrity(dependency, lock).await? {
            DependencyStatus::Installed => {
                // no action needed, dependency is already installed and matches the lockfile entry
                return Ok(lock.clone());
            }
            DependencyStatus::FailedIntegrity => match dependency {
                Dependency::Http(dep) => {
                    let path = dep.install_path();
                    fs::remove_dir_all(&path)
                        .await
                        .map_err(|e| InstallError::IOError { path, source: e })?;
                }
                Dependency::Git(dep) => {
                    reset_git_dependency(dep, lock).await?;
                    // dependency should now be at the correct commit, we can exit
                    return Ok(lock.clone());
                }
            },
            DependencyStatus::Missing => {}
        }
        return install_dependency_locked(lock, dependency.install_path()).await;
    }
    // no lockfile entry, install from config object
    let url = match dependency.url() {
        Some(url) => url.clone(),
        None => get_dependency_url_remote(dependency).await?,
    };
    Ok(LockEntry::builder()
        .name(dependency.name())
        .version(dependency.version())
        .source(url)
        .checksum(String::new()) // TODO
        .maybe_integrity(None::<String>) // TODO
        .build())
}

pub async fn install_dependency_locked(
    lock: &LockEntry,
    path: impl AsRef<Path>,
) -> Result<LockEntry> {
    match get_url_type(&lock.source) {
        crate::utils::UrlType::Git => {
            println!("{}", format!("Started GIT download of {}", lock.name).green());
            // if the dependency was specified without a commit hash and we didn't have a lockfile,
            // clone the default branch
            let rev = if lock.checksum.is_empty() { None } else { Some(lock.checksum.clone()) };
            let commit = clone_repo(&lock.source, rev, path).await?;
            Ok(LockEntry::builder()
                .name(&lock.name)
                .version(&lock.version)
                .source(&lock.source)
                .checksum(commit)
                .build())
        }
        crate::utils::UrlType::Http => {
            println!("{}", format!("Started HTTP download of {}", lock.name).green());
            let zip_path = download_file(&lock.source, path.as_ref().with_extension("zip")).await?;
            let zip_integrity = tokio::task::spawn_blocking({
                let zip_path = zip_path.clone();
                move || hash_file(zip_path)
            })
            .await?
            .map_err(|e| InstallError::IOError { path: zip_path.clone(), source: e })?;
            if lock.checksum != zip_integrity.to_string() {
                return Err(InstallError::ZipIntegrityError(zip_path.clone()).into());
            }
            let integrity = unzip_file(&zip_path).await?;
            Ok(LockEntry::builder()
                .name(&lock.name)
                .version(&lock.version)
                .source(&lock.source)
                .checksum(&lock.checksum)
                .integrity(integrity.to_string())
                .build())
        }
    }
}

pub async fn check_dependency_integrity(
    dependency: &Dependency,
    lock: &LockEntry,
) -> Result<DependencyStatus> {
    match dependency {
        Dependency::Http(http) => check_http_dependency(http, lock).await,
        Dependency::Git(git) => check_git_dependency(git, lock).await,
    }
}

async fn check_http_dependency(
    dependency: &HttpDependency,
    lock: &LockEntry,
) -> Result<DependencyStatus> {
    let path = dependency.install_path();
    if fs::metadata(&path).await.is_err() {
        return Ok(DependencyStatus::Missing);
    }
    let Some(integrity) = &lock.integrity else {
        return Ok(DependencyStatus::FailedIntegrity);
    };
    let current_hash = tokio::task::spawn_blocking({
        let path = path.clone();
        move || hash_folder(path, None)
    })
    .await?
    .map_err(|e| InstallError::IOError { path, source: e })?;
    if &current_hash.to_string() != integrity {
        return Ok(DependencyStatus::FailedIntegrity);
    }
    Ok(DependencyStatus::Installed)
}

async fn check_git_dependency(
    dependency: &GitDependency,
    lock: &LockEntry,
) -> Result<DependencyStatus> {
    let path = dependency.install_path();
    if fs::metadata(&path).await.is_err() {
        return Ok(DependencyStatus::Missing);
    }
    // check that the location is a git repository
    let top_level = match run_git_command(
        &["rev-parse", "--show-toplevel", path.to_string_lossy().as_ref()],
        Some(&path),
    )
    .await
    {
        Ok(top_level) => top_level.trim().to_string(),
        Err(_) => {
            // error getting the top level directory, assume the directory is not a git repository
            fs::remove_dir_all(&path)
                .await
                .map_err(|e| InstallError::IOError { path, source: e })?;
            return Ok(DependencyStatus::Missing);
        }
    };
    // compare the top level directory to the install path
    let absolute_path = fs::canonicalize(&path)
        .await
        .map_err(|e| InstallError::IOError { path: path.clone(), source: e })?;
    if top_level.trim() != absolute_path.to_string_lossy() {
        // the top level directory is not the install path, assume the directory is not a git
        // repository
        fs::remove_dir_all(&path).await.map_err(|e| InstallError::IOError { path, source: e })?;
        return Ok(DependencyStatus::Missing);
    }
    // for git dependencies, the `checksum` field holds the commit hash
    match run_git_command(&["diff", "--exit-code", &lock.checksum], Some(&path)).await {
        Ok(_) => Ok(DependencyStatus::Installed),
        Err(_) => Ok(DependencyStatus::FailedIntegrity),
    }
}

/// Reset a git dependency to the commit specified in the lockfile entry
///
/// This function runs `git reset --hard <commit>` and `git clean -fd` in the git dependency's
/// directory
async fn reset_git_dependency(dependency: &GitDependency, lock: &LockEntry) -> Result<()> {
    let path = dependency.install_path();
    run_git_command(&["reset", "--hard", &lock.checksum], Some(&path)).await?;
    run_git_command(&["clean", "-fd"], Some(&path)).await?;
    Ok(())
}
