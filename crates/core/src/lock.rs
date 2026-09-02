//! Lockfile handling.
//!
//! The lockfile contains the resolved dependencies of a project. It is a TOML file with an array of
//! dependencies, each containing the name, version, and other information about the dependency.
//!
//! The lockfile is used to ensure that the same versions of dependencies are installed across
//! different machines. It is also used to skip the installation of dependencies that are already
//! installed.
use crate::{
    config::{Dependency, GitIdentifier},
    errors::{LockError, LockMismatch},
    utils::{is_symlink, sanitize_filename, version_matches_req},
};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub mod forge;

pub const SOLDEER_LOCK: &str = "soldeer.lock";

pub type Result<T> = std::result::Result<T, LockError>;

/// A trait implemented by lockfile entries to provide the install path
pub trait Integrity {
    /// Returns the install path of the dependency.
    fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf;

    /// Returns the integrity checksum if relevant.
    fn integrity(&self) -> Option<&String>;
}

/// A lock entry for a git dependency.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bon::Builder)]
#[builder(on(String, into))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub struct GitLockEntry {
    /// The name of the dependency.
    pub name: String,

    /// The version (this corresponds to the version requirement of the dependency).
    pub version: String,

    /// The git url of the dependency.
    pub git: String,

    /// The resolved git commit hash.
    pub rev: String,
}

impl Integrity for GitLockEntry {
    /// Returns the install path of the dependency.
    ///
    /// The directory does not need to exist. Since the lock entry contains the version,
    /// the install path can be calculated without needing to check the actual directory.
    fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        format_install_path(&self.name, &self.version, deps)
    }

    /// There is no integrity checksum for git lock entries
    fn integrity(&self) -> Option<&String> {
        None
    }
}

/// A lock entry for an HTTP dependency.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bon::Builder)]
#[builder(on(String, into))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub struct HttpLockEntry {
    /// The name of the dependency.
    pub name: String,

    /// The resolved version of the dependency (not necessarily matches the version requirement of
    /// the dependency).
    ///
    /// If the version req is a semver range, then this will be the exact version that was
    /// resolved.
    pub version: String,

    /// The URL from where the dependency was downloaded.
    pub url: String,

    /// The checksum of the downloaded zip file.
    pub checksum: String,

    /// The integrity hash of the downloaded zip file after extraction.
    pub integrity: String,
}

impl Integrity for HttpLockEntry {
    /// Returns the install path of the dependency.
    ///
    /// The directory does not need to exist. Since the lock entry contains the version,
    /// the install path can be calculated without needing to check the actual directory.
    fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        format_install_path(&self.name, &self.version, deps)
    }

    /// Returns the integrity checksum
    fn integrity(&self) -> Option<&String> {
        Some(&self.integrity)
    }
}

/// A lock entry for a private dependency.
///
/// The link is not stored in the lockfile as it must be fetched from the registry with a valid
/// token before each download.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bon::Builder)]
#[builder(on(String, into))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub struct PrivateLockEntry {
    /// The name of the dependency.
    pub name: String,

    /// The resolved version of the dependency (not necessarily matches the version requirement of
    /// the dependency).
    ///
    /// If the version req is a semver range, then this will be the exact version that was
    /// resolved.
    pub version: String,

    /// The checksum of the downloaded zip file.
    pub checksum: String,

    /// The integrity hash of the downloaded zip file after extraction.
    pub integrity: String,
}

impl Integrity for PrivateLockEntry {
    /// Returns the install path of the dependency.
    ///
    /// The directory does not need to exist. Since the lock entry contains the version,
    /// the install path can be calculated without needing to check the actual directory.
    fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        format_install_path(&self.name, &self.version, deps)
    }

    /// Returns the integrity checksum
    fn integrity(&self) -> Option<&String> {
        Some(&self.integrity)
    }
}

