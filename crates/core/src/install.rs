//! Install dependencies.
//!
//! This module contains functions to install dependencies from the config object or from the
//! lockfile. Dependencies can be installed in parallel.
use crate::{
    config::{Dependency, GitIdentifier},
    download::{clone_repo, delete_dependency_files, download_file, unzip_file},
    errors::InstallError,
    lock::{format_install_path, GitLockEntry, HttpLockEntry, LockEntry},
    registry::{get_dependency_url_remote, get_latest_supported_version},
    utils::{canonicalize, hash_file, hash_folder, run_forge_command, run_git_command},
};
use derive_more::derive::Display;
use log::{debug, info, warn};
use path_slash::PathBufExt as _;
use std::{
    fmt,
    ops::Deref,
    path::{Path, PathBuf},
};
use tokio::{fs, sync::mpsc, task::JoinSet};
use toml_edit::DocumentMut;

pub type Result<T> = std::result::Result<T, InstallError>;

#[derive(Debug, Clone, Display)]
pub struct DependencyName(String);

impl Deref for DependencyName {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: fmt::Display> From<&T> for DependencyName {
    fn from(value: &T) -> Self {
        Self(value.to_string())
    }
}

/// Collection of channels to monitor the progress of the install process.
#[derive(Debug)]
pub struct InstallMonitoring {
    /// Channel to receive install progress logs.
    pub logs: mpsc::UnboundedReceiver<String>,

    /// Progress for calls to the API to retrieve the packages versions.
    pub versions: mpsc::UnboundedReceiver<DependencyName>,

    /// Progress for downloading the dependencies.
    pub downloads: mpsc::UnboundedReceiver<DependencyName>,

    /// Progress for unzipping the downloaded files.
    pub unzip: mpsc::UnboundedReceiver<DependencyName>,

    /// Progress for installing subdependencies.
    pub subdependencies: mpsc::UnboundedReceiver<DependencyName>,

    /// Progress for checking the integrity of the installed dependencies.
    pub integrity: mpsc::UnboundedReceiver<DependencyName>,
}

/// Collection of channels to notify the caller of the install progress.
#[derive(Debug, Clone)]
pub struct InstallProgress {
    /// Channel to send messages to be logged to the user.
    pub logs: mpsc::UnboundedSender<String>,

    /// Progress for calls to the API to retrieve the packages versions.
    pub versions: mpsc::UnboundedSender<DependencyName>,

    /// Progress for downloading the dependencies.
    pub downloads: mpsc::UnboundedSender<DependencyName>,

    /// Progress for unzipping the downloaded files.
    pub unzip: mpsc::UnboundedSender<DependencyName>,

    /// Progress for installing subdependencies.
    pub subdependencies: mpsc::UnboundedSender<DependencyName>,

    /// Progress for checking the integrity of the installed dependencies.
    pub integrity: mpsc::UnboundedSender<DependencyName>,
}

impl InstallProgress {
    /// Create a new install progress tracker, with a receiving half ([InstallMonitoring]) and a
    /// sending half ([InstallProgress]).
    pub fn new() -> (Self, InstallMonitoring) {
        let (logs_tx, logs_rx) = mpsc::unbounded_channel();
        let (versions_tx, versions_rx) = mpsc::unbounded_channel();
        let (downloads_tx, downloads_rx) = mpsc::unbounded_channel();
        let (unzip_tx, unzip_rx) = mpsc::unbounded_channel();
        let (subdependencies_tx, subdependencies_rx) = mpsc::unbounded_channel();
        let (integrity_tx, integrity_rx) = mpsc::unbounded_channel();
        (
            Self {
                logs: logs_tx,
                versions: versions_tx,
                downloads: downloads_tx,
                unzip: unzip_tx,
                subdependencies: subdependencies_tx,
                integrity: integrity_tx,
            },
            InstallMonitoring {
                logs: logs_rx,
                versions: versions_rx,
                downloads: downloads_rx,
                unzip: unzip_rx,
                subdependencies: subdependencies_rx,
                integrity: integrity_rx,
            },
        )
    }

