//! Install dependencies.
//!
//! This module contains functions to install dependencies from the config object or from the
//! lockfile. Dependencies can be installed in parallel.
use crate::{
    config::{
        Dependency, GitIdentifier, HttpDependency, Paths, detect_config_location, read_config_deps,
        read_soldeer_config,
    },
    download::{clone_repo, delete_dependency_files, download_file, unzip_file},
    errors::{ConfigError, InstallError, LockError},
    lock::{
        GitLockEntry, HttpLockEntry, Integrity, LockEntry, PrivateLockEntry, forge,
        format_install_path, generate_lockfile_contents, read_lockfile, write_lockfile,
    },
    registry::{DownloadUrl, get_dependency_url_remote, get_latest_supported_version},
    utils::{
        IntegrityChecksum, canonicalize, hash_file, hash_folder, is_symlink, run_git_command,
        sanitize_filename,
    },
};
use derive_more::derive::Display;
use log::{debug, info, warn};
use path_slash::PathBufExt as _;
use sha2::{Digest as _, Sha256};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fmt,
    future::Future,
    ops::Deref,
    path::{Component, Path, PathBuf},
    pin::Pin,
};
use tokio::{fs, sync::mpsc, task::JoinSet};

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

    /// The URL from which the zip file will be downloaded.
    url: String,

    /// The checksum of the downloaded zip file, if available (e.g. from the lockfile)
    checksum: Option<String>,

    /// An optional relative path to the project's root within the zip file.
    ///
    /// The project root is where the soldeer.toml or foundry.toml resides. If no path is provided,
    /// then the zip's root must contain a Soldeer config.
    project_root: Option<PathBuf>,

    /// Whether a single wrapping root directory should be stripped from the zip archive during
    /// extraction.
    ///
    /// This is `true` for custom URL dependencies, which typically point to source archives
    /// wrapping the contents in a root folder (e.g. GitHub-generated zips), and `false` for
    /// registry packages, where the archive contains the published files directly and any
    /// top-level directory is part of the package's layout.
    #[builder(default = true)]
    zip_strip_root: bool,
}

impl fmt::Display for HttpInstallInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // since the version is an exact version number, we use a dash and not a tilde
        write!(f, "{}-{}", self.name, self.version)
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

    /// An optional relative path to the project's root within the repository.
    ///
    /// The project root is where the soldeer.toml or foundry.toml resides. If no path is provided,
    /// then the repo's root must contain a Soldeer config.
    project_root: Option<PathBuf>,
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

    /// Installation information for a private dependency.
    Private(HttpInstallInfo),
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

impl InstallInfo {
    async fn from_lock(
        lock: LockEntry,
        project_root: Option<PathBuf>,
        zip_strip_root: bool,
    ) -> Result<Self> {
        match lock {
            LockEntry::Http(lock) => Ok(HttpInstallInfo {
                name: lock.name,
                version: lock.version,
                url: lock.url,
                checksum: Some(lock.checksum),
                project_root,
                zip_strip_root,
            }
            .into()),
            LockEntry::Git(lock) => Ok(GitInstallInfo {
                name: lock.name,
                version: lock.version,
                git: lock.git,
                identifier: Some(GitIdentifier::from_rev(lock.rev)),
                project_root,
            }
            .into()),
            LockEntry::Private(lock) => {
                // need to retrieve a signed download URL from the registry
                let download = get_dependency_url_remote(
                    &HttpDependency::builder()
                        .name(&lock.name)
                        .version_req(&lock.version)
                        .build()
                        .into(),
                    &lock.version,
                )
                .await?;
                Ok(Self::Private(HttpInstallInfo {
                    name: lock.name,
                    version: lock.version,
                    url: download.url,
                    checksum: Some(lock.checksum),
                    project_root,
                    // private dependencies always come from the registry, where archives never
                    // have a wrapping root directory
                    zip_strip_root: false,
                }))
            }
        }
    }
}

