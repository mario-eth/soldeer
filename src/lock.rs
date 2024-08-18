use crate::{
    config::Dependency,
    download::IntegrityChecksum,
    errors::LockError,
    utils::{get_current_working_dir, read_file_to_string},
    LOCK_FILE,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use yansi::Paint as _;

pub type Result<T> = std::result::Result<T, LockError>;

// Top level struct to hold the TOML data.
#[bon::builder]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Hash)]
#[non_exhaustive]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub checksum: String,
    pub integrity: Option<String>,
}

// parse file contents
#[derive(Serialize, Deserialize, Default)]
struct LockFileParsed {
    dependencies: Vec<LockEntry>,
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
    Ok((data.dependencies, contents))
}

pub fn generate_lockfile_contents(mut entries: Vec<LockEntry>) -> String {
    entries.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));
    let data = LockFileParsed { dependencies: entries };
    toml_edit::ser::to_string_pretty(&data).expect("Lock entries should be serializable")
}

pub fn add_to_lockfile(entry: LockEntry) -> Result<()> {
    let (mut entries, _) = read_lockfile()?;
    if let Some(index) =
        entries.iter().position(|e| e.name == entry.name && e.version == entry.version)
    {
        let _ = std::mem::replace(&mut entries[index], entry);
    } else {
        entries.push(entry);
    }
    let new_contents = generate_lockfile_contents(entries);
    fs::write(LOCK_FILE.as_path(), new_contents)?;
    Ok(())
}

// OLD CODE ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LockWriteMode {
    Replace,
}

pub fn write_lock(
    dependencies: &[Dependency],
    integrity_checksums: &[Option<IntegrityChecksum>],
    mode: LockWriteMode,
) -> Result<()> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    if mode == LockWriteMode::Replace && lock_file.exists() {
        fs::remove_file(&lock_file)?;
    }

    if !lock_file.exists() {
        fs::File::create(&lock_file)?;
    }

    let mut entries = read_lock()?;
    for (dep, integrity) in dependencies.iter().zip(integrity_checksums.iter()) {
        let entry = match dep {
            Dependency::Http(dep) => LockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .source(dep.url.as_ref().expect("url field should be present"))
                .checksum(dep.checksum.as_ref().expect("checksum field should be present"))
                .maybe_integrity(integrity.clone().map(|c| c.to_string()))
                .build(),
            Dependency::Git(dep) => LockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .source(&dep.git)
                .checksum(dep.rev.as_ref().expect("rev field should be present"))
                .build(),
        };
        // check for entry already existing
        match entries.iter().position(|e| e.name == entry.name && e.version == entry.version) {
            Some(pos) => {
                println!("{}", format!("Updating {dep} in the lock file.").green());
                // replace the entry with the new data
                entries[pos] = entry;
            }
            None => {
                println!(
                    "{}",
                    format!("Writing {}~{} to the lock file.", entry.name, entry.version).green()
                );
                entries.push(entry);
            }
        }
    }
    // make sure the ordering is consistent
    entries.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));

    if entries.is_empty() {
        // remove lock file if there are no deps left
        let _ = fs::remove_file(&lock_file);
        return Ok(());
    }

    let file_contents = toml_edit::ser::to_string_pretty(&LockType { dependencies: entries })?;

    // replace contents of lockfile with new contents
    fs::write(lock_file, file_contents)?;
    Ok(())
}

pub fn remove_lock(dependency: &Dependency) -> Result<()> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    let entries: Vec<_> = read_lock()?
        .into_iter()
        .filter(|e| e.name != dependency.name() || e.version != dependency.version())
        .collect();

    if entries.is_empty() {
        // remove lock file if there are no deps left
        let _ = fs::remove_file(&lock_file);
        return Ok(());
    }

    let file_contents = toml_edit::ser::to_string_pretty(&LockType { dependencies: entries })?;

    // replace contents of lockfile with new contents
    fs::write(lock_file, file_contents)?;

    Ok(())
}

// Top level struct to hold the TOML data.
#[derive(Serialize, Deserialize, Debug, Default)]
struct LockType {
    dependencies: Vec<LockEntry>,
}

fn read_lock() -> Result<Vec<LockEntry>> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    if !lock_file.exists() {
        return Err(LockError::Missing);
    }
    let contents = read_file_to_string(lock_file);

    // parse file contents
    let data: LockType = toml_edit::de::from_str(&contents).unwrap_or_default();
    Ok(data.dependencies)
}
