use crate::{
    config::Dependency,
    download::{clone_repo, delete_dependency_files, download_file, unzip_file},
    errors::InstallError,
    lock::{format_install_path, GitLockEntry, HttpLockEntry, LockEntry},
    registry::{get_dependency_url_remote, get_latest_supported_version},
    utils::{hash_file, hash_folder, run_forge_command, run_git_command},
};
use cliclack::{progress_bar, MultiProgress, ProgressBar};
use std::{fmt, fs as std_fs, path::Path};
use tokio::{fs, task::JoinSet};
use toml_edit::DocumentMut;

pub const PROGRESS_TEMPLATE: &str = "[{elapsed_precise}] {bar:30.magenta} ({pos}/{len}) {msg}";

pub type Result<T> = std::result::Result<T, InstallError>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DependencyStatus {
    Missing,
    FailedIntegrity,
    Installed,
}

#[derive(Clone)]
pub struct Progress {
    pub multi: MultiProgress,
    pub versions: ProgressBar,
    pub downloads: ProgressBar,
    pub unzip: ProgressBar,
    pub subdependencies: ProgressBar,
    pub integrity: ProgressBar,
}

impl Progress {
    pub fn new(multi: &MultiProgress, deps: u64) -> Self {
        let versions = multi.add(progress_bar(deps).with_template(PROGRESS_TEMPLATE));
        let downloads = multi.add(progress_bar(deps).with_template(PROGRESS_TEMPLATE));
        let unzip = multi.add(progress_bar(deps).with_template(PROGRESS_TEMPLATE));
        let subdependencies = multi.add(progress_bar(deps).with_template(PROGRESS_TEMPLATE));
        let integrity = multi.add(progress_bar(deps).with_template(PROGRESS_TEMPLATE));
        Self { multi: multi.clone(), versions, downloads, unzip, subdependencies, integrity }
    }

    pub fn start_all(&self) {
        self.versions.start("Retrieving versions...");
        self.downloads.start("Downloading dependencies...");
        self.unzip.start("Unzipping dependencies...");
        self.subdependencies.start("Installing subdependencies...");
        self.integrity.start("Checking integrity...");
    }

    pub fn increment_all(&self) {
        self.versions.inc(1);
        self.downloads.inc(1);
        self.unzip.inc(1);
        self.subdependencies.inc(1);
        self.integrity.inc(1);
    }

    pub fn stop_all(&self) {
        self.versions.stop("Done retrieving versions");
        self.downloads.stop("Done downloading dependencies");
        self.unzip.stop("Done unzipping dependencies");
        self.subdependencies.stop("Done installing subdependencies");
        self.integrity.stop("Done checking integrity");
    }

    pub fn log(&self, msg: impl fmt::Display) {
        self.multi.println(msg);
    }
}

#[bon::builder]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HttpInstallInfo {
    name: String,
    version: String,
    url: String,
    checksum: Option<String>,
    integrity: Option<String>,
}

#[bon::builder]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GitInstallInfo {
    name: String,
    version: String,
    git: String,
    rev: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum InstallInfo {
    Http(HttpInstallInfo),
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
                integrity: Some(lock.integrity),
            }
            .into(),
            LockEntry::Git(lock) => GitInstallInfo {
                name: lock.name,
                version: lock.version,
                git: lock.git,
                rev: Some(lock.rev),
            }
            .into(),
        }
    }
}