    /// Log a message related to progress to the caller.
    pub fn log(&self, msg: impl fmt::Display) {
        if let Err(e) = self.logs.send(msg.to_string()) {
            warn!(err:err = e; "error sending log message to the install progress channel");
        }
    }

    /// Advance all progress trackers at once, passing the dependency name.
    pub fn update_all(&self, dependency_name: DependencyName) {
        if let Err(e) = self.versions.send(dependency_name.clone()) {
            warn!(err:err = e; "error sending version message to the install progress channel");
        }
        if let Err(e) = self.downloads.send(dependency_name.clone()) {
            warn!(err:err = e; "error sending download message to the install progress channel");
        }
        if let Err(e) = self.unzip.send(dependency_name.clone()) {
            warn!(err:err = e; "error sending unzip message to the install progress channel");
        }
        if let Err(e) = self.subdependencies.send(dependency_name.clone()) {
            warn!(err:err = e; "error sending sudependencies message to the install progress channel");
        }
        if let Err(e) = self.integrity.send(dependency_name) {
            warn!(err:err = e; "error sending integrity message to the install progress channel");
        }
    }
}

/// Status of a dependency, which can either be missing, installed and untouched, or installed but
/// failing the integrity check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DependencyStatus {
    /// The dependency is missing.
    Missing,

    /// The dependency is installed but the integrity check failed.
    FailedIntegrity,

    /// The dependency is installed and the integrity check passed.
    Installed,
}

/// HTTP dependency installation information.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bon::Builder)]
#[builder(on(String, into))]
struct HttpInstallInfo {
    /// The name of the dependency.
    name: String,

    /// The version of the dependency. This is not a version requirement string but a specific.
    /// version.
    version: String,

    /// THe URL from which the zip file will be downloaded.
    url: String,

    /// The checksum of the downloaded zip file, if available (e.g. from the lockfile)
    checksum: Option<String>,
}

impl fmt::Display for HttpInstallInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version) // since the version is an exact version number,
                                                    // we use a dash and not a tilde
    }
}

/// Git dependency installation information.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bon::Builder)]
#[builder(on(String, into))]
struct GitInstallInfo {
    /// The name of the dependency.
    name: String,

    /// The version of the dependency.
    version: String,

    /// The URL of the git repository.
    git: String,

    /// The identifier of the git dependency (e.g. a commit hash, branch name, or tag name). If
    /// `None` is provided, the default branch is used.
    identifier: Option<GitIdentifier>,
}

impl fmt::Display for GitInstallInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.name, self.version)
    }
}

/// Installation information for a dependency.
///
/// A builder can be used to create the underlying [`HttpInstallInfo`] or [`GitInstallInfo`] and
/// then converted into this type with `.into()`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
enum InstallInfo {
    /// Installation information for an HTTP dependency.
    Http(HttpInstallInfo),

    /// Installation information for a git dependency.
    Git(GitInstallInfo),
}

impl From<HttpInstallInfo> for InstallInfo {
    fn from(value: HttpInstallInfo) -> Self {
        Self::Http(value)
    }
}

impl From<GitInstallInfo> for InstallInfo {
    fn from(value: GitInstallInfo) -> Self {
        Self::Git(value)
    }
}

impl From<LockEntry> for InstallInfo {
    fn from(lock: LockEntry) -> Self {
        match lock {
            LockEntry::Http(lock) => HttpInstallInfo {
                name: lock.name,
                version: lock.version,
                url: lock.url,
                checksum: Some(lock.checksum),
            }
            .into(),
            LockEntry::Git(lock) => GitInstallInfo {
                name: lock.name,
                version: lock.version,
                git: lock.git,
                identifier: Some(GitIdentifier::from_rev(lock.rev)),
            }
            .into(),
        }
    }
}

