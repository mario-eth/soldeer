use crate::{
    config::{Dependency, GitDependency, HttpDependency},
    download::IntegrityChecksum,
    errors::InstallError,
    lock::LockEntry,
    remote::get_dependency_url_remote,
    utils::hash_folder,
};
use tokio::{fs, process::Command};

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
    }
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
    let current_hash =
        hash_folder(&path, None).map_err(|e| InstallError::IOError { path, source: e })?;
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
    let top_level = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| InstallError::IOError { path: path.clone(), source: e })?;
    if !top_level.status.success() {
        // error getting the top level directory, assume the directory is not a git repository
        fs::remove_dir_all(&path).await.map_err(|e| InstallError::IOError { path, source: e })?;
        return Ok(DependencyStatus::Missing);
    }
    // compare the top level directory to the install path
    let top_level = String::from_utf8(top_level.stdout).unwrap_or_default();
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
    let mut diff = Command::new("git")
        .arg("diff")
        .arg("--exit-code")
        .arg(&lock.checksum)
        .current_dir(&path)
        .spawn()
        .map_err(|e| InstallError::IOError { path: path.clone(), source: e })?;
    let status = diff.wait().await.map_err(|e| InstallError::IOError { path, source: e })?;
    if !status.success() {
        return Ok(DependencyStatus::FailedIntegrity);
    }
    Ok(DependencyStatus::Installed)
}

/// Reset a git dependency to the commit specified in the lockfile entry
///
/// This function runs `git reset --hard <commit>` and `git clean -fd` in the git dependency's
/// directory
async fn reset_git_dependency(dependency: &GitDependency, lock: &LockEntry) -> Result<()> {
    let path = dependency.install_path();
    let reset = Command::new("git")
        .arg("reset")
        .arg("--hard")
        .arg(&lock.checksum)
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| InstallError::IOError { path: path.clone(), source: e })?;
    if !reset.status.success() {
        return Err(InstallError::GitError(String::from_utf8(reset.stdout).unwrap_or_default()));
    }
    let clean = Command::new("git")
        .arg("clean")
        .arg("-fd")
        .current_dir(&path)
        .output()
        .await
        .map_err(|e| InstallError::IOError { path, source: e })?;
    if !clean.status.success() {
        return Err(InstallError::GitError(String::from_utf8(clean.stdout).unwrap_or_default()));
    }
    Ok(())
}
