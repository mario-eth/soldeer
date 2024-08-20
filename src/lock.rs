use crate::{config::Dependency, errors::LockError, utils::get_current_working_dir, LOCK_FILE};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

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

impl From<TomlLockEntry> for LockEntry {
    fn from(value: TomlLockEntry) -> Self {
        if let Some(url) = value.url {
            HttpLockEntry::builder()
                .name(value.name)
                .version(value.version)
                .url(url)
                .checksum(value.checksum.expect("http lock entry should have a checksum"))
                .integrity(
                    value.integrity.expect("http lock entry should have an integrity checksum"),
                )
                .build()
                .into()
        } else {
            GitLockEntry::builder()
                .name(value.name)
                .version(value.version)
                .git(value.git.expect("git lock entry should have a git URL"))
                .rev(value.rev.expect("git lock entry should have a rev"))
                .build()
                .into()
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

pub fn read_lockfile() -> Result<(Vec<LockEntry>, String)> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };
    if !lock_file.exists() {
        return Ok((vec![], String::new()));
    }
    let contents = fs::read_to_string(&lock_file)?;

    let data: LockFileParsed = toml_edit::de::from_str(&contents).unwrap_or_default();
    Ok((data.dependencies.into_iter().map(Into::into).collect(), contents))
}

pub fn generate_lockfile_contents(mut entries: Vec<LockEntry>) -> String {
    entries
        .sort_unstable_by(|a, b| a.name().cmp(b.name()).then_with(|| a.version().cmp(b.version())));
    let data = LockFileParsed { dependencies: entries.into_iter().map(Into::into).collect() };
    toml_edit::ser::to_string_pretty(&data).expect("Lock entries should be serializable")
}

pub fn add_to_lockfile(entry: LockEntry) -> Result<()> {
    let (mut entries, _) = read_lockfile()?;
    if let Some(index) =
        entries.iter().position(|e| e.name() == entry.name() && e.version() == entry.version())
    {
        let _ = std::mem::replace(&mut entries[index], entry);
    } else {
        entries.push(entry);
    }
    let new_contents = generate_lockfile_contents(entries);
    fs::write(LOCK_FILE.as_path(), new_contents)?;
    Ok(())
}

pub fn remove_lock(dependency: &Dependency) -> Result<()> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    let (entries, _) = read_lockfile()?;

    let entries: Vec<_> = entries
        .into_iter()
        .filter_map(|e| {
            if e.name() != dependency.name() || e.version() != dependency.version() {
                Some(e.into())
            } else {
                None
            }
        })
        .collect();

    if entries.is_empty() {
        // remove lock file if there are no deps left
        let _ = fs::remove_file(&lock_file);
        return Ok(());
    }

    let file_contents =
        toml_edit::ser::to_string_pretty(&LockFileParsed { dependencies: entries })?;

    // replace contents of lockfile with new contents
    fs::write(lock_file, file_contents)?;

    Ok(())
}
