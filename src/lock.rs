use crate::{
    config::Dependency,
    errors::LockError,
    utils::{get_current_working_dir, read_file_to_string},
    LOCK_FILE,
};
use serde_derive::{Deserialize, Serialize};
use std::{
    fs::{self, remove_file},
    path::PathBuf,
};
use yansi::Paint;

// Top level struct to hold the TOML data.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LockEntry {
    name: String,
    version: String,
    source: String,
    zip_checksum: String,
}

impl From<&Dependency> for LockEntry {
    fn from(value: &Dependency) -> Self {
        LockEntry {
            name: value.name.clone(),
            version: value.version.clone(),
            source: value.url.clone(),
            zip_checksum: value.hash.clone(),
        }
    }
}

pub fn lock_check(dependency: &Dependency, create_lock: bool) -> Result<(), LockError> {
    let lock_entries = match read_lock() {
        Ok(entries) => entries,
        Err(_) => {
            if create_lock {
                let _ = write_lock(&[], LockWriteMode::Append);
                return Ok(());
            }
            return Err(LockError { cause: "Lock does not exists".to_string() });
        }
    };

    let is_locked = lock_entries.iter().any(|lock_entry| {
        lock_entry.name == dependency.name && lock_entry.version == dependency.version
    });

    if is_locked {
        return Err(LockError {
            cause: format!(
                "Dependency {}-{} is already installed",
                dependency.name, dependency.version
            ),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LockWriteMode {
    Replace,
    Append,
}

pub fn write_lock(dependencies: &[Dependency], clean: LockWriteMode) -> Result<(), LockError> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    if clean == LockWriteMode::Replace && lock_file.exists() {
        remove_file(&lock_file)
            .map_err(|_| LockError { cause: "Could not clean lock file".to_string() })?;
    }

    if !lock_file.exists() {
        fs::File::create(&lock_file)
            .map_err(|_| LockError { cause: "Could not create lock file".to_string() })?;
    }

    let mut entries = read_lock()?;
    for dep in dependencies {
        let entry: LockEntry = dep.into();
        // check for entry already existing
        match entries.iter().position(|e| e.name == entry.name && e.version == entry.version) {
            Some(pos) => {
                // replace the entry with the new data
                entries[pos] = entry;
            }
            None => {
                println!(
                    "{}",
                    Paint::green(&format!(
                        "Writing {}~{} to the lock file.",
                        dep.name, dep.version
                    ))
                );
                entries.push(entry);
            }
        }
    }
    // make sure the ordering is consistent
    entries.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));

    if entries.is_empty() {
        // remove lock file if there are no deps left
        let _ = remove_file(&lock_file);
        return Ok(());
    }

    let file_contents = toml::to_string(&LockType { dependencies: entries })
        .map_err(|_| LockError { cause: "Could not serialize lock file".to_string() })?;

    // replace contents of lockfile with new contents
    fs::write(lock_file, file_contents)
        .map_err(|_| LockError { cause: "Could not write to the lock file".to_string() })?;
    Ok(())
}

pub fn remove_lock(dependency: &Dependency) -> Result<(), LockError> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    let entries: Vec<_> = read_lock()?
        .into_iter()
        .filter(|e| e.name != dependency.name || e.version != dependency.version)
        .collect();

    if entries.is_empty() {
        // remove lock file if there are no deps left
        let _ = remove_file(&lock_file);
        return Ok(());
    }

    let file_contents = toml::to_string(&LockType { dependencies: entries })
        .map_err(|_| LockError { cause: "Could not serialize lock file".to_string() })?;

    // replace contents of lockfile with new contents
    fs::write(lock_file, file_contents)
        .map_err(|_| LockError { cause: "Could not write to the lock file".to_string() })?;

    Ok(())
}

// Top level struct to hold the TOML data.
#[derive(Serialize, Deserialize, Debug, Default)]
struct LockType {
    dependencies: Vec<LockEntry>,
}

