//! Lockfile handling.
//!
//! The lockfile contains the resolved dependencies of a project. It is a TOML file with an array of
//! dependencies, each containing the name, version, and other information about the dependency.
//!
//! The lockfile is used to ensure that the same versions of dependencies are installed across
//! different machines. It is also used to skip the installation of dependencies that are already
//! installed.
use crate::{config::Dependency, errors::LockError, utils::sanitize_filename};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, LockError>;

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

impl GitLockEntry {
    /// Returns the install path of the dependency.
    ///
    /// The directory does not need to exist. Since the lock entry contains the version,
    /// the install path can be calculated without needing to check the actual directory.
    pub fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        format_install_path(&self.name, &self.version, deps)
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

impl HttpLockEntry {
    /// Returns the install path of the dependency.
    ///
    /// The directory does not need to exist. Since the lock entry contains the version,
    /// the install path can be calculated without needing to check the actual directory.
    pub fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        format_install_path(&self.name, &self.version, deps)
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
        }
    }
}

impl TryFrom<TomlLockEntry> for LockEntry {
    type Error = LockError;

    /// Convert a [`TomlLockEntry`] into a [`LockEntry`] if possible.
    fn try_from(value: TomlLockEntry) -> std::result::Result<Self, Self::Error> {
        if let Some(url) = value.url {
            Ok(HttpLockEntry::builder()
                .name(&value.name)
                .version(value.version)
                .url(url)
                .checksum(value.checksum.ok_or(LockError::MissingField {
                    field: "checksum".to_string(),
                    dep: value.name.clone(),
                })?)
                .integrity(value.integrity.ok_or(LockError::MissingField {
                    field: "integrity".to_string(),
                    dep: value.name.clone(),
                })?)
                .build()
                .into())
        } else {
            Ok(GitLockEntry::builder()
                .name(&value.name)
                .version(value.version)
                .git(value.git.ok_or(LockError::MissingField {
                    field: "git".to_string(),
                    dep: value.name.clone(),
                })?)
                .rev(value.rev.ok_or(LockError::MissingField {
                    field: "rev".to_string(),
                    dep: value.name.clone(),
                })?)
                .build()
                .into())
        }
    }
}

impl LockEntry {
    /// The name of the dependency.
    pub fn name(&self) -> &str {
        match self {
            Self::Git(lock) => &lock.name,
            Self::Http(lock) => &lock.name,
        }
    }

    /// The version of the dependency.
    pub fn version(&self) -> &str {
        match self {
            Self::Git(lock) => &lock.version,
            Self::Http(lock) => &lock.version,
        }
    }

    /// The install path of the dependency.
    pub fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        match self {
            Self::Git(lock) => lock.install_path(deps),
            Self::Http(lock) => lock.install_path(deps),
        }
    }

    /// Get the underlying [`HttpLockEntry`] if this is an HTTP lock entry.
    pub fn as_http(&self) -> Option<&HttpLockEntry> {
        if let Self::Http(l) = self {
            Some(l)
        } else {
            None
        }
    }

    /// Get the underlying [`GitLockEntry`] if this is a git lock entry.
    pub fn as_git(&self) -> Option<&GitLockEntry> {
        if let Self::Git(l) = self {
            Some(l)
        } else {
            None
        }
    }
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
    if !path.as_ref().exists() {
        debug!(path:? = path.as_ref(); "lockfile does not exist");
        return Ok(LockFile::default());
    }
    let contents = fs::read_to_string(&path)?;

    let data: LockFileParsed = toml_edit::de::from_str(&contents)
        .inspect_err(|err| {
            warn!(err:?; "error while parsing lockfile contents, it will be ignored");
        })
        .unwrap_or_default();
    Ok(LockFile {
        entries: data.dependencies.into_iter().filter_map(|d| d.try_into().ok()).collect(),
        raw: contents,
    })
}

/// Generate the contents of a lockfile from a list of lock entries.
///
/// The entries do not need to be sorted, they will be sorted by name.
pub fn generate_lockfile_contents(mut entries: Vec<LockEntry>) -> String {
    entries.sort_unstable_by(|a, b| a.name().cmp(b.name()));
    let data = LockFileParsed { dependencies: entries.into_iter().map(Into::into).collect() };
    toml_edit::ser::to_string_pretty(&data).expect("Lock entries should be serializable")
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
    fs::write(&path, new_contents)?;
    debug!(path:? = path.as_ref(); "lockfile modified");
    Ok(())
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
    fs::write(&path, file_contents)?;
    debug!(path:? = path.as_ref(); "lockfile modified");
    Ok(())
}

/// Format the install path of a dependency.
///
/// The folder name is sanitized to remove disallowed characters.
pub fn format_install_path(name: &str, version: &str, deps: impl AsRef<Path>) -> PathBuf {
    deps.as_ref().join(sanitize_filename(&format!("{name}-{version}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use testdir::testdir;

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
    fn test_toml_lock_entry_bad_git() {
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
            matches!(entry, Err(LockError::MissingField { ref field, dep: _ }) if field == "git"),
            "{entry:?}"
        );

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
        let file_path = dir.join("soldeer.lock");
        // last entry is invalid and should be skipped
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
        assert!(res.is_ok(), "{res:?}");
        let lockfile = res.unwrap();
        assert_eq!(lockfile.entries.len(), 2);
        assert_eq!(lockfile.entries[0].name(), "test");
        assert_eq!(lockfile.entries[0].version(), "1.0.0");
        let git = lockfile.entries[0].as_git().unwrap();
        assert_eq!(git.git, "git@github.com:test/test.git");
        assert_eq!(git.rev, "123456");
        assert_eq!(lockfile.entries[1].name(), "test2");
        assert_eq!(lockfile.entries[1].version(), "1.0.0");
        let http = lockfile.entries[1].as_http().unwrap();
        assert_eq!(http.url, "https://example.com/zip.zip");
        assert_eq!(http.checksum, "123456");
        assert_eq!(http.integrity, "beef");
        assert_eq!(lockfile.raw, content);
    }

    #[test]
    fn test_generate_lockfile_content() {
        let dir = testdir!();
        let file_path = dir.join("soldeer.lock");
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
        let file_path = dir.join("soldeer.lock");
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
        let file_path = dir.join("soldeer.lock");
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
        let file_path = dir.join("soldeer.lock");
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
        let file_path = dir.join("soldeer.lock");
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
}