pub async fn install_dependencies(
    dependencies: &[Dependency],
    locks: &[LockEntry],
    deps: impl AsRef<Path>,
    recursive_deps: bool,
    progress: Progress,
) -> Result<Vec<LockEntry>> {
    let mut set = JoinSet::new();
    for dep in dependencies {
        set.spawn({
            let d = dep.clone();
            let p = progress.clone();
            let lock = locks.iter().find(|l| l.name() == dep.name()).cloned();
            let deps = deps.as_ref().to_path_buf();
            async move { install_dependency(&d, lock.as_ref(), deps, None, recursive_deps, p).await }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res??);
    }
    Ok(results)
}

/// Install a single dependency
///
/// It's important that all file operations are done via the `tokio::fs` module because we are
/// highly concurrent here.
pub async fn install_dependency(
    dependency: &Dependency,
    lock: Option<&LockEntry>,
    deps: impl AsRef<Path>,
    force_version: Option<String>,
    recursive_deps: bool,
    progress: Progress,
) -> Result<LockEntry> {
    if let Some(lock) = lock {
        match check_dependency_integrity(lock, &deps).await? {
            DependencyStatus::Installed => {
                // no action needed, dependency is already installed and matches the lockfile
                // entry
                progress.increment_all();
                return Ok(lock.clone());
            }
            DependencyStatus::FailedIntegrity => match dependency {
                Dependency::Http(_) => {
                    // we know the folder exists because otherwise we would have gotten
                    // `Missing`
                    progress.log(format!(
                        "Dependency {dependency} failed integrity check, reinstalling"
                    ));
                    delete_dependency_files(dependency, &deps).await?;
                    // we won't need to retrieve the version number so we mark it as done
                    progress.versions.inc(1);
                }
                Dependency::Git(_) => {
                    progress.log(format!(
                        "Dependency {dependency} failed integrity check, resetting to commit {}",
                        lock.as_git().expect("lock entry should be of type git").rev
                    ));
                    reset_git_dependency(
                        lock.as_git().expect("lock entry should be of type git"),
                        &deps,
                    )
                    .await?;
                    // dependency should now be at the correct commit, we can exit
                    progress.increment_all();
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
                // we won't need to retrieve the version number so we mark it as done
                progress.versions.inc(1);
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
        // indicate that we have retrieved the version number
        progress.versions.inc(1);
        let info = match &dependency {
            Dependency::Http(dep) => {
                HttpInstallInfo::builder().name(&dep.name).version(&version).url(url).build().into()
            }
            Dependency::Git(dep) => GitInstallInfo::builder()
                .name(&dep.name)
                .version(&version)
                .git(url)
                .maybe_rev(dep.rev.clone())
                .build()
                .into(),
        };
        install_dependency_inner(
            &info,
            format_install_path(dependency.name(), &version, &deps),
            recursive_deps,
            progress,
        )
        .await
    }
}

pub async fn check_dependency_integrity(
    lock: &LockEntry,
    deps: impl AsRef<Path>,
) -> Result<DependencyStatus> {
    match lock {
        LockEntry::Http(lock) => check_http_dependency(lock, deps).await,
        LockEntry::Git(lock) => check_git_dependency(lock, deps).await,
    }
}

pub fn ensure_dependencies_dir(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        std_fs::create_dir(path)
            .map_err(|e| InstallError::IOError { path: path.to_path_buf(), source: e })?;
    }
    Ok(())
}

async fn install_dependency_inner(
    dep: &InstallInfo,
    path: impl AsRef<Path>,
    subdependencies: bool,
    progress: Progress,
) -> Result<LockEntry> {
    match dep {
        InstallInfo::Http(dep) => {
            let zip_path = download_file(&dep.url, &path).await?;
            progress.downloads.inc(1);
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
            }
            unzip_file(&zip_path, &path).await?;
            progress.unzip.inc(1);
            if subdependencies {
                install_subdependencies(&path).await?;
            }
            progress.subdependencies.inc(1);
            let integrity = hash_folder(&path, None);
            progress.integrity.inc(1);
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
            let commit = clone_repo(&dep.git, dep.rev.as_ref(), &path).await?;
            progress.downloads.inc(1);
            if subdependencies {
                install_subdependencies(&path).await?;
            }
            progress.unzip.inc(1);
            progress.subdependencies.inc(1);
            progress.integrity.inc(1);
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
        move || hash_folder(path, None)
    })
    .await?;
    println!("current_hash: {current_hash}");
    if current_hash.to_string() != lock.integrity {
        return Ok(DependencyStatus::FailedIntegrity);
    }
    Ok(DependencyStatus::Installed)
}

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
            top_level.split_whitespace().next().unwrap_or_default().to_string()
        }
        Err(_) => {
            // error getting the top level directory, assume the directory is not a git repository
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
        return Ok(DependencyStatus::Missing);
    }
    // for git dependencies, the `rev` field holds the commit hash
    match run_git_command(&["diff", "--exit-code", &lock.rev], Some(&path)).await {
        Ok(_) => Ok(DependencyStatus::Installed),
        Err(_) => Ok(DependencyStatus::FailedIntegrity),
    }
}

/// Reset a git dependency to the commit specified in the lockfile entry
///
/// This function runs `git reset --hard <commit>` and `git clean -fd` in the git dependency's
/// directory
async fn reset_git_dependency(lock: &GitLockEntry, deps: impl AsRef<Path>) -> Result<()> {
    let path = lock.install_path(deps);
    run_git_command(&["reset", "--hard", &lock.rev], Some(&path)).await?;
    run_git_command(&["clean", "-fd"], Some(&path)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use testdir::testdir;

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

        let hash = hash_folder(&path, None);
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
        let rev =
            clone_repo("https://github.com/beeb/test-repo.git", None::<&str>, &path).await.unwrap();
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
}
