use crate::config::Dependency;
use crate::utils::{
    get_current_working_dir,
    read_file_to_string,
};
use serde_derive::Deserialize;
use std::fmt;
use std::fs::{
    self,
};
use std::path::PathBuf;

extern crate toml_edit;
use std::io::Write;

// Top level struct to hold the TOML data.
#[derive(Deserialize, Debug)]
struct LockEntry {
    name: String,
    version: String,
}
// Top level struct to hold the TOML data.
#[derive(Deserialize, Debug)]
struct LockType {
    sdependencies: Vec<LockEntry>,
}

pub fn lock_check(dependencies: &[Dependency]) -> Result<Vec<Dependency>, LockError> {
    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock")
    } else {
        get_current_working_dir().unwrap().join("soldeer.lock")
    };

    if !lock_file.exists() {
        return Ok(dependencies.to_vec());
    }
    let lock_path: String = lock_file.to_str().unwrap().to_string();

    let contents = read_file_to_string(&lock_path);

    // Use a `match` block to return the
    // file `contents` as a `LockEntry struct: Ok(d)`
    // or handle any `errors: Err(_)`.
    let data: LockType = match toml::from_str(&contents) {
        // If successful, return data as `LockEntry` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(_err) => {
            eprintln!("Lock file might be empty");
            return Ok(dependencies.to_vec());
        }
    };
    let mut unlock_dependencies: Vec<Dependency> = Vec::new();
    dependencies.iter().for_each(|dependency| {
        let mut is_locked: bool = false;
        data.sdependencies.iter().for_each(|lock_entry| {
            if lock_entry.name == dependency.name && lock_entry.version == dependency.version {
                println!(
                    "Dependency {}-{} is locked",
                    lock_entry.name, lock_entry.version
                );
                is_locked = true;
            }
        });
        if !is_locked {
            unlock_dependencies.push(dependency.clone());
        }
    });
    Ok(unlock_dependencies)
}

pub fn write_lock(dependencies: &[Dependency]) -> Result<(), LockError> {
    println!("Writing lock file...");

    let lock_file: PathBuf = if cfg!(test) {
        get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock")
    } else {
        get_current_working_dir().unwrap().join("soldeer.lock")
    };

    let lock_path: String = lock_file.to_str().unwrap().to_string();
    if !lock_file.exists() {
        std::fs::File::create(lock_path.clone()).unwrap();
    }

    let mut new_lock_entries: String = String::new();
    dependencies.iter().for_each(|dependency| {
        let hash = sha256_digest(&dependency.name, &dependency.version);
        new_lock_entries.push_str(&format!(
            r#"
[[sdependencies]]
name = "{}"
version = "{}"
source = "{}"
checksum = "{}"
"#,
            dependency.name, dependency.version, dependency.url, hash
        ));
    });
    let mut file: std::fs::File = fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open(lock_file)
        .unwrap();
    if let Err(e) = write!(file, "{}", new_lock_entries) {
        eprintln!("Couldn't write to file: {}", e);
    }
    Ok(())
}

#[cfg(not(test))]
fn sha256_digest(dependency_name: &str, dependency_version: &str) -> String {
    let bytes = std::fs::read(
        get_current_working_dir()
            .unwrap()
            .join("dependencies")
            .join(format!("{}-{}.zip", dependency_name, dependency_version)),
    )
    .unwrap(); // Vec<u8>
    sha256::digest(bytes)
}

#[cfg(test)]
fn sha256_digest(_dependency_name: &str, _dependency_version: &str) -> String {
    return "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string();
}

#[derive(Debug, Clone)]
pub struct LockError;

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "lock failed")
    }
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
        let lock_file: PathBuf = get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock");
        if lock_file.exists() {
            fs::remove_file(&lock_file).unwrap();
        }
        lock_file
    }

    pub fn initialize() {
        let lock_file = check_lock_file();
        let lock_contents = r#"
[[sdependencies]]
name = "@openzeppelin-contracts"
version = "2.3.0"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip"
checksum = "a2d469062adeb62f7a4aada78237acae4ad3c168ba65c3ac9c76e290332c11ec"
                    
[[sdependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#;
        File::create(&lock_file)
            .unwrap()
            .write_all(lock_contents.as_bytes())
            .unwrap();
    }

    #[test]
    #[serial]
    fn lock_file_not_present_test() {
        let lock_file = check_lock_file();
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        let _result: Vec<Dependency> = lock_check(&dependencies).unwrap();
        assert_eq!(lock_file.exists(), false);
    }

    #[test]
    #[serial]
    fn check_lock_all_locked_test() {
        initialize();
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        let result: Vec<Dependency> = lock_check(&dependencies).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    #[serial]
    fn check_lock_not_all_locked_test() {
        initialize();
        let lock_file = get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock");
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.4.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.4.0.zip".to_string(),
        });
        let result: Vec<Dependency> = lock_check(&dependencies).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "@openzeppelin-contracts");
        assert_eq!(result[0].version, "2.4.0");
        fs::remove_file(&lock_file).unwrap();
    }

    #[test]
    #[serial]
    fn write_clean_lock_test() {
        let lock_file = check_lock_file();
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.5.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip".to_string(),
        });
        let mut result: Vec<Dependency> = lock_check(&dependencies).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "@openzeppelin-contracts");
        assert_eq!(result[0].version, "2.5.0");
        write_lock(&result).unwrap();
        let contents = read_file_to_string(&lock_file.to_str().unwrap().to_string());

        assert_eq!(
            contents,
            r#"
[[sdependencies]]
name = "@openzeppelin-contracts"
version = "2.5.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.5.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#
        );
        result = lock_check(&dependencies).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    #[serial]
    fn write_append_lock_test() {
        let lock_file = check_lock_file();
        initialize();
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts-2".to_string(),
            version: "2.6.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.6.0.zip".to_string(),
        });
        write_lock(&dependencies).unwrap();
        let contents = read_file_to_string(&lock_file.to_str().unwrap().to_string());

        assert_eq!(
            contents,
            r#"
[[sdependencies]]
name = "@openzeppelin-contracts"
version = "2.3.0"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip"
checksum = "a2d469062adeb62f7a4aada78237acae4ad3c168ba65c3ac9c76e290332c11ec"
                    
[[sdependencies]]
name = "@prb-test"
version = "0.6.5"
source = "registry+https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@prb-test~0.6.5.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"

[[sdependencies]]
name = "@openzeppelin-contracts-2"
version = "2.6.0"
source = "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.6.0.zip"
checksum = "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016"
"#
        );
        let result = lock_check(&dependencies).unwrap();
        assert_eq!(result.len(), 0);
    }
}