/// A lock entry for a dependency.
///
/// A builder should be used to create the underlying [`HttpLockEntry`] or [`GitLockEntry`] and then
/// converted into this type with `.into()`.
///
/// # Examples
///
/// ```
/// # use soldeer_core::lock::{LockEntry, HttpLockEntry};
/// let dep: LockEntry = HttpLockEntry::builder()
///     .name("my-dep")
///     .version("1.2.3")
///     .url("https://...")
///     .checksum("dead")
///     .integrity("beef")
///     .build()
///     .into();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum LockEntry {
    /// A lock entry for an HTTP dependency.
    Http(HttpLockEntry),

    /// A lock entry for a git dependency.
    Git(GitLockEntry),

    /// A lock entry for a git dependency.
    Private(PrivateLockEntry),
}

/// A TOML representation of a lock entry, which merges all fields from the two variants of
/// [`LockEntry`].
///
/// This is used to serialize and deserialize lock entries to and from TOML. All fields which are
/// not present in both variants are optional.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TomlLockEntry {
    pub name: String,
    pub version: String,
    pub git: Option<String>,
    pub url: Option<String>,
    pub rev: Option<String>,
    pub checksum: Option<String>,
    pub integrity: Option<String>,
}

impl From<LockEntry> for TomlLockEntry {
    /// Convert a [`LockEntry`] into a [`TomlLockEntry`].
    fn from(value: LockEntry) -> Self {
        match value {
            LockEntry::Http(lock) => Self {
                name: lock.name,
                version: lock.version,
                git: None,
                url: Some(lock.url),
                rev: None,
                checksum: Some(lock.checksum),
                integrity: Some(lock.integrity),
            },
            LockEntry::Git(lock) => Self {
                name: lock.name,
                version: lock.version,
                git: Some(lock.git),
                url: None,
                rev: Some(lock.rev),
                checksum: None,
                integrity: None,
            },
            LockEntry::Private(lock) => Self {
                name: lock.name,
                version: lock.version,
                git: None,
                url: None,
                rev: None,
                checksum: Some(lock.checksum),
                integrity: Some(lock.integrity),
            },
        }
    }
}

impl TryFrom<TomlLockEntry> for LockEntry {
    type Error = LockError;

    /// Convert a [`TomlLockEntry`] into a [`LockEntry`] if possible.
    fn try_from(value: TomlLockEntry) -> std::result::Result<Self, Self::Error> {
        match (value.url, value.git) {
            (None, None) => Ok(PrivateLockEntry::builder()
                .name(&value.name)
                .version(value.version)
                .checksum(value.checksum.ok_or(LockError::MissingField {
                    field: "checksum".to_string(),
                    dep: value.name.clone(),
                })?)
                .integrity(value.integrity.ok_or(LockError::MissingField {
                    field: "integrity".to_string(),
                    dep: value.name,
                })?)
                .build()
                .into()),
            (None, Some(git)) => {
                Ok(GitLockEntry::builder()
                    .name(&value.name)
                    .version(value.version)
                    .git(git)
                    .rev(value.rev.ok_or(LockError::MissingField {
                        field: "rev".to_string(),
                        dep: value.name,
                    })?)
                    .build()
                    .into())
            }
            (Some(url), None) => Ok(HttpLockEntry::builder()
                .name(&value.name)
                .version(value.version)
                .url(url)
                .checksum(value.checksum.ok_or(LockError::MissingField {
                    field: "checksum".to_string(),
                    dep: value.name.clone(),
                })?)
                .integrity(value.integrity.ok_or(LockError::MissingField {
                    field: "integrity".to_string(),
                    dep: value.name,
                })?)
                .build()
                .into()),
            (Some(_), Some(_)) => Err(LockError::InvalidLockEntry),
        }
    }
}

impl LockEntry {
    /// The name of the dependency.
    pub fn name(&self) -> &str {
        match self {
            Self::Git(lock) => &lock.name,
            Self::Http(lock) => &lock.name,
            Self::Private(lock) => &lock.name,
        }
    }

    /// The version of the dependency.
    pub fn version(&self) -> &str {
        match self {
            Self::Git(lock) => &lock.version,
            Self::Http(lock) => &lock.version,
            Self::Private(lock) => &lock.version,
        }
    }

