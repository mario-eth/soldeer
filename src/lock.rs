use crate::config::Dependency;
use crate::errors::LockError;
use crate::utils::{
    get_current_working_dir,
    read_file_to_string,
};
use crate::LOCK_FILE;
use serde_derive::Deserialize;
use std::fs::{
    self,
    remove_file,
};
use std::io::Write;
use std::path::PathBuf;
use yansi::Paint;

// Top level struct to hold the TOML data.
#[derive(Deserialize, Debug)]
pub struct LockEntry {
    name: String,
    version: String,
    source: String,
    checksum: String,
}

impl Clone for LockEntry {
    fn clone(&self) -> LockEntry {
        LockEntry {
            name: String::from(&self.name),
            version: String::from(&self.version),
            source: String::from(&self.source),
            checksum: String::from(&self.checksum),
        }
    }
}

// Top level struct to hold the TOML data.
#[derive(Deserialize, Debug)]
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
        return Err(LockError {
            cause: "Lock does not exists".to_string(),
        });
    }
    let lock_path: String = lock_file.to_str().unwrap().to_string();

    let contents = read_file_to_string(&lock_path);

    // reading the contents into a data structure using toml::from_str
    let data: LockType = match toml::from_str(&contents) {
        Ok(d) => d,
        Err(_err) => {
            return Ok(vec![]);
        }
    };
    Ok(data.dependencies)
}

pub fn lock_check(
    dependency: &Dependency,
    create_lock: bool,
) -> Result<Vec<Dependency>, LockError> {
    let lock_entries = match read_lock() {
        Ok(entries) => entries,
        Err(_) => {
            if create_lock {
                let _ = write_lock(&[], false).map_err(|_| {
                    LockError {
                        cause: "Could not write lock file".to_string(),
                    }
                });
                vec![]
            } else {
                return Err(LockError {
                    cause: "Lock does not exists".to_string(),
                });
            }
        }
    };

    let mut unlock_dependencies: Vec<Dependency> = Vec::new();
    let mut is_locked: bool = false;
    let mut string_err = String::new();
    lock_entries.iter().for_each(|lock_entry| {
        if lock_entry.name == dependency.name && lock_entry.version == dependency.version {
            string_err = format!(
                "Dependency {}-{} is already installed",
                lock_entry.name, lock_entry.version
            );
            is_locked = true;
        }
    });

    unlock_dependencies.push(dependency.clone());

    if is_locked {
        return Err(LockError { cause: string_err });
    }
    Ok(unlock_dependencies)
}

pub fn write_lock(dependencies: &[Dependency], clean: bool) -> Result<(), LockError> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    if clean && (lock_file).exists() {
        match remove_file(&lock_file) {
            Ok(_) => {}
            Err(_) => {
                return Err(LockError {
                    cause: "Could not clean lock file".to_string(),
                })
            }
        }
    }

    if !lock_file.exists() {
        std::fs::File::create(lock_file.to_str().unwrap()).unwrap();
    }

    let mut new_lock_entries: String = String::new();
    dependencies.iter().for_each(|dependency| {
        new_lock_entries.push_str(&format!(
            r#"
[[dependencies]]
name = "{}"
version = "{}"
source = "{}"
checksum = "{}"
"#,
            dependency.name, dependency.version, dependency.url, dependency.hash
        ));
        println!(
            "{}",
            Paint::green(&format!(
                "Writing {}~{} to the lock file.",
                &dependency.name.to_string(),
                &dependency.version.to_string()
            ))
        );
    });
    let mut file: std::fs::File = fs::OpenOptions::new().append(true).open(lock_file).unwrap();
    if write!(file, "{}", new_lock_entries).is_err() {
        return Err(LockError {
            cause: "Could not write to the lock file".to_string(),
        });
    }
    Ok(())
}

pub fn remove_lock(dependency_name: &str, dependency_version: &str) -> Result<(), LockError> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir().join("test").join("soldeer.lock")
    } else {
        LOCK_FILE.clone()
    };

    let entries = match read_lock() {
        Ok(entries) => entries,
        Err(err) => return Err(err),
    };
    let mut new_lock_entries: String = String::new();
    entries.iter().for_each(|entry| {
        if entry.name != dependency_name || entry.version != dependency_version {
            new_lock_entries.push_str(&format!(
                r#"
[[dependencies]]
name = "{}"
version = "{}"
source = "{}"
checksum = "{}"
"#,
                &entry.name, &entry.version, &entry.source, &entry.checksum
            ));
        }
    });
    let mut file: std::fs::File = fs::OpenOptions::new()
        .truncate(true)
        .write(true)
        .open(lock_file)
        .unwrap();
    if write!(file, "{}", new_lock_entries).is_err() {
        return Err(LockError {
            cause: "Could not write to the lock file".to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Dependency;
    use crate::utils::read_file_to_string;
    use serial_test::serial;
    use std::fs::File;
    use std::io::Write;

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
                    
[[dependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#;
        File::create(lock_file)
            .unwrap()
            .write_all(lock_contents.as_bytes())
            .unwrap();
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
        write_lock(&dependencies, false).unwrap();
        assert!(lock_check(&dependency, true).is_err_and(|e| {
            e.cause == "Dependency @openzeppelin-contracts-2.5.0 is already installed"
        }));
        let contents = read_file_to_string(&lock_file.to_str().unwrap().to_string());

        assert_eq!(
            contents,
            r#"
[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.5.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
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
        write_lock(&dependencies, false).unwrap();
        let contents = read_file_to_string(&lock_file.to_str().unwrap().to_string());

        assert_eq!(
            contents,
            r#"
[[dependencies]]
name = "@openzeppelin-contracts"
version = "2.3.0"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip"
checksum = "a2d469062adeb62f7a4aada78237acae4ad3c168ba65c3ac9c76e290332c11ec"
                    
[[dependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"

[[dependencies]]
name = "@openzeppelin-contracts-2"
version = "2.6.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.6.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#
        );

        assert!(lock_check(&dependency, true).is_err_and(|e| {
            e.cause == "Dependency @openzeppelin-contracts-2-2.6.0 is already installed"
        }));
    }
}
