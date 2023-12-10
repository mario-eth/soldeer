use crate::config::Dependency;
use crate::utils::get_current_working_dir;
use serde_derive::Deserialize;
use std::fmt;
use std::fs::{self};
use std::path::PathBuf;

extern crate toml_edit;
use std::io::Write;
use std::process::exit;

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
    let lock_file: PathBuf;
    if cfg!(test) {
        lock_file = get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock");
    } else {
        lock_file = get_current_working_dir().unwrap().join("soldeer.lock");
    }

    if !lock_file.exists() {
        return Ok(dependencies.to_vec());
    }
    let lock_path: String = lock_file.to_str().unwrap().to_string();
    // Read the contents of the file using a `match` block
    // to return the `data: Ok(c)` as a `String`
    // or handle any `errors: Err(_)`.
    let contents: String = match fs::read_to_string(&lock_path) {
        // If successful return the files text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(_) => {
            // Write `msg` to `stderr`.
            eprintln!("Could not read file `{}`", &lock_path);
            // Exit the program with exit code `1`.
            exit(1);
        }
    };

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
    let lock_file: PathBuf;
    if cfg!(test) {
        lock_file = get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock");
    } else {
        lock_file = get_current_working_dir().unwrap().join("soldeer.lock");
    }
    let lock_path: String = lock_file.to_str().unwrap().to_string();
    if !lock_file.exists() {
        std::fs::File::create(lock_path.clone()).unwrap();
    }
    println!("lock path {}", lock_path);

    let mut new_lock_entries: String = String::new();
    dependencies.iter().for_each(|dependency| {
        let bytes = std::fs::read(
            get_current_working_dir()
                .unwrap()
                .join("dependencies")
                .join(format!("{}-{}.zip", dependency.name, dependency.version)),
        )
        .unwrap(); // Vec<u8>
        let hash = sha256::digest(&bytes);
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
    if let Err(e) = write!(file, "{}", new_lock_entries.to_string()) {
        eprintln!("Couldn't write to file: {}", e);
    }
    Ok(())
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
    use serial_test::serial;
    use std::io::Write;

    pub fn initialize() {
        let lock_file: PathBuf = get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock");
        if lock_file.exists() {
            fs::remove_file(&lock_file).unwrap();
        }
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
        let lock_file: PathBuf = get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock");
        if lock_file.exists() {
            fs::remove_file(&lock_file).unwrap();
        }
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        let result: Vec<Dependency> = lock_check(&dependencies).unwrap();
        assert_eq!(lock_file.exists(), false);
        initialize();
    }

    #[test]
    #[serial]
    fn write_lock_test() {
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
        result = lock_check(&dependencies).unwrap();
        assert_eq!(result.len(), 0);
        initialize();
    }

    #[test]
    fn check_lock_all_locked_test() {
        let lock_file: PathBuf = get_current_working_dir()
            .unwrap()
            .join("test")
            .join("soldeer.lock");
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
    fn check_lock_not_all_locked_test() {
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
    }
}