    /// The install path of the dependency.
    pub fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        match self {
            Self::Git(lock) => lock.install_path(deps),
            Self::Http(lock) => lock.install_path(deps),
            Self::Private(lock) => lock.install_path(deps),
        }
    }

    /// Get the underlying [`HttpLockEntry`] if this is an HTTP lock entry.
    pub fn as_http(&self) -> Option<&HttpLockEntry> {
        if let Self::Http(l) = self { Some(l) } else { None }
    }

    /// Get the underlying [`GitLockEntry`] if this is a git lock entry.
    pub fn as_git(&self) -> Option<&GitLockEntry> {
        if let Self::Git(l) = self { Some(l) } else { None }
    }

    /// Get the underlying [`PrivateLockEntry`] if this is a private package lock entry.
    pub fn as_private(&self) -> Option<&PrivateLockEntry> {
        if let Self::Private(l) = self { Some(l) } else { None }
    }

    /// Check that this entry describes the dependency declared in the config file.
    ///
    /// The name is not checked, since entries are looked up by name in the first place. Registry
    /// dependencies don't pin a URL in the config, so their download URL is resolved again at
    /// install time. For git dependencies, we only check consistency with the config if a `rev` is
    /// provided, as anything more would require talking to a remote.
    ///
    /// # Errors
    /// If the entry's type, version, source URL or pinned commit disagrees with the config.
    pub fn matches(&self, dependency: &Dependency) -> std::result::Result<(), LockMismatch> {
        match (self, dependency) {
            (Self::Http(lock), Dependency::Http(dep)) => {
                check_version(&lock.version, &dep.version_req, dep.url.is_some())?;
                if let Some(url) = &dep.url &&
                    url != &lock.url
                {
                    return Err(LockMismatch::Url {
                        locked: lock.url.clone(),
                        required: url.clone(),
                    });
                }
                Ok(())
            }
            (Self::Private(lock), Dependency::Http(dep)) => {
                // private dependencies always come from the registry, so a custom URL can't match
                if dep.url.is_some() {
                    return Err(LockMismatch::Type {
                        locked: "private".to_string(),
                        required: "http".to_string(),
                    });
                }
                check_version(&lock.version, &dep.version_req, false)
            }
            (Self::Git(lock), Dependency::Git(dep)) => {
                check_version(&lock.version, &dep.version_req, true)?;
                if lock.git != dep.git {
                    return Err(LockMismatch::Git {
                        locked: lock.git.clone(),
                        required: dep.git.clone(),
                    });
                }
                if let Some(GitIdentifier::Rev(rev)) = &dep.identifier &&
                    rev != &lock.rev
                {
                    return Err(LockMismatch::Rev {
                        locked: lock.rev.clone(),
                        required: rev.clone(),
                    });
                }
                Ok(())
            }
            (lock, dep) => Err(LockMismatch::Type {
                locked: lock.kind().to_string(),
                required: match dep {
                    Dependency::Http(_) => "http".to_string(),
                    Dependency::Git(_) => "git".to_string(),
                },
            }),
        }
    }

    /// The kind of dependency this entry describes, for diagnostics.
    fn kind(&self) -> &'static str {
        match self {
            Self::Git(_) => "git",
            Self::Http(_) => "http",
            Self::Private(_) => "private",
        }
    }
}

/// Check that a locked version is compatible with the version requirement from the config.
///
/// Git dependencies and HTTP dependencies with a custom URL store the requirement string verbatim
/// as their version, since there is no registry to resolve a range against, so they are compared
/// exactly.
fn check_version(
    locked: &str,
    version_req: &str,
    exact: bool,
) -> std::result::Result<(), LockMismatch> {
    let matches =
        if exact { locked == version_req } else { version_matches_req(locked, version_req) };
    if matches {
        return Ok(());
    }
    Err(LockMismatch::Version { locked: locked.to_string(), required: version_req.to_string() })
}

/// Find the lockfile entry corresponding to a dependency from the config file.
///
/// # Errors
/// If an entry exists for that dependency name but disagrees with the config.
pub fn lock_for_dependency<'a>(
    locks: &'a [LockEntry],
    dependency: &Dependency,
) -> Result<Option<&'a LockEntry>> {
    let Some(lock) = locks.iter().find(|l| l.name() == dependency.name()) else {
        debug!(dep:% = dependency; "no lockfile entry for dependency");
        return Ok(None);
    };
    lock.matches(dependency)
        .map_err(|source| LockError::Mismatch { dep: dependency.to_string(), source })?;
    Ok(Some(lock))
}

