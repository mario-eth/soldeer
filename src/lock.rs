use crate::{
    config::Dependency,
    dependency_downloader::IntegrityChecksum,
    errors::LockError,
    utils::{get_current_working_dir, read_file_to_string},
    LOCK_FILE,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use yansi::Paint as _;

pub type Result<T> = std::result::Result<T, LockError>;

// Top level struct to hold the TOML data.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct LockEntry {
    name: String,
    version: String,
    source: String,
    checksum: String,
    integrity: Option<String>,
}

impl LockEntry {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        source: impl Into<String>,
        checksum: impl Into<String>,
        integrity: Option<String>,
    ) -> Self {
        LockEntry {
            name: name.into(),
            version: version.into(),
            source: source.into(),
            checksum: checksum.into(),
            integrity,
        }
    }
}

pub fn lock_check(dependency: &Dependency, allow_missing_lockfile: bool) -> Result<()> {
    let lock_entries = match read_lock() {
        Ok(entries) => entries,
        Err(e) => {
            if allow_missing_lockfile {
                return Ok(());
            }
            return Err(e);
        }
    };

    let is_locked = lock_entries.iter().any(|lock_entry| {
        lock_entry.name == dependency.name() && lock_entry.version == dependency.version()
    });

    if is_locked {
        return Err(LockError::DependencyInstalled(dependency.to_string()));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LockWriteMode {
    Replace,
    Append,
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
            Dependency::Http(dep) => LockEntry::new(
                &dep.name,
                &dep.version,
                dep.url.as_ref().unwrap(),
                dep.checksum.as_ref().unwrap(),
                integrity.clone().map(|c| c.to_string()),
            ),
            Dependency::Git(dep) => {
                LockEntry::new(&dep.name, &dep.version, &dep.git, dep.rev.as_ref().unwrap(), None)
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Dependency, HttpDependency},
        utils::read_file_to_string,
    };
    use serial_test::serial;
    use std::{fs::File, io::Write};

    fn check_lock_file() -> PathBuf {
        let lock_file: PathBuf = get_current_working_dir().join("test").join("soldeer.lock");
        if lock_file.exists() {
            fs::remove_file(&lock_file).unwrap();
        }
        lock_file
    }

    pub fn initialize() {
        let lock_file = check_lock_file();
        let lock_contents = r#"
[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.3.0"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip"
checksum = "a2d469062adeb62f7a4aada78237acae4ad3c168ba65c3ac9c76e290332c11ec"
integrity = "deadbeef"

[[dependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
integrity = "deadbeef"
"#;
        File::create(lock_file).unwrap().write_all(lock_contents.as_bytes()).unwrap();
    }

    #[test]
    #[serial]
    fn lock_file_not_present_test() {
        let lock_file = check_lock_file();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None
        });

        assert!(matches!(lock_check(&dependency, false), Err(LockError::Missing)));

        assert!(!lock_file.exists());
    }

    #[test]
    #[serial]
    fn check_lock_all_locked_test() {
        initialize();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None
        });

        assert!(matches!(lock_check(&dependency, false), Err(LockError::DependencyInstalled(_))));
    }

    #[test]
    #[serial]
    fn write_clean_lock_test() {
        let lock_file = check_lock_file();
        let dependency =  Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string()),
            checksum: Some("5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string())
        });
        let dependencies = vec![dependency.clone()];
        write_lock(
            &dependencies,
            &[Some(IntegrityChecksum("deadbeef".to_string()))],
            LockWriteMode::Append,
        )
        .unwrap();
        assert!(matches!(lock_check(&dependency, true), Err(LockError::DependencyInstalled(_))));

        let contents = read_file_to_string(lock_file);

        assert_eq!(
            contents,
            r#"[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.5.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
integrity = "deadbeef"
"#
        );
        assert!(matches!(lock_check(&dependency, true), Err(LockError::DependencyInstalled(_))));
    }

    #[test]
    #[serial]
    fn write_append_lock_test() {
        let lock_file = check_lock_file();
        initialize();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts-2".to_string(),
            version: "2.6.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.6.0.zip".to_string()),
            checksum: Some("5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string())
        });
        dependencies.push(dependency.clone());
        write_lock(
            &dependencies,
            &[Some(IntegrityChecksum("deadbeef".to_string()))],
            LockWriteMode::Append,
        )
        .unwrap();
        let contents = read_file_to_string(lock_file);

        assert_eq!(
            contents,
            r#"[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.3.0"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip"
checksum = "a2d469062adeb62f7a4aada78237acae4ad3c168ba65c3ac9c76e290332c11ec"
integrity = "deadbeef"

[[dependencies]]
name = "@openzeppelin-contracts-2"
version = "2.6.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.6.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
integrity = "deadbeef"

[[dependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
integrity = "deadbeef"
"#
        );

        assert!(matches!(lock_check(&dependency, true), Err(LockError::DependencyInstalled(_))));
    }

    #[test]
    #[serial]
    fn remove_lock_single_success() {
        let lock_file = check_lock_file();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string()),
            checksum: Some("5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string())
        });
        let dependencies = vec![dependency.clone()];
        write_lock(&dependencies, &[Some(IntegrityChecksum::default())], LockWriteMode::Append)
            .unwrap();

        match remove_lock(&dependency) {
            Ok(_) => {}
            Err(_) => {
                assert_eq!("Invalid State", "");
            }
        }
        assert!(!lock_file.exists());
    }

    #[test]
    #[serial]
    fn remove_lock_multiple_success() {
        let lock_file = check_lock_file();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string()),
            checksum: Some("5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string())
        });
        let dependency2 = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts2".to_string(),
            version: "2.5.0".to_string(),
            url: Some( "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string()),
            checksum: Some("5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string())
        });
        let dependencies = vec![dependency.clone(), dependency2.clone()];
        write_lock(
            &dependencies,
            &[
                Some(IntegrityChecksum("deadbeef".to_string())),
                Some(IntegrityChecksum("deadbeef".to_string())),
            ],
            LockWriteMode::Append,
        )
        .unwrap();

        match remove_lock(&dependency) {
            Ok(_) => {}
            Err(_) => {
                assert_eq!("Invalid State", "");
            }
        }
        let contents = read_file_to_string(lock_file);

        assert_eq!(
            contents,
            r#"[[dependencies]]
name = "@openzeppelin-contracts2"
version = "2.5.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
integrity = "deadbeef"
"#
        );
    }

    #[test]
    #[serial]
    fn remove_lock_one_fails() {
        let lock_file = check_lock_file();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string()),
            checksum: Some("5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string())
        });

        let dependencies = vec![dependency.clone()];
        write_lock(
            &dependencies,
            &[Some(IntegrityChecksum("deadbeef".to_string()))],
            LockWriteMode::Append,
        )
        .unwrap();

        match remove_lock(&Dependency::Http(HttpDependency {
            name: "non-existent".to_string(),
            version: dependency.version().to_string(),
            url: None,
            checksum: None,
        })) {
            Ok(_) => {}
            Err(_) => {
                assert_eq!("Invalid State", "");
            }
        }
        let contents = read_file_to_string(lock_file);

        assert_eq!(
            contents,
            r#"[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.5.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
integrity = "deadbeef"
"#
        );
    }
}