fn read_lock() -> Result<Vec<LockEntry>, LockError> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    if !lock_file.exists() {
        return Err(LockError { cause: "Lock does not exists".to_string() });
    }

    let contents = read_file_to_string(lock_file);

    // reading the contents into a data structure using toml::from_str
    let data: LockType = toml::from_str(&contents).unwrap_or_default();
    Ok(data.dependencies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Dependency, utils::read_file_to_string};
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
zip_checksum = "a2d469062adeb62f7a4aada78237acae4ad3c168ba65c3ac9c76e290332c11ec"

[[dependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
zip_checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#;
        File::create(lock_file).unwrap().write_all(lock_contents.as_bytes()).unwrap();
    }

    #[test]
    #[serial]
    fn lock_file_not_present_test() {
        let lock_file = check_lock_file();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
            hash: String::new()
        };

        assert!(
            lock_check(&dependency, false).is_err_and(|e| { e.cause == "Lock does not exists" })
        );
        assert!(!lock_file.exists());
    }

    #[test]
    #[serial]
    fn check_lock_all_locked_test() {
        initialize();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
            hash: String::new(),
        };

        assert!(lock_check(&dependency, true).is_err_and(|e| {
            e.cause == "Dependency @openzeppelin-contracts-2.3.0 is already installed"
        }));
    }

    #[test]
    #[serial]
    fn write_clean_lock_test() {
        let lock_file = check_lock_file();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string(),
            hash: "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
        };
        let dependencies = vec![dependency.clone()];
        write_lock(&dependencies, LockWriteMode::Append).unwrap();
        assert!(lock_check(&dependency, true).is_err_and(|e| {
            e.cause == "Dependency @openzeppelin-contracts-2.5.0 is already installed"
        }));
        let contents = read_file_to_string(lock_file);

        assert_eq!(
            contents,
            r#"[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.5.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip"
zip_checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#
        );
        assert!(lock_check(&dependency, true).is_err_and(|e| {
            e.cause == "Dependency @openzeppelin-contracts-2.5.0 is already installed"
        }));
    }

    #[test]
    #[serial]
    fn write_append_lock_test() {
        let lock_file = check_lock_file();
        initialize();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts-2".to_string(),
            version: "2.6.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.6.0.zip".to_string(),
            hash: "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
        };
        dependencies.push(dependency.clone());
        write_lock(&dependencies, LockWriteMode::Append).unwrap();
        let contents = read_file_to_string(lock_file);

        assert_eq!(
            contents,
            r#"[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.3.0"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip"
zip_checksum = "a2d469062adeb62f7a4aada78237acae4ad3c168ba65c3ac9c76e290332c11ec"

[[dependencies]]
name = "@openzeppelin-contracts-2"
version = "2.6.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.6.0.zip"
zip_checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"

[[dependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
zip_checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#
        );

        assert!(lock_check(&dependency, true).is_err_and(|e| {
            e.cause == "Dependency @openzeppelin-contracts-2-2.6.0 is already installed"
        }));
    }

    #[test]
    #[serial]
    fn remove_lock_single_success() {
        let lock_file = check_lock_file();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string(),
            hash: "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
        };
        let dependencies = vec![dependency.clone()];
        write_lock(&dependencies, LockWriteMode::Append).unwrap();

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
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string(),
            hash: "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
        };
        let dependency2= Dependency {
            name: "@openzeppelin-contracts2".to_string(),
            version: "2.5.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string(),
            hash: "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
        };
        let dependencies = vec![dependency.clone(), dependency2.clone()];
        write_lock(&dependencies, LockWriteMode::Append).unwrap();

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
zip_checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#
        );
    }

    #[test]
    #[serial]
    fn remove_lock_one_fails() {
        let lock_file = check_lock_file();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string(),
            hash: "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
        };

        let dependencies = vec![dependency.clone()];
        write_lock(&dependencies, LockWriteMode::Append).unwrap();

        match remove_lock(&Dependency {
            name: "non-existent".to_string(),
            version: dependency.version.clone(),
            url: String::new(),
            hash: String::new(),
        }) {
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
zip_checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#
        );
    }
}
