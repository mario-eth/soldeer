use crate::{config::Dependency, errors::LockError, utils::sanitize_filename};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, LockError>;

#[bon::builder]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Hash)]
#[non_exhaustive]
pub struct GitLockEntry {
    pub name: String,
    pub version: String,
    pub git: String,
    pub rev: String,
}

impl GitLockEntry {
    pub fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        format_install_path(&self.name, &self.version, deps)
    }
}

#[bon::builder]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Hash)]
#[non_exhaustive]
pub struct HttpLockEntry {
    pub name: String,
    pub version: String,
    pub url: String,
    pub checksum: String,
    pub integrity: String,
}

impl HttpLockEntry {
    pub fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        format_install_path(&self.name, &self.version, deps)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Hash)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum LockEntry {
    Http(HttpLockEntry),
    Git(GitLockEntry),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Hash)]
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
    pub fn name(&self) -> &str {
        match self {
            LockEntry::Git(lock) => &lock.name,
            LockEntry::Http(lock) => &lock.name,
        }
    }

    pub fn version(&self) -> &str {
        match self {
            LockEntry::Git(lock) => &lock.version,
            LockEntry::Http(lock) => &lock.version,
        }
    }

    pub fn install_path(&self, deps: impl AsRef<Path>) -> PathBuf {
        match self {
            LockEntry::Git(lock) => lock.install_path(deps),
            LockEntry::Http(lock) => lock.install_path(deps),
        }
    }

    #[allow(unused)]
    pub fn as_http(&self) -> Option<&HttpLockEntry> {
        if let Self::Http(l) = self {
            Some(l)
        } else {
            None
        }
    }

    pub fn as_git(&self) -> Option<&GitLockEntry> {
        if let Self::Git(l) = self {
            Some(l)
        } else {
            None
        }
    }
}

impl From<HttpLockEntry> for LockEntry {
    fn from(value: HttpLockEntry) -> Self {
        LockEntry::Http(value)
    }
}

impl From<GitLockEntry> for LockEntry {
    fn from(value: GitLockEntry) -> Self {
        LockEntry::Git(value)
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct LockFileParsed {
    dependencies: Vec<TomlLockEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct LockFile {
    pub entries: Vec<LockEntry>,
    pub raw: String,
}

pub fn read_lockfile(path: impl AsRef<Path>) -> Result<LockFile> {
    if !path.as_ref().exists() {
        return Ok(LockFile::default());
    }
    let contents = fs::read_to_string(&path)?;

    let data: LockFileParsed = toml_edit::de::from_str(&contents).unwrap_or_default();
    Ok(LockFile {
        entries: data.dependencies.into_iter().filter_map(|d| d.try_into().ok()).collect(),
        raw: contents,
    })
}

pub fn generate_lockfile_contents(mut entries: Vec<LockEntry>) -> String {
    entries
        .sort_unstable_by(|a, b| a.name().cmp(b.name()).then_with(|| a.version().cmp(b.version())));
    let data = LockFileParsed { dependencies: entries.into_iter().map(Into::into).collect() };
    toml_edit::ser::to_string_pretty(&data).expect("Lock entries should be serializable")
}

pub fn add_to_lockfile(entry: LockEntry, path: impl AsRef<Path>) -> Result<()> {
    let mut lockfile = read_lockfile(&path)?;
    if let Some(index) = lockfile
        .entries
        .iter()
        .position(|e| e.name() == entry.name() && e.version() == entry.version())
    {
        let _ = std::mem::replace(&mut lockfile.entries[index], entry);
    } else {
        lockfile.entries.push(entry);
    }
    let new_contents = generate_lockfile_contents(lockfile.entries);
    fs::write(&path, new_contents)?;
    Ok(())
}

pub fn remove_lock(dependency: &Dependency, path: impl AsRef<Path>) -> Result<()> {
    let lockfile = read_lockfile(&path)?;

    let entries: Vec<_> = lockfile
        .entries
        .into_iter()
        .filter_map(|e| if e.name() != dependency.name() { Some(e.into()) } else { None })
        .collect();

    if entries.is_empty() {
        // remove lock file if there are no deps left
        let _ = fs::remove_file(&path);
        return Ok(());
    }

    let file_contents =
        toml_edit::ser::to_string_pretty(&LockFileParsed { dependencies: entries })?;

    // replace contents of lockfile with new contents
    fs::write(&path, file_contents)?;

    Ok(())
}

pub fn format_install_path(name: &str, version: &str, deps: impl AsRef<Path>) -> PathBuf {
    deps.as_ref().join(sanitize_filename(&format!("{}-{}", name, version)))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