impl From<HttpLockEntry> for LockEntry {
    /// Wrap an [`HttpLockEntry`] in a [`LockEntry`].
    fn from(value: HttpLockEntry) -> Self {
        Self::Http(value)
    }
}

impl From<GitLockEntry> for LockEntry {
    /// Wrap a [`GitLockEntry`] in a [`LockEntry`].
    fn from(value: GitLockEntry) -> Self {
        Self::Git(value)
    }
}

impl From<PrivateLockEntry> for LockEntry {
    /// Wrap a [`PrivateLockEntry`] in a [`LockEntry`].
    fn from(value: PrivateLockEntry) -> Self {
        Self::Private(value)
    }
}

/// A parsed TOML lock file.
///
/// The lockfile is a table with one entry `dependencies` containing an array of [`TomlLockEntry`]s.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, Hash)]
struct LockFileParsed {
    dependencies: Vec<TomlLockEntry>,
}

/// The result of reading and parsing a lock file.
///
/// The [`TomlLockEntry`]s are converted into [`LockEntry`]s. A copy of the text contents of
/// the lockfile is provided for diffing purposes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LockFile {
    /// The parsed lock entries.
    pub entries: Vec<LockEntry>,

    /// The raw contents of the lockfile.
    pub raw: String,
}

/// Read a lockfile from disk.
pub fn read_lockfile(path: impl AsRef<Path>) -> Result<LockFile> {
    if is_symlink(&path)? {
        return Err(LockError::IOError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "soldeer.lock must not be a symlink",
        )));
    }
    if !path.as_ref().exists() {
        debug!(path:? = path.as_ref(); "lockfile does not exist");
        return Ok(LockFile::default());
    }
    let contents = fs::read_to_string(&path)?;

    let data: LockFileParsed = toml_edit::de::from_str(&contents).inspect_err(|err| {
        warn!(path:? = path.as_ref(), err:%; "error while parsing soldeer.lock TOML contents");
    })?;
    let entries =
        data.dependencies.into_iter().map(TryInto::try_into).collect::<Result<Vec<LockEntry>>>()?;
    Ok(LockFile { entries, raw: contents })
}

/// Generate the contents of a lockfile from a list of lock entries.
///
/// The entries do not need to be sorted, they will be sorted by name.
pub fn generate_lockfile_contents(mut entries: Vec<LockEntry>) -> String {
    entries.sort_unstable_by(|a, b| a.name().cmp(b.name()));
    let data = LockFileParsed { dependencies: entries.into_iter().map(Into::into).collect() };
    toml_edit::ser::to_string_pretty(&data).expect("Lock entries should be serializable")
}

/// Write contents to a lockfile without following symlinks.
///
/// To ensure the path is not modified to be a symlink again after the check,
/// the contents are written to a temporary file in the same folder and then the
/// file is renamed to overwrite the final location.
///
/// # Errors
/// If the `path` is a symlink, an `IOError` is returned.
pub fn write_lockfile(contents: &str, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if is_symlink(path)? {
        return Err(LockError::IOError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "soldeer.lock must not be a symlink",
        )));
    }
    replace_file(contents, path)?;
    debug!(path:? = path; "lockfile modified");
    Ok(())
}

/// Add a lock entry to a lockfile.
///
/// If an entry with the same name already exists, it will be replaced.
/// The entries are sorted by name before being written back to the file.
pub fn add_to_lockfile(entry: LockEntry, path: impl AsRef<Path>) -> Result<()> {
    let mut lockfile = read_lockfile(&path)?;
    if let Some(index) = lockfile.entries.iter().position(|e| e.name() == entry.name()) {
        debug!(name = entry.name(); "replacing existing lockfile entry");
        let _ = std::mem::replace(&mut lockfile.entries[index], entry);
    } else {
        debug!(name = entry.name(); "adding new lockfile entry");
        lockfile.entries.push(entry);
    }
    let new_contents = generate_lockfile_contents(lockfile.entries);
    write_lockfile(&new_contents, &path)
}