/// Git submodule information
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
struct Submodule {
    url: String,
    path: String,
    branch: Option<String>,
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
    validate_dependency_path_collisions(dependencies)?;
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

/// Install a list of dependencies sequentially.
///
/// This function can be used inside another tokio task to avoid spawning more tasks, useful for
/// recursive install. For each dep, checks the integrity of the dependency if found on disk,
/// downloads the dependency (zip file or cloning repo) if not already present, unzips the zip file
/// if necessary, installs sub-dependencies and generates the lockfile entry.
pub async fn install_dependencies_sequential(
    dependencies: &[Dependency],
    locks: &[LockEntry],
    deps: impl AsRef<Path> + Clone,
    recursive_deps: bool,
    progress: InstallProgress,
) -> Result<Vec<LockEntry>> {
    validate_dependency_path_collisions(dependencies)?;
    let mut results = Vec::new();
    for dep in dependencies {
        debug!(dep:% = dep; "installing dependency sequentially");
        let lock = locks.iter().find(|l| l.name() == dep.name());
        results.push(
            install_dependency(dep, lock, deps.clone(), None, recursive_deps, progress.clone())
                .await?,
        );
        debug!(dep:% = dep; "sequential install finished");
    }
    debug!("all sequential installs have finished");
    Ok(results)
}

pub fn validate_dependency_path_collisions(dependencies: &[Dependency]) -> Result<()> {
    let mut paths = HashMap::<String, String>::new();
    for dependency in dependencies {
        let path = sanitize_filename(dependency.name());
        if let Some(other) = paths.insert(path.clone(), dependency.name().to_string()) &&
            other != dependency.name()
        {
            return Err(InstallError::PathCollision {
                dependency: dependency.name().to_string(),
                other,
                path,
            });
        }
    }
    Ok(())
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
                if recursive_deps {
                    let project_root = dependency.project_root();
                    install_subdependencies(lock.install_path(&deps), project_root.as_ref())
                        .await?;
                }
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
            &InstallInfo::from_lock(
                lock.clone(),
                dependency.project_root(),
                // custom URLs point to source archives which may wrap the contents in a root
                // folder; registry archives never do
                dependency.url().is_some(),
            )
            .await?,
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

        let (download, version) = match dependency.url() {
            // for git dependencies and http dependencies which have a custom url, we use the
            // version requirement string as version, because in that case a version requirement has
            // little sense (we can't automatically bump the version)
            Some(url) => (
                DownloadUrl { url: url.clone(), private: false },
                dependency.version_req().to_string(),
            ),
            None => {
                let version = match force_version {
                    Some(v) => v,
                    None => get_latest_supported_version(dependency).await?,
                };
                (get_dependency_url_remote(dependency, &version).await?, version)
            }
        };
        debug!(dep:% = dependency, version; "resolved version");
        debug!(dep:% = dependency, url:? = download; "resolved download URL");
        // indicate that we have retrieved the version number
        progress.versions.send(dependency.into()).ok();

        // custom URLs point to source archives which may wrap the contents in a root folder;
        // registry archives never do
        let zip_strip_root = dependency.url().is_some();
        let info = match &dependency {
            Dependency::Http(dep) => {
                if download.private {
                    InstallInfo::Private(
                        HttpInstallInfo::builder()
                            .name(&dep.name)
                            .version(&version)
                            .url(download.url)
                            .zip_strip_root(zip_strip_root)
                            .build(),
                    )
                } else {
                    HttpInstallInfo::builder()
                        .name(&dep.name)
                        .version(&version)
                        .url(download.url)
                        .zip_strip_root(zip_strip_root)
                        .build()
                        .into()
                }
            }
            Dependency::Git(dep) => GitInstallInfo::builder()
                .name(&dep.name)
                .version(&version)
                .git(download.url)
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
        LockEntry::Private(lock) => check_http_dependency(lock, deps).await,
        LockEntry::Git(lock) => check_git_dependency(lock, deps).await,
    }
}

/// Ensure that the dependencies directory exists.
///
/// If the directory does not exist, it will be created.
pub fn ensure_dependencies_dir(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if is_symlink(path)
        .map_err(|e| InstallError::IOError { path: path.to_path_buf(), source: e })?
    {
        return Err(InstallError::IOError {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "dependencies path must not be a symlink",
            ),
        });
    }
    if !path.exists() {
        debug!(path:?; "dependencies dir doesn't exist, creating it");
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
            let (zip_integrity, integrity) =
                install_http_dependency(dep, path, subdependencies, progress).await?;
            Ok(HttpLockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .url(&dep.url)
                .checksum(zip_integrity.to_string())
                .integrity(integrity.to_string())
                .build()
                .into())
        }
        InstallInfo::Private(dep) => {
            let (zip_integrity, integrity) =
                install_http_dependency(dep, path, subdependencies, progress).await?;
            Ok(PrivateLockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
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
                install_subdependencies(&path, dep.project_root.as_ref()).await?;
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
/// submodules if it exists. If a valid Soldeer config is found at the project root (optionally a
/// sub-dir of the dependency folder), the soldeer dependencies are installed.
fn install_subdependencies(
    path: impl AsRef<Path>,
    project_root: Option<&PathBuf>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
    let path = path.as_ref().to_path_buf();
    Box::pin(async move {
        let gitmodules_path = path.join(".gitmodules");
        if fs::metadata(&gitmodules_path).await.is_ok() {
            debug!(path:?; "found .gitmodules, installing subdependencies with git");
            if fs::metadata(path.join(".git")).await.is_ok() {
                debug!(path:?; "subdependency contains .git directory, cloning submodules");
                run_git_command(&["submodule", "update", "--init"], Some(&path)).await?;
                // we need to recurse into each of the submodules to ensure any soldeer sub-deps
                // of those are also installed
                let submodules = get_submodules(&path).await?;
                let mut set = JoinSet::new();
                for (_, submodule) in submodules {
                    let sub_path = path.join(submodule.path);
                    debug!(sub_path:?; "recursing into the git submodule");
                    set.spawn(async move { install_subdependencies(sub_path, None).await });
                }
                while let Some(res) = set.join_next().await {
                    res??;
                }
            } else {
                debug!(path:?; "subdependency has git submodules configuration but is not a git repository");
                let submodule_paths = reinit_submodules(&path).await?;
                // we need to recurse into each of the submodules to ensure any soldeer sub-deps
                // of those are also installed
                let mut set = JoinSet::new();
                for sub_path in submodule_paths {
                    debug!(sub_path:?; "recursing into the git submodule");
                    set.spawn(async move { install_subdependencies(sub_path, None).await });
                }
                while let Some(res) = set.join_next().await {
                    res??;
                }
            }
        }
        // if there's a suitable soldeer config, install the soldeer deps
        let path = get_subdependency_root(path, project_root).await?;
        if detect_config_location(&path).is_some() {
            // install subdependencies
            debug!(path:?; "found soldeer config, installing subdependencies");
            install_subdependencies_inner(Paths::from_root(path)?).await?;
        }
        Ok(())
    })
}

/// Inner logic for installing subdependencies at a given path.
///
/// This is a similar implementation to the one found in `soldeer_commands` but
/// simplified.
async fn install_subdependencies_inner(paths: Paths) -> Result<()> {
    let config = read_soldeer_config(&paths.config)?;
    ensure_dependencies_dir(&paths.dependencies)?;
    let (dependencies, _) = read_config_deps(&paths.config)?;
    let lockfile = read_lockfile(&paths.lock)?;
    let (progress, _) = InstallProgress::new(); // not used at the moment
    let new_locks = install_dependencies(
        &dependencies,
        &lockfile.entries,
        &paths.dependencies,
        config.recursive_deps,
        progress,
    )
    .await?;
    write_lockfile(&generate_lockfile_contents(new_locks), &paths.lock)?;
    Ok(())
}

/// Download and unzip an HTTP dependency
async fn install_http_dependency(
    dep: &HttpInstallInfo,
    path: impl AsRef<Path>,
    subdependencies: bool,
    progress: InstallProgress,
) -> Result<(IntegrityChecksum, IntegrityChecksum)> {
    let path = path.as_ref();
    let parent = path.parent().expect("dependency install path should have a parent");
    let dir_name = path.file_name().expect("dependency install path should have a file name");
    let zip_path =
        download_file(&dep.url, parent, &format!("{}-{}", dep.name, dep.version)).await?;
    progress.downloads.send(dep.into()).ok();

    // the archive is extracted next to its final location and only moved into
    // place once complete, so an interrupted install can never leave a partial
    // dependency behind
    let staging_path = parent.join(staging_dir_name(dir_name));
    fs::remove_dir_all(&staging_path).await.ok(); // ignore error if folder doesn't exist
    fs::create_dir(&staging_path)
        .await
        .map_err(|e| InstallError::IOError { path: staging_path.clone(), source: e })?;

    let result = async {
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
        unzip_file(&zip_path, &staging_path, dep.zip_strip_root).await?;
        progress.unzip.send(dep.into()).ok();

        if subdependencies {
            debug!(dep:% = dep; "installing subdependencies");
            install_subdependencies(&staging_path, dep.project_root.as_ref()).await?;
            debug!(dep:% = dep; "finished installing subdependencies");
        }
        progress.subdependencies.send(dep.into()).ok();

        let integrity = tokio::task::spawn_blocking({
            let path = staging_path.clone();
            move || hash_folder(&path)
        })
        .await?
        .map_err(|e| InstallError::IOError { path: staging_path.clone(), source: e })?;
        debug!(dep:% = dep, checksum = integrity.0; "integrity checksum computed");
        progress.integrity.send(dep.into()).ok();
        fs::rename(&staging_path, path)
            .await
            .map_err(|e| InstallError::IOError { path: path.to_path_buf(), source: e })?;
        Ok((zip_integrity, integrity))
    }
    .await;

    if result.is_err() {
        fs::remove_dir_all(&staging_path).await.ok();
        fs::remove_file(&zip_path).await.ok();
    }
    result
}

/// Name of the staging directory used while extracting a dependency into `dir_name`.
///
/// The name is derived from the install directory name, so a leftover staging
/// directory from an interrupted install is reclaimed by the next attempt.
///
/// It is deliberately short, Windows caps paths at 260 characters, and
/// everything extracted into the staging directory has to fit under that limit
fn staging_dir_name(dir_name: &OsStr) -> String {
    let digest = Sha256::digest(dir_name.as_encoded_bytes());
    format!(".{}", const_hex::encode(&digest[..4]))
}

/// Retrieve a map of git submodules for a path by looking at the `.gitmodules` file.
async fn get_submodules(path: &PathBuf) -> Result<HashMap<String, Submodule>> {
    let submodules_config =
        run_git_command(&["config", "-f", ".gitmodules", "-l"], Some(path)).await?;
    let mut submodules = HashMap::<String, Submodule>::new();
    for config_line in submodules_config.trim().lines() {
        let (item, value) = config_line.split_once('=').expect("config format should be valid");
        let Some(item) = item.strip_prefix("submodule.") else {
            continue;
        };
        let (submodule_name, item_name) =
            item.rsplit_once('.').expect("config format should be valid");
        let entry = submodules.entry(submodule_name.to_string()).or_default();
        match item_name {
            "path" => entry.path = value.to_string(),
            "url" => entry.url = value.to_string(),
            "branch" => entry.branch = Some(value.to_string()),
            _ => {}
        }
    }
    Ok(submodules)
}

/// Re-add submodules found in a `.gitmodules` when the folder has to be re-initialized as a git
/// repo.
///
/// The file is parsed, and each module is added again with `git submodule add`.
async fn reinit_submodules(path: &PathBuf) -> Result<Vec<PathBuf>> {
    debug!(path:?; "running git init");
    run_git_command(&["init"], Some(path)).await?;
    let submodules = get_submodules(path).await?;
    debug!(submodules:?, path:?; "got submodules config");
    let mut foundry_lock = forge::Lockfile::new(path);
    if foundry_lock.read().is_ok() {
        debug!(path:?; "foundry lockfile exists");
    }
    let mut out = Vec::new();
    for (submodule_name, submodule) in submodules {
        let submodule_path = validate_submodule_path(&submodule.path)?;
        // make sure to remove the path if it already exists
        let dest_path = path.join(&submodule_path);
        fs::remove_dir_all(&dest_path).await.ok(); // ignore error if folder doesn't exist
        let mut args = vec!["submodule", "add", "-f", "--name", &submodule_name];
        if let Some(branch) = &submodule.branch {
            args.push("-b");
            args.push(branch);
        }
        args.push(&submodule.url);
        let submodule_path = submodule_path.to_string_lossy().to_string();
        args.push(&submodule_path);
        run_git_command(args, Some(path)).await?;
        if let Some(
            forge::DepIdentifier::Branch { rev, .. } |
            forge::DepIdentifier::Tag { rev, .. } |
            forge::DepIdentifier::Rev { rev },
        ) = foundry_lock.get(Path::new(&submodule_path))
        {
            debug!(submodule_name, path:?; "found corresponding item in foundry lockfile");
            run_git_command(["checkout", rev], Some(&dest_path)).await?;
            debug!(submodule_name, path:?; "submodule checked out at {rev}");
        }
        debug!(submodule_name, path:?; "added submodule");
        out.push(path.join(submodule_path));
    }
    Ok(out)
}

fn validate_submodule_path(path: &str) -> Result<PathBuf> {
    let path_ref = Path::new(path);
    let invalid = path_ref.as_os_str().is_empty() ||
        path_ref.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        });
    if invalid {
        return Err(InstallError::InvalidSubmodulePath(path.to_string()));
    }
    let normalized: PathBuf =
        path_ref.components().filter(|component| *component != Component::CurDir).collect();
    if normalized.as_os_str().is_empty() {
        return Err(InstallError::InvalidSubmodulePath(path.to_string()));
    }
    Ok(normalized)
}

/// Check the integrity of an HTTP dependency.
///
/// This function hashes the contents of the dependency directory and compares it with the lockfile
/// entry.
async fn check_http_dependency(
    lock: &impl Integrity,
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
    let Some(integrity) = lock.integrity() else {
        return Err(LockError::MissingField {
            field: "integrity".to_string(),
            dep: path.to_string_lossy().to_string(),
        }
        .into());
    };
    if &current_hash.to_string() != integrity {
        debug!(path:?, expected = integrity, computed = current_hash.0; "integrity checksum mismatch");
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

/// Normalize and check the path to a subdependency's project root.
///
/// The combination of the subdependency path with the relative path to the root must be at or below
/// the level of the subdependency, to avoid directory traversal.
async fn get_subdependency_root(
    subdependency_path: PathBuf,
    relative_root: Option<&PathBuf>,
) -> Result<PathBuf> {
    let path = match relative_root {
        Some(relative_root) => {
            let tentative_path =
                canonicalize(subdependency_path.join(relative_root)).await.map_err(|_| {
                    InstallError::ConfigError(ConfigError::InvalidProjectRoot {
                        project_root: relative_root.to_owned(),
                        dep_path: subdependency_path.clone(),
                    })
                })?;
            // final path must be below the dependency's folder
            let path_with_slashes = subdependency_path.to_slash_lossy().into_owned();
            if !tentative_path.to_slash_lossy().starts_with(&path_with_slashes) {
                return Err(InstallError::ConfigError(ConfigError::InvalidProjectRoot {
                    project_root: relative_root.to_owned(),
                    dep_path: subdependency_path.clone(),
                }));
            }
            tentative_path
        }
        None => subdependency_path,
    };
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{GitDependency, HttpDependency},
        lock::read_lockfile,
        push::zip_file,
        update::update_dependencies,
    };
    use mockito::{Matcher, Server, ServerGuard};
    use std::io::{Cursor, Write as _};
    use temp_env::async_with_vars;
    use testdir::testdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

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

    async fn mock_api_private() -> ServerGuard {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[{"created_at":"2025-09-28T12:36:09.526660Z","deleted":false,"downloads":0,"file_size":65083,"id":"0440c261-8cdf-4738-9139-c4dc7b0c7f3e","internal_name":"test-private/0_1_0_28-09-2025_12:36:08_test-private.zip","private":true,"project_id":"14f419e7-2d64-49e4-86b9-b44b36627786","uploader":"bf8e75f4-0c36-4bcb-a23b-2682df92f176","url":"https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip","version":"0.1.0"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;
        let data2 = r#"{"data":[{"created_at":"2025-09-28T12:36:09.526660Z","deleted":false,"id":"0440c261-8cdf-4738-9139-c4dc7b0c7f3e","internal_name":"test-private/0_1_0_28-09-2025_12:36:08_test-private.zip","private":true,"project_id":"14f419e7-2d64-49e4-86b9-b44b36627786","url":"https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip","version":"0.1.0"}],"status":"success"}"#;
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
        assert_eq!(
            lock.url,
            "https://github.com/mario-eth/soldeer/archive/8585a7ec85a29889cec8d08f4770e15ec4795943.zip"
        );
        assert_eq!(
            lock.checksum,
            "94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468"
        );
        let hash = hash_folder(&dir).unwrap();
        assert_eq!(lock.integrity, hash.to_string());
    }

    #[test]
    fn test_validate_submodule_path_rejects_parent_directory() {
        assert!(matches!(
            validate_submodule_path("../victim-1.0.0"),
            Err(InstallError::InvalidSubmodulePath(path)) if path == "../victim-1.0.0"
        ));
        assert!(matches!(
            validate_submodule_path("lib/../../victim-1.0.0"),
            Err(InstallError::InvalidSubmodulePath(_))
        ));
    }

    #[test]
    fn test_validate_submodule_path_rejects_absolute_path() {
        assert!(matches!(
            validate_submodule_path("/tmp/victim-1.0.0"),
            Err(InstallError::InvalidSubmodulePath(path)) if path == "/tmp/victim-1.0.0"
        ));
        #[cfg(windows)]
        {
            assert!(matches!(
                validate_submodule_path("C:\\victim-1.0.0"),
                Err(InstallError::InvalidSubmodulePath(_))
            ));
            assert!(matches!(
                validate_submodule_path("\\victim-1.0.0"),
                Err(InstallError::InvalidSubmodulePath(_))
            ));
        }
    }

    #[test]
    fn test_validate_submodule_path_normalizes_curdir() {
        assert_eq!(validate_submodule_path("./lib/dep").unwrap(), PathBuf::from("lib").join("dep"));
        assert!(matches!(validate_submodule_path("."), Err(InstallError::InvalidSubmodulePath(_))));
    }

    #[tokio::test]
    async fn test_install_http_dependency_cleans_partial_tree_on_extract_error() {
        let dir = testdir!();
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("src/Contract.sol", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"contract Contract {}\n").unwrap();
        // a file entry nested under another file entry cannot be extracted
        zip.start_file("src/Contract.sol/nested", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"invalid").unwrap();
        let archive = zip.finish().unwrap().into_inner();

        let mut server = Server::new_async().await;
        server.mock("GET", "/file.zip").with_body(archive).create_async().await;
        let dep = HttpInstallInfo::builder()
            .name("foo")
            .version("2.0.0")
            .url(format!("{}/file.zip", server.url()))
            .build();
        let install_path = dir.join("foo-2.0.0");
        // leftover staging dir from a previous interrupted install
        let staging_path = dir.join(staging_dir_name(OsStr::new("foo-2.0.0")));
        fs::create_dir(&staging_path).await.unwrap();
        fs::write(staging_path.join("stale.txt"), "stale").await.unwrap();
        let (progress, _) = InstallProgress::new();

        let res = install_http_dependency(&dep, &install_path, false, progress).await;
        assert!(res.is_err(), "{res:?}");
        assert!(!install_path.exists());
        assert!(!dir.join("foo-2.0.0.zip").exists());
        assert!(!staging_path.exists());
    }

    // a failed http update must not leave a partial tree behind: the lockfile still points at the
    // old version, and remappings resolve through a filesystem scan that would otherwise pick the
    // orphaned directory up
    #[tokio::test]
    async fn test_failed_update_leaves_no_orphan_tree() {
        let dir = testdir!();
        let deps = dir.join("dependencies");
        fs::create_dir(&deps).await.unwrap();

        // a valid v1 install, as if a previous `soldeer install` had succeeded
        let v1_path = deps.join("foo-1.0.0");
        fs::create_dir(&v1_path).await.unwrap();
        fs::write(v1_path.join("Contract.sol"), "contract V1 {}\n").await.unwrap();
        let v1_integrity = hash_folder(&v1_path).unwrap();

        // the v2 archive collides a file entry with a directory entry, so extraction fails partway
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("src/Contract.sol", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"contract Evil {}\n").unwrap();
        zip.start_file("src/Contract.sol/x", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"invalid").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let mut server = Server::new_async().await;
        server.mock("GET", "/v2.zip").with_body(archive).create_async().await;

        let dependency: Dependency = HttpDependency::builder()
            .name("foo")
            .version_req("2.0.0")
            .url(format!("{}/v2.zip", server.url()))
            .build()
            .into();
        let lock: LockEntry = HttpLockEntry::builder()
            .name("foo")
            .version("1.0.0")
            .url("https://example.com/v1.zip")
            .checksum("checksum")
            .integrity(v1_integrity.to_string())
            .build()
            .into();
        let (progress, _) = InstallProgress::new();

        let res =
            update_dependencies(std::slice::from_ref(&dependency), &[lock], &deps, false, progress)
                .await;
        assert!(res.is_err(), "{res:?}");
        assert!(!deps.join("foo-2.0.0").exists());
        assert!(dependency.install_path_sync(&deps).is_none());
        // the previously installed version is left untouched
        assert!(v1_path.join("Contract.sol").exists());
    }

    #[test]
    fn test_staging_dir_name_is_short_and_deterministic() {
        let dir_name = OsStr::new("@uniswap-permit2-1.0.0");
        let name = staging_dir_name(dir_name);
        assert_eq!(name, staging_dir_name(dir_name));
        assert_ne!(name, staging_dir_name(OsStr::new("@uniswap-permit2-1.0.1")));
        // staging must not lengthen paths, or deeply nested trees blow the windows 260 char limit
        assert!(name.len() <= dir_name.len(), "{name}");
        assert_eq!(Path::new(&name).components().count(), 1, "{name}");
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_dependencies_dir_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let dir = testdir!();
        let target = dir.join("target");
        let dependencies = dir.join("dependencies");
        std::fs::create_dir(&target).unwrap();
        symlink(&target, &dependencies).unwrap();

        let res = ensure_dependencies_dir(&dependencies);
        assert!(res.is_err(), "{res:?}");
        assert!(target.exists());
    }

    #[tokio::test]
    async fn test_installed_recursive_dependency_persists_nested_lockfile() {
        let dir = testdir!();
        let parent_path = dir.join("dependencies/parent-1.0.0");
        fs::create_dir_all(&parent_path).await.unwrap();
        fs::write(
            parent_path.join("soldeer.toml"),
            "[dependencies]\nchild = { version = \"1.0.0\", url = \"PLACEHOLDER\" }\n",
        )
        .await
        .unwrap();

        let child_root = dir.join("child-source");
        fs::create_dir(&child_root).await.unwrap();
        let child_file = child_root.join("Child.sol");
        fs::write(&child_file, "contract Child {}\n").await.unwrap();
        let archive = zip_file(&child_root, &[child_file], "child").unwrap();
        let mut server = Server::new_async().await;
        server.mock("GET", "/child.zip").with_body_from_file(&archive).create_async().await;
        fs::write(
            parent_path.join("soldeer.toml"),
            format!(
                "[dependencies]\nchild = {{ version = \"1.0.0\", url = \"{}/child.zip\" }}\n",
                server.url()
            ),
        )
        .await
        .unwrap();

        let parent_dependency: Dependency = HttpDependency::builder()
            .name("parent")
            .version_req("1.0.0")
            .url("https://example.com/parent.zip")
            .build()
            .into();
        let parent_integrity = hash_folder(&parent_path).unwrap();
        let parent_lock: LockEntry = HttpLockEntry::builder()
            .name("parent")
            .version("1.0.0")
            .url("https://example.com/parent.zip")
            .checksum("checksum")
            .integrity(parent_integrity.to_string())
            .build()
            .into();
        let (progress, _) = InstallProgress::new();

        let res = install_dependency(
            &parent_dependency,
            Some(&parent_lock),
            dir.join("dependencies"),
            None,
            true,
            progress,
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert!(parent_path.join("dependencies/child-1.0.0").exists());
        let nested_lock = read_lockfile(parent_path.join("soldeer.lock")).unwrap();
        assert_eq!(nested_lock.entries.len(), 1);
        assert_eq!(nested_lock.entries[0].name(), "child");
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
        assert_eq!(
            &lock.url,
            "https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip"
        );
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
        assert_eq!(
            &lock.url,
            "https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip"
        );
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

    #[tokio::test]
    async fn test_install_dependency_private() {
        let server = mock_api_private().await;
        let dir = testdir!();
        let dep =
            HttpDependency::builder().name("test-private").version_req("0.1.0").build().into();
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
        let lock = lock.as_private().unwrap();
        assert_eq!(
            lock.checksum,
            "94a73dbe106f48179ea39b00d42e5d4dd96fdc6252caa3a89ce7efdaec0b9468"
        );
        let hash = hash_folder(lock.install_path(&dir)).unwrap();
        assert_eq!(lock.integrity, hash.to_string());
    }

    #[tokio::test]
    async fn test_install_dependencies_rejects_sanitized_path_collision() {
        let dir = testdir!();
        let dependencies: Vec<Dependency> = vec![
            HttpDependency::builder()
                .name("foo/bar")
                .version_req("^1.0.0")
                .url("https://example.com/first.zip")
                .build()
                .into(),
            HttpDependency::builder()
                .name("foo-bar")
                .version_req("1.0.0")
                .url("https://example.com/second.zip")
                .build()
                .into(),
        ];
        let (progress, _) = InstallProgress::new();

        let res = install_dependencies(&dependencies, &[], &dir, false, progress).await;
        assert!(matches!(res, Err(InstallError::PathCollision { .. })));
    }
}