/// Install a list of dependencies in parallel.
///
/// This function spawns a task for each dependency and waits for all of them to finish. Each task
/// checks the integrity of the dependency if found on disk, downloads the dependency (zip file or
/// cloning repo) if not already present, unzips the zip file if necessary, installs
/// sub-dependencies and generates the lockfile entry.
pub async fn install_dependencies(
    dependencies: &[Dependency],
    locks: &[LockEntry],
    deps: impl AsRef<Path>,
    recursive_deps: bool,
    progress: InstallProgress,
) -> Result<Vec<LockEntry>> {
    let mut set = JoinSet::new();
    for dep in dependencies {
        debug!(dep:% = dep; "spawning task to install dependency");
        set.spawn({
            let d = dep.clone();
            let p = progress.clone();
            let lock = locks.iter().find(|l| l.name() == dep.name()).cloned();
            let deps = deps.as_ref().to_path_buf();
            async move {
                install_dependency(
                    &d,
                    lock.as_ref(),
                    deps,
                    None,
                    recursive_deps,
                    p,
                )
                .await
            }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        let res = res??;
        debug!(dep:% = res.name(); "install task finished");
        results.push(res);
    }
    debug!("all install tasks have finished");
    Ok(results)
}

/// Install a single dependency.
///
/// This function checks the integrity of the dependency if found on disk, downloads the dependency
/// (zip file or cloning repo) if not already present, unzips the zip file if necessary, installs
/// sub-dependencies and generates the lockfile entry.
///
/// If no lockfile entry is provided, the dependency is installed from the config object and
/// integrity checks are skipped.
pub async fn install_dependency(
    dependency: &Dependency,
    lock: Option<&LockEntry>,
    deps: impl AsRef<Path>,
    force_version: Option<String>,
    recursive_deps: bool,
    progress: InstallProgress,
) -> Result<LockEntry> {
    if let Some(lock) = lock {
        debug!(dep:% = dependency; "installing based on lock entry");
        match check_dependency_integrity(lock, &deps).await? {
            DependencyStatus::Installed => {
                info!(dep:% = dependency; "skipped install, dependency already up-to-date with lockfile");
                progress.update_all(dependency.into());

                return Ok(lock.clone());
            }
            DependencyStatus::FailedIntegrity => match dependency {
                Dependency::Http(_) => {
                    info!(dep:% = dependency; "dependency failed integrity check, reinstalling");
                    progress.log(format!(
                        "Dependency {dependency} failed integrity check, reinstalling"
                    ));
                    // we know the folder exists because otherwise we would have gotten
                    // `Missing`
                    delete_dependency_files(dependency, &deps).await?;
                    debug!(dep:% = dependency; "removed dependency folder");
                    // we won't need to retrieve the version number so we mark it as done
                    progress.versions.send(dependency.into()).ok();
                }
                Dependency::Git(_) => {
                    let commit = &lock.as_git().expect("lock entry should be of type git").rev;
                    info!(dep:% = dependency, commit; "dependency failed integrity check, resetting to commit");
                    progress.log(format!(
                        "Dependency {dependency} failed integrity check, resetting to commit {commit}"
                    ));

                    reset_git_dependency(
                        lock.as_git().expect("lock entry should be of type git"),
                        &deps,
                    )
                    .await?;
                    debug!(dep:% = dependency; "reset git dependency");
                    // dependency should now be at the correct commit, we can exit
                    progress.update_all(dependency.into());

                    return Ok(lock.clone());
                }
            },
            DependencyStatus::Missing => {
                // make sure there is no existing directory for the dependency
                if let Some(path) = dependency.install_path(&deps).await {
                    fs::remove_dir_all(&path)
                        .await
                        .map_err(|e| InstallError::IOError { path, source: e })?;
                }
                info!(dep:% = dependency; "dependency is missing, installing");
                // we won't need to retrieve the version number so we mark it as done
                progress.versions.send(dependency.into()).ok();
            }
        }
        install_dependency_inner(
            &lock.clone().into(),
            lock.install_path(&deps),
            recursive_deps,
            progress,
        )
        .await
    } else {
        // no lockfile entry, install from config object
        debug!(dep:% = dependency; "no lockfile entry, installing based on config");
        // make sure there is no existing directory for the dependency
        if let Some(path) = dependency.install_path(&deps).await {
            fs::remove_dir_all(&path)
                .await
                .map_err(|e| InstallError::IOError { path, source: e })?;
        }

        let (url, version) = match dependency.url() {
            // for git dependencies and http dependencies which have a custom url, we use the
            // version requirement string as version, because in that case a version requirement has
            // little sense (we can't automatically bump the version)
            Some(url) => (url.clone(), dependency.version_req().to_string()),
            None => {
                let version = match force_version {
                    Some(v) => v,
                    None => get_latest_supported_version(dependency).await?,
                };
                (get_dependency_url_remote(dependency, &version).await?, version)
            }
        };
        debug!(dep:% = dependency, version; "resolved version");
        debug!(dep:% = dependency, url; "resolved download URL");
        // indicate that we have retrieved the version number
        progress.versions.send(dependency.into()).ok();

        let info = match &dependency {
            Dependency::Http(dep) => {
                HttpInstallInfo::builder().name(&dep.name).version(&version).url(url).build().into()
            }
            Dependency::Git(dep) => GitInstallInfo::builder()
                .name(&dep.name)
                .version(&version)
                .git(url)
                .maybe_identifier(dep.identifier.clone())
                .build()
                .into(),
        };
        let install_path = format_install_path(dependency.name(), &version, &deps);
        debug!(dep:% = dependency; "installing to path {install_path:?}");
        install_dependency_inner(&info, install_path, recursive_deps, progress).await
    }
}

/// Check the integrity of a dependency that was installed.
///
/// If any file has changed in the dependency directory (except ignored files and any `.git`
/// directory), the integrity check will fail.
pub async fn check_dependency_integrity(
    lock: &LockEntry,
    deps: impl AsRef<Path>,
) -> Result<DependencyStatus> {
    match lock {
        LockEntry::Http(lock) => check_http_dependency(lock, deps).await,
        LockEntry::Git(lock) => check_git_dependency(lock, deps).await,
    }
}

/// Ensure that the dependencies directory exists.
///
/// If the directory does not exist, it will be created.
pub fn ensure_dependencies_dir(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        debug!(path:? = path; "dependencies dir doesn't exist, creating it");
        std::fs::create_dir(path)
            .map_err(|e| InstallError::IOError { path: path.to_path_buf(), source: e })?;
    }
    Ok(())
}

/// Install a single dependency.
async fn install_dependency_inner(
    dep: &InstallInfo,
    path: impl AsRef<Path>,
    subdependencies: bool,
    progress: InstallProgress,
) -> Result<LockEntry> {
    match dep {
        InstallInfo::Http(dep) => {
            let path = path.as_ref();
            let zip_path = download_file(
                &dep.url,
                path.parent().expect("dependency install path should have a parent"),
                &format!("{}-{}", dep.name, dep.version),
            )
            .await?;
            progress.downloads.send(dep.into()).ok();

            let zip_integrity = tokio::task::spawn_blocking({
                let zip_path = zip_path.clone();
                move || hash_file(zip_path)
            })
            .await?
            .map_err(|e| InstallError::IOError { path: zip_path.clone(), source: e })?;
            if let Some(checksum) = &dep.checksum {
                if checksum != &zip_integrity.to_string() {
                    return Err(InstallError::ZipIntegrityError {
                        path: zip_path.clone(),
                        expected: checksum.to_string(),
                        actual: zip_integrity.to_string(),
                    });
                }
                debug!(zip_path:?; "archive integrity check successful");
            } else {
                debug!(zip_path:?; "no checksum available for archive integrity check");
            }
            unzip_file(&zip_path, path).await?;
            progress.unzip.send(dep.into()).ok();

            if subdependencies {
                debug!(dep:% = dep; "installing subdependencies");
                install_subdependencies(path).await?;
                debug!(dep:% = dep; "finished installing subdependencies");
            }
            progress.subdependencies.send(dep.into()).ok();

            let integrity = hash_folder(path)
                .map_err(|e| InstallError::IOError { path: path.to_path_buf(), source: e })?;
            debug!(dep:% = dep, checksum = integrity.0; "integrity checksum computed");
            progress.integrity.send(dep.into()).ok();

            Ok(HttpLockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .url(&dep.url)
                .checksum(zip_integrity.to_string())
                .integrity(integrity.to_string())
                .build()
                .into())
        }
        InstallInfo::Git(dep) => {
            // if the dependency was specified without a commit hash and we didn't have a lockfile,
            // clone the default branch
            let commit = clone_repo(&dep.git, dep.identifier.as_ref(), &path).await?;
            progress.downloads.send(dep.into()).ok();

            if subdependencies {
                debug!(dep:% = dep; "installing subdependencies");
                install_subdependencies(&path).await?;
                debug!(dep:% = dep; "finished installing subdependencies");
            }
            progress.unzip.send(dep.into()).ok();
            progress.subdependencies.send(dep.into()).ok();
            progress.integrity.send(dep.into()).ok();
            Ok(GitLockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .git(&dep.git)
                .rev(commit)
                .build()
                .into())
        }
    }
}

/// Install subdependencies of a dependency.
///
/// This function checks for a `.gitmodules` file in the dependency directory and clones the
/// submodules if it exists. If a `soldeer.toml` file is found, the soldeer dependencies are
/// installed with a call to `forge soldeer install`. If the dependency has a `foundry.toml` file
/// with a `dependencies` table, the soldeer dependencies are installed as well.
///
/// TODO: this function should install soldeer deps without calling to forge or the soldeer binary.
async fn install_subdependencies(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_path_buf();
    let gitmodules_path = path.join(".gitmodules");
    if fs::metadata(&gitmodules_path).await.is_ok() {
        // clone submodules
        run_git_command(&["submodule", "update", "--init", "--recursive"], Some(&path)).await?;
    }
    // if there is a soldeer.toml file, install the soldeer deps
    let soldeer_config_path = path.join("soldeer.toml");
    if fs::metadata(&soldeer_config_path).await.is_ok() {
        // install subdependencies
        run_forge_command(&["soldeer", "install"], Some(&path)).await?;
        return Ok(());
    }
    // if soldeer deps are defined in the foundry.toml file, install them
    let foundry_path = path.join("foundry.toml");
    if let Ok(contents) = fs::read_to_string(&foundry_path).await {
        if let Ok(doc) = contents.parse::<DocumentMut>() {
            if doc.contains_table("dependencies") {
                run_forge_command(&["soldeer", "install"], Some(&path)).await?;
            }
        }
    }
    Ok(())
}

/// Check the integrity of an HTTP dependency.
///
/// This function hashes the contents of the dependency directory and compares it with the lockfile
/// entry.
async fn check_http_dependency(
    lock: &HttpLockEntry,
    deps: impl AsRef<Path>,
) -> Result<DependencyStatus> {
    let path = lock.install_path(deps);
    if fs::metadata(&path).await.is_err() {
        return Ok(DependencyStatus::Missing);
    }
    let current_hash = tokio::task::spawn_blocking({
        let path = path.clone();
        move || hash_folder(&path)
    })
    .await?
    .map_err(|e| InstallError::IOError { path: path.to_path_buf(), source: e })?;
    if current_hash.to_string() != lock.integrity {
        debug!(path:?, expected = lock.integrity, computed = current_hash.0; "integrity checksum mismatch");
        return Ok(DependencyStatus::FailedIntegrity);
    }
    Ok(DependencyStatus::Installed)
}

/// Check the integrity of a git dependency.
///
/// This function checks that the dependency is a git repository and that the current commit is the
/// one specified in the lockfile entry.
async fn check_git_dependency(
    lock: &GitLockEntry,
    deps: impl AsRef<Path>,
) -> Result<DependencyStatus> {
    let path = lock.install_path(deps);
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
        Ok(top_level) => {
            // stdout contains the path twice, we only keep the first item
            PathBuf::from(top_level.split_whitespace().next().unwrap_or_default())
        }
        Err(_) => {
            // error getting the top level directory, assume the directory is not a git repository
            debug!(path:?; "`git rev-parse --show-toplevel` failed");
            return Ok(DependencyStatus::Missing);
        }
    };
    let top_level = top_level.to_slash_lossy();
    // compare the top level directory to the install path

    let absolute_path = canonicalize(&path)
        .await
        .map_err(|e| InstallError::IOError { path: path.clone(), source: e })?;
    if top_level.trim() != absolute_path.to_slash_lossy() {
        // the top level directory is not the install path, assume the directory is not a git
        // repository
        debug!(path:?; "dependency's toplevel dir is outside of dependency folder: not a git repo");
        return Ok(DependencyStatus::Missing);
    }
    // for git dependencies, the `rev` field holds the commit hash
    match run_git_command(&["diff", "--exit-code", &lock.rev], Some(&path)).await {
        Ok(_) => Ok(DependencyStatus::Installed),
        Err(_) => {
            debug!(path:?, rev = lock.rev; "git repo has non-empty diff compared to lockfile rev");
            Ok(DependencyStatus::FailedIntegrity)
        }
    }
}