/// Remove a lock entry from a lockfile, matching on the name.
///
/// If the entry is the last entry in the lockfile, the lockfile will be removed.
pub fn remove_lock(dependency: &Dependency, path: impl AsRef<Path>) -> Result<()> {
    let lockfile = read_lockfile(&path)?;

    let entries: Vec<_> = lockfile
        .entries
        .into_iter()
        .filter_map(|e| if e.name() != dependency.name() { Some(e.into()) } else { None })
        .collect();

    if entries.is_empty() {
        // remove lock file if there are no deps left
        debug!(path:? = path.as_ref(); "no remaining lockfile entry, deleting file");
        let _ = fs::remove_file(&path);
        return Ok(());
    }

    let file_contents =
        toml_edit::ser::to_string_pretty(&LockFileParsed { dependencies: entries })?;

    // replace contents of lockfile with new contents
    write_lockfile(&file_contents, &path)
}

/// Format the install path of a dependency.
///
/// The folder name is sanitized to remove disallowed characters.
pub fn format_install_path(name: &str, version: &str, deps: impl AsRef<Path>) -> PathBuf {
    deps.as_ref().join(sanitize_filename(&format!("{name}-{version}")))
}

/// Replace a file with the given contents, via a temporary file in the same
/// folder.
///
/// The rename is atomic and replaces any potential symlink at `path`.
fn replace_file(contents: &str, path: &Path) -> Result<()> {
    let Some(filename) = path.file_name() else {
        return Err(LockError::IOError(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "lockfile path must point to a file",
        )));
    };
    let mut tmp_filename = filename.to_os_string();
    tmp_filename.push(format!(".{}.tmp", Uuid::new_v4()));
    let tmp_path = path.with_file_name(tmp_filename);
    let res = fs::write(&tmp_path, contents).and_then(|()| fs::rename(&tmp_path, path));
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GitDependency, HttpDependency};
    use testdir::testdir;

    fn http_lock(version: &str, url: &str) -> LockEntry {
        HttpLockEntry::builder()
            .name("mylib")
            .version(version)
            .url(url)
            .checksum("dead")
            .integrity("beef")
            .build()
            .into()
    }

    fn git_lock(version: &str, git: &str, rev: &str) -> LockEntry {
        GitLockEntry::builder().name("mylib").version(version).git(git).rev(rev).build().into()
    }

    #[test]
    fn test_check_matches_registry_version() {
        let lock = http_lock("1.2.0", "https://example.com/mylib.zip");
        // the lockfile records the resolved version, which must satisfy the requirement
        for req in ["^1.0.0", "1.2.0", "*", ">=1.1.0, <2.0.0"] {
            let dep = HttpDependency::builder().name("mylib").version_req(req).build().into();
            assert!(lock.matches(&dep).is_ok(), "{req}");
        }
        for req in ["^2.0.0", "1.1.0", "=1.3.0"] {
            let dep = HttpDependency::builder().name("mylib").version_req(req).build().into();
            assert!(matches!(lock.matches(&dep), Err(LockMismatch::Version { .. })), "{req}");
        }
    }

    #[test]
    fn test_check_matches_custom_url() {
        let lock = http_lock("1.0.0", "https://example.com/mylib.zip");
        let dep = HttpDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .url("https://example.com/mylib.zip")
            .build()
            .into();
        assert!(lock.matches(&dep).is_ok());

        // a lockfile entry cannot repoint a dependency at another URL
        let dep = HttpDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .url("https://evil.com/mylib.zip")
            .build()
            .into();
        assert!(matches!(lock.matches(&dep), Err(LockMismatch::Url { .. })));

        // for a custom URL the version requirement is stored verbatim, so it is compared exactly
        let dep = HttpDependency::builder()
            .name("mylib")
            .version_req("^1.0.0")
            .url("https://example.com/mylib.zip")
            .build()
            .into();
        assert!(matches!(lock.matches(&dep), Err(LockMismatch::Version { .. })));
    }

    #[test]
    fn test_check_matches_git() {
        let rev = "78c2f6a1a54db26bab6c3f501854a1564eb3707f";
        let lock = git_lock("1.0.0", "git@github.com:foo/bar.git", rev);
        let dep = GitDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .git("git@github.com:foo/bar.git")
            .build()
            .into();
        assert!(lock.matches(&dep).is_ok());

        let dep = GitDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .git("git@github.com:evil/bar.git")
            .build()
            .into();
        assert!(matches!(lock.matches(&dep), Err(LockMismatch::Git { .. })));

        // an explicit rev in the config wins over the one recorded in the lockfile
        let dep = GitDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .git("git@github.com:foo/bar.git")
            .identifier(GitIdentifier::from_rev("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))
            .build()
            .into();
        assert!(matches!(lock.matches(&dep), Err(LockMismatch::Rev { .. })));

        // a branch can only be resolved by talking to the remote, so it is not compared
        let dep = GitDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .git("git@github.com:foo/bar.git")
            .identifier(GitIdentifier::from_branch("main"))
            .build()
            .into();
        assert!(lock.matches(&dep).is_ok());
    }

    #[test]
    fn test_check_matches_type() {
        let git = git_lock("1.0.0", "git@github.com:foo/bar.git", "dead");
        let dep = HttpDependency::builder().name("mylib").version_req("1.0.0").build().into();
        assert!(matches!(git.matches(&dep), Err(LockMismatch::Type { .. })));

        let http = http_lock("1.0.0", "https://example.com/mylib.zip");
        let dep = GitDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .git("git@github.com:foo/bar.git")
            .build()
            .into();
        assert!(matches!(http.matches(&dep), Err(LockMismatch::Type { .. })));

        // private entries come from the registry, so they can't back a custom URL
        let private: LockEntry = PrivateLockEntry::builder()
            .name("mylib")
            .version("1.0.0")
            .checksum("dead")
            .integrity("beef")
            .build()
            .into();
        let dep = HttpDependency::builder().name("mylib").version_req("1.0.0").build().into();
        assert!(private.matches(&dep).is_ok());
        let dep = HttpDependency::builder()
            .name("mylib")
            .version_req("1.0.0")
            .url("https://example.com/mylib.zip")
            .build()
            .into();
        assert!(matches!(private.matches(&dep), Err(LockMismatch::Type { .. })));
    }

    #[test]
    fn test_lock_for_dependency() {
        let locks = vec![http_lock("1.2.0", "https://example.com/mylib.zip")];
        let dep = HttpDependency::builder().name("mylib").version_req("^1.0.0").build().into();
        assert!(lock_for_dependency(&locks, &dep).unwrap().is_some());

        // a dependency without an entry is not an error, it just gets installed from the config
        let dep = HttpDependency::builder().name("other").version_req("^1.0.0").build().into();
        assert!(lock_for_dependency(&locks, &dep).unwrap().is_none());

        let dep = HttpDependency::builder().name("mylib").version_req("^2.0.0").build().into();
        let res = lock_for_dependency(&locks, &dep);
        assert!(matches!(res, Err(LockError::Mismatch { .. })), "{res:?}");
    }

    #[test]
    fn test_toml_to_lock_entry_conversion_http() {
        let toml_entry = TomlLockEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            git: None,
            url: Some("https://example.com/zip.zip".to_string()),
            rev: None,
            checksum: Some("123456".to_string()),
            integrity: Some("beef".to_string()),
        };
        let entry: Result<LockEntry> = toml_entry.try_into();
        assert!(entry.is_ok(), "{entry:?}");
        let entry = entry.unwrap();
        assert_eq!(entry.name(), "test");
        assert_eq!(entry.version(), "1.0.0");
        let http = entry.as_http().unwrap();
        assert_eq!(http.url, "https://example.com/zip.zip");
        assert_eq!(http.checksum, "123456");
        assert_eq!(http.integrity, "beef");
    }

    #[test]
    fn test_toml_to_lock_entry_conversion_git() {
        let toml_entry = TomlLockEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            git: Some("git@github.com:test/test.git".to_string()),
            url: None,
            rev: Some("123456".to_string()),
            checksum: None,
            integrity: None,
        };
        let entry: Result<LockEntry> = toml_entry.try_into();
        assert!(entry.is_ok(), "{entry:?}");
        let entry = entry.unwrap();
        assert_eq!(entry.name(), "test");
        assert_eq!(entry.version(), "1.0.0");
        let git = entry.as_git().unwrap();
        assert_eq!(git.git, "git@github.com:test/test.git");
        assert_eq!(git.rev, "123456");
    }

    #[test]
    fn test_toml_lock_entry_bad_http() {
        let toml_entry = TomlLockEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            git: None,
            url: Some("https://example.com/zip.zip".to_string()),
            rev: None,
            checksum: None,
            integrity: None,
        };
        let entry: Result<LockEntry> = toml_entry.try_into();
        assert!(
            matches!(entry, Err(LockError::MissingField { ref field, dep: _ }) if field == "checksum"),
            "{entry:?}"
        );

        let toml_entry = TomlLockEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            git: None,
            url: Some("https://example.com/zip.zip".to_string()),
            rev: None,
            checksum: Some("123456".to_string()),
            integrity: None,
        };
        let entry: Result<LockEntry> = toml_entry.try_into();
        assert!(
            matches!(entry, Err(LockError::MissingField { ref field, dep: _ }) if field == "integrity"),
            "{entry:?}"
        );
    }

    #[test]
    fn test_toml_lock_entry_bad_private() {
        let toml_entry = TomlLockEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            git: None,
            url: None,
            rev: None,
            checksum: None,
            integrity: None,
        };
        let entry: Result<LockEntry> = toml_entry.try_into();
        assert!(
            matches!(entry, Err(LockError::MissingField { ref field, dep: _ }) if field == "checksum"),
            "{entry:?}"
        );
    }

    #[test]
    fn test_toml_lock_entry_bad_git() {
        let toml_entry = TomlLockEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            git: Some("git@github.com:test/test.git".to_string()),
            url: Some("https://example.com/zip.zip".to_string()),
            rev: None,
            checksum: None,
            integrity: None,
        };
        let entry: Result<LockEntry> = toml_entry.try_into();
        assert!(matches!(entry, Err(LockError::InvalidLockEntry)), "{entry:?}");

        let toml_entry = TomlLockEntry {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            git: Some("git@github.com:test/test.git".to_string()),
            url: None,
            rev: None,
            checksum: None,
            integrity: None,
        };
        let entry: Result<LockEntry> = toml_entry.try_into();
        assert!(
            matches!(entry, Err(LockError::MissingField { ref field, dep: _ }) if field == "rev"),
            "{entry:?}"
        );
    }

    #[test]
    fn test_read_lockfile() {
        let dir = testdir!();
        let file_path = dir.join(SOLDEER_LOCK);
        // an invalid entry invalidates the whole lockfile
        let content = r#"[[dependencies]]
name = "test"
version = "1.0.0"
git = "git@github.com:test/test.git"
rev = "123456"

[[dependencies]]
name = "test2"
version = "1.0.0"
url = "https://example.com/zip.zip"
checksum = "123456"
integrity = "beef"

[[dependencies]]
name = "test3"
version = "1.0.0"
"#;
        fs::write(&file_path, content).unwrap();
        let res = read_lockfile(&file_path);
        assert!(matches!(res, Err(LockError::MissingField { .. })));
    }

    #[test]
    fn test_generate_lockfile_content() {
        let dir = testdir!();
        let file_path = dir.join(SOLDEER_LOCK);
        let content = r#"[[dependencies]]
name = "test"
version = "1.0.0"
git = "git@github.com:test/test.git"
rev = "123456"

[[dependencies]]
name = "test2"
version = "1.0.0"
url = "https://example.com/zip.zip"
checksum = "123456"
integrity = "beef"
"#;
        fs::write(&file_path, content).unwrap();
        let lockfile = read_lockfile(&file_path).unwrap();
        let new_content = generate_lockfile_contents(lockfile.entries);
        assert_eq!(new_content, content);
    }

    #[test]
    fn test_add_to_lockfile() {
        let dir = testdir!();
        let file_path = dir.join(SOLDEER_LOCK);
        let content = r#"[[dependencies]]
name = "test"
version = "1.0.0"
git = "git@github.com:test/test.git"
rev = "123456"
"#;
        fs::write(&file_path, content).unwrap();
        let entry: LockEntry = HttpLockEntry::builder()
            .name("test2")
            .version("1.0.0")
            .url("https://example.com/zip.zip")
            .checksum("123456")
            .integrity("beef")
            .build()
            .into();
        let res = add_to_lockfile(entry.clone(), &file_path);
        assert!(res.is_ok(), "{res:?}");
        let lockfile = read_lockfile(&file_path).unwrap();
        assert_eq!(lockfile.entries.len(), 2);
        assert_eq!(lockfile.entries[1], entry);
    }

    #[test]
    fn test_replace_in_lockfile() {
        let dir = testdir!();
        let file_path = dir.join(SOLDEER_LOCK);
        let content = r#"[[dependencies]]
name = "test"
version = "1.0.0"
git = "git@github.com:test/test.git"
rev = "123456"
"#;
        fs::write(&file_path, content).unwrap();
        let entry: LockEntry = HttpLockEntry::builder()
            .name("test")
            .version("2.0.0")
            .url("https://example.com/zip.zip")
            .checksum("123456")
            .integrity("beef")
            .build()
            .into();
        let res = add_to_lockfile(entry.clone(), &file_path);
        assert!(res.is_ok(), "{res:?}");
        let lockfile = read_lockfile(&file_path).unwrap();
        assert_eq!(lockfile.entries.len(), 1);
        assert_eq!(lockfile.entries[0], entry);
    }

    #[test]
    fn test_remove_lock() {
        let dir = testdir!();
        let file_path = dir.join(SOLDEER_LOCK);
        let content = r#"[[dependencies]]
name = "test"
version = "1.0.0"
git = "git@github.com:test/test.git"
rev = "123456"

[[dependencies]]
name = "test2"
version = "1.0.0"
url = "https://example.com/zip.zip"
checksum = "123456"
integrity = "beef"
"#;
        fs::write(&file_path, content).unwrap();
        let dep = Dependency::from_name_version("test2~2.0.0", None, None).unwrap();
        let res = remove_lock(&dep, &file_path);
        assert!(res.is_ok(), "{res:?}");
        let lockfile = read_lockfile(&file_path).unwrap();
        assert_eq!(lockfile.entries.len(), 1);
        assert_eq!(lockfile.entries[0].name(), "test");
    }

    #[test]
    fn test_remove_lock_empty() {
        let dir = testdir!();
        let file_path = dir.join(SOLDEER_LOCK);
        let content = r#"[[dependencies]]
name = "test"
version = "1.0.0"
git = "git@github.com:test/test.git"
rev = "123456"
"#;
        fs::write(&file_path, content).unwrap();
        let dep = Dependency::from_name_version("test~1.0.0", None, None).unwrap();
        let res = remove_lock(&dep, &file_path);
        assert!(res.is_ok(), "{res:?}");
        assert!(!file_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_lockfile_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let dir = testdir!();
        let target = dir.join("trusted.lock");
        let link = dir.join(SOLDEER_LOCK);
        fs::write(&target, "trusted").unwrap();
        symlink(&target, &link).unwrap();

        assert!(matches!(read_lockfile(&link), Err(LockError::IOError(_))));
        let entry: LockEntry = HttpLockEntry::builder()
            .name("test")
            .version("1.0.0")
            .url("https://example.com/zip.zip")
            .checksum("123456")
            .integrity("beef")
            .build()
            .into();
        assert!(matches!(add_to_lockfile(entry, &link), Err(LockError::IOError(_))));
        assert_eq!(fs::read_to_string(target).unwrap(), "trusted");
    }

    #[cfg(unix)]
    #[test]
    fn test_write_lockfile_does_not_follow_symlink() {
        use std::os::unix::fs::symlink;

        let dir = testdir!();
        let target = dir.join("trusted.lock");
        let link = dir.join(SOLDEER_LOCK);
        fs::write(&target, "trusted").unwrap();
        symlink(&target, &link).unwrap();

        // simulates losing the check-to-write race: the symlink is replaced, not written through
        replace_file("attacker", &link).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "trusted");
        assert!(!fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[test]
    fn test_write_lockfile_leaves_no_temp_file() {
        let dir = testdir!();
        let path = dir.join(SOLDEER_LOCK);
        write_lockfile("[[dependencies]]\n", &path).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "[[dependencies]]\n");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n != SOLDEER_LOCK)
            .collect();
        assert!(leftovers.is_empty(), "leftover files: {leftovers:?}");
    }
}