/// Reset a git dependency to the commit specified in the lockfile entry.
///
/// This function runs `git reset --hard <commit>` and `git clean -fd` in the git dependency's
/// directory.
async fn reset_git_dependency(lock: &GitLockEntry, deps: impl AsRef<Path>) -> Result<()> {
    let path = lock.install_path(deps);
    run_git_command(&["reset", "--hard", &lock.rev], Some(&path)).await?;
    run_git_command(&["clean", "-fd"], Some(&path)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GitDependency, HttpDependency};
    use mockito::{Matcher, Server, ServerGuard};
    use temp_env::async_with_vars;
    use testdir::testdir;

    async fn mock_api_server() -> ServerGuard {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[{"created_at":"2024-08-06T17:31:25.751079Z","deleted":false,"downloads":3389,"id":"660132e6-4902-4804-8c4b-7cae0a648054","internal_name":"forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","version":"1.9.2"},{"created_at":"2024-07-03T14:44:59.729623Z","deleted":false,"downloads":5290,"id":"fa5160fc-ba7b-40fd-8e99-8becd6dadbe4","internal_name":"forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","version":"1.9.1"},{"created_at":"2024-07-03T14:44:58.148723Z","deleted":false,"downloads":21,"id":"b463683a-c4b4-40bf-b707-1c4eb343c4d2","internal_name":"forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","version":"1.9.0"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;
        let data2 = r#"{"data":[{"created_at":"2024-08-06T17:31:25.751079Z","deleted":false,"downloads":3391,"id":"660132e6-4902-4804-8c4b-7cae0a648054","internal_name":"forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","version":"1.9.2"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision-cli")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data2)
            .create_async()
            .await;
        server
    }

    #[tokio::test]
    async fn test_check_http_dependency() {
        let lock = HttpLockEntry::builder()
            .name("lib1")
            .version("1.0.0")
            .url("https://example.com/zip.zip")
            .checksum("")
            .integrity("beef")
            .build();
        let dir = testdir!();
        let path = dir.join("lib1-1.0.0");
        fs::create_dir(&path).await.unwrap();
        fs::write(path.join("test.txt"), "foobar").await.unwrap();
        let res = check_http_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::FailedIntegrity);

        let lock = HttpLockEntry::builder()
            .name("lib2")
            .version("1.0.0")
            .url("https://example.com/zip.zip")
            .checksum("")
            .integrity("")
            .build();
        let res = check_http_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::Missing);

        let hash = hash_folder(&path).unwrap();
        let lock = HttpLockEntry::builder()
            .name("lib1")
            .version("1.0.0")
            .url("https://example.com/zip.zip")
            .checksum("")
            .integrity(hash.to_string())
            .build();
        let res = check_http_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::Installed);
    }

    #[tokio::test]
    async fn test_check_git_dependency() {
        // happy path
        let dir = testdir!();
        let path = &dir.join("test-repo-1.0.0");
        let rev = clone_repo("https://github.com/beeb/test-repo.git", None, &path).await.unwrap();
        let lock =
            GitLockEntry::builder().name("test-repo").version("1.0.0").git("").rev(rev).build();
        let res = check_git_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::Installed);

        // replace contents of existing file, diff is not empty
        fs::write(path.join("foo.txt"), "foo").await.unwrap();
        let res = check_git_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::FailedIntegrity);

        // wrong commit is checked out
        let lock = GitLockEntry::builder()
            .name("test-repo")
            .version("1.0.0")
            .git("")
            .rev("78c2f6a1a54db26bab6c3f501854a1564eb3707f")
            .build();
        let res = check_git_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::FailedIntegrity);

        // missing folder
        let lock = GitLockEntry::builder().name("lib1").version("1.0.0").git("").rev("").build();
        let res = check_git_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::Missing);

        // remove .git folder -> not a git repo
        let lock =
            GitLockEntry::builder().name("test-repo").version("1.0.0").git("").rev("").build();
        fs::remove_dir_all(path.join(".git")).await.unwrap();
        let res = check_git_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), DependencyStatus::Missing);
    }

    #[tokio::test]
    async fn test_reset_git_dependency() {
        let dir = testdir!();
        let path = &dir.join("test-repo-1.0.0");
        clone_repo("https://github.com/beeb/test-repo.git", None, &path).await.unwrap();
        let lock = GitLockEntry::builder()
            .name("test-repo")
            .version("1.0.0")
            .git("")
            .rev("78c2f6a1a54db26bab6c3f501854a1564eb3707f")
            .build();
        let test = path.join("test.txt");
        fs::write(&test, "foobar").await.unwrap();
        let res = reset_git_dependency(&lock, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        // non checked-in file
        assert!(fs::metadata(test).await.is_err());
        // file that is in `main` but not in `78c2f6a`
        assert!(fs::metadata(path.join("foo.txt")).await.is_err());
        let commit = run_git_command(&["rev-parse", "--verify", "HEAD"], Some(path))
            .await
            .unwrap()
            .trim()
            .to_string();
        assert_eq!(commit, "78c2f6a1a54db26bab6c3f501854a1564eb3707f");
    }

    #[tokio::test]
    async fn test_install_dependency_inner_http() {
        let dir = testdir!();
        let install: InstallInfo = HttpInstallInfo::builder().name("test").version("1.0.0").url("https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip").checksum("94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468").build().into();
        let (progress, _) = InstallProgress::new();
        let res = install_dependency_inner(&install, &dir, false, progress).await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), "test");
        assert_eq!(lock.version(), "1.0.0");
        let lock = lock.as_http().unwrap();
        assert_eq!(lock.url, "https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip");
        assert_eq!(
            lock.checksum,
            "94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468"
        );
        let hash = hash_folder(&dir).unwrap();
        assert_eq!(lock.integrity, hash.to_string());
    }

    #[tokio::test]
    async fn test_install_dependency_inner_git() {
        let dir = testdir!();
        let install: InstallInfo = GitInstallInfo::builder()
            .name("test")
            .version("1.0.0")
            .git("https://github.com/beeb/test-repo.git")
            .build()
            .into();
        let (progress, _) = InstallProgress::new();
        let res = install_dependency_inner(&install, &dir, false, progress).await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), "test");
        assert_eq!(lock.version(), "1.0.0");
        let lock = lock.as_git().unwrap();
        assert_eq!(lock.git, "https://github.com/beeb/test-repo.git");
        assert_eq!(lock.rev, "d5d72fa135d28b2e8307650b3ea79115183f2406");
        assert!(dir.join(".git").exists());
    }

    #[tokio::test]
    async fn test_install_dependency_inner_git_rev() {
        let dir = testdir!();
        let install: InstallInfo = GitInstallInfo::builder()
            .name("test")
            .version("1.0.0")
            .git("https://github.com/beeb/test-repo.git")
            .identifier(GitIdentifier::from_rev("78c2f6a1a54db26bab6c3f501854a1564eb3707f"))
            .build()
            .into();
        let (progress, _) = InstallProgress::new();
        let res = install_dependency_inner(&install, &dir, false, progress).await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), "test");
        assert_eq!(lock.version(), "1.0.0");
        let lock = lock.as_git().unwrap();
        assert_eq!(lock.git, "https://github.com/beeb/test-repo.git");
        assert_eq!(lock.rev, "78c2f6a1a54db26bab6c3f501854a1564eb3707f");
        assert!(dir.join(".git").exists());
    }

    #[tokio::test]
    async fn test_install_dependency_inner_git_branch() {
        let dir = testdir!();
        let install: InstallInfo = GitInstallInfo::builder()
            .name("test")
            .version("1.0.0")
            .git("https://github.com/beeb/test-repo.git")
            .identifier(GitIdentifier::from_branch("dev"))
            .build()
            .into();
        let (progress, _) = InstallProgress::new();
        let res = install_dependency_inner(&install, &dir, false, progress).await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), "test");
        assert_eq!(lock.version(), "1.0.0");
        let lock = lock.as_git().unwrap();
        assert_eq!(lock.git, "https://github.com/beeb/test-repo.git");
        assert_eq!(lock.rev, "8d903e557e8f1b6e62bde768aa456d4ddfca72c4");
        assert!(dir.join(".git").exists());
    }

    #[tokio::test]
    async fn test_install_dependency_inner_git_tag() {
        let dir = testdir!();
        let install: InstallInfo = GitInstallInfo::builder()
            .name("test")
            .version("1.0.0")
            .git("https://github.com/beeb/test-repo.git")
            .identifier(GitIdentifier::from_tag("v0.1.0"))
            .build()
            .into();
        let (progress, _) = InstallProgress::new();
        let res = install_dependency_inner(&install, &dir, false, progress).await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), "test");
        assert_eq!(lock.version(), "1.0.0");
        let lock = lock.as_git().unwrap();
        assert_eq!(lock.git, "https://github.com/beeb/test-repo.git");
        assert_eq!(lock.rev, "78c2f6a1a54db26bab6c3f501854a1564eb3707f");
        assert!(dir.join(".git").exists());
    }

    #[tokio::test]
    async fn test_install_dependency_registry() {
        let server = mock_api_server().await;
        let dir = testdir!();
        let dep = HttpDependency::builder().name("forge-std").version_req("1.9.2").build().into();
        let (progress, _) = InstallProgress::new();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            install_dependency(&dep, None, &dir, None, false, progress),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), dep.name());
        assert_eq!(lock.version(), dep.version_req());
        let lock = lock.as_http().unwrap();
        assert_eq!(&lock.url, "https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip");
        assert_eq!(
            lock.checksum,
            "20fd008c7c69b6c737cc0284469d1c76497107bc3e004d8381f6d8781cb27980"
        );
        let hash = hash_folder(lock.install_path(&dir)).unwrap();
        assert_eq!(lock.integrity, hash.to_string());
    }

    #[tokio::test]
    async fn test_install_dependency_registry_compatible() {
        let server = mock_api_server().await;
        let dir = testdir!();
        let dep = HttpDependency::builder().name("forge-std").version_req("^1.9.0").build().into();
        let (progress, _) = InstallProgress::new();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            install_dependency(&dep, None, &dir, None, false, progress),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), dep.name());
        assert_eq!(lock.version(), "1.9.2");
        let lock = lock.as_http().unwrap();
        assert_eq!(&lock.url, "https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip");
        let hash = hash_folder(lock.install_path(&dir)).unwrap();
        assert_eq!(lock.integrity, hash.to_string());
    }

    #[tokio::test]
    async fn test_install_dependency_http() {
        let dir = testdir!();
        let dep = HttpDependency::builder().name("test").version_req("1.0.0").url("https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip").build().into();
        let (progress, _) = InstallProgress::new();
        let res = install_dependency(&dep, None, &dir, None, false, progress).await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), dep.name());
        assert_eq!(lock.version(), dep.version_req());
        let lock = lock.as_http().unwrap();
        assert_eq!(&lock.url, dep.url().unwrap());
        assert_eq!(
            lock.checksum,
            "94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468"
        );
        let hash = hash_folder(lock.install_path(&dir)).unwrap();
        assert_eq!(lock.integrity, hash.to_string());
    }

    #[tokio::test]
    async fn test_install_dependency_git() {
        let dir = testdir!();
        let dep = GitDependency::builder()
            .name("test")
            .version_req("1.0.0")
            .git("https://github.com/beeb/test-repo.git")
            .build()
            .into();
        let (progress, _) = InstallProgress::new();
        let res = install_dependency(&dep, None, &dir, None, false, progress).await;
        assert!(res.is_ok(), "{res:?}");
        let lock = res.unwrap();
        assert_eq!(lock.name(), dep.name());
        assert_eq!(lock.version(), dep.version_req());
        let lock = lock.as_git().unwrap();
        assert_eq!(&lock.git, dep.url().unwrap());
        assert_eq!(lock.rev, "d5d72fa135d28b2e8307650b3ea79115183f2406");
    }
}
