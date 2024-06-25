use std::{
    env,
    fs::{
        self,
        remove_dir_all,
        remove_file,
    },
    path::{
        Path,
        PathBuf,
    },
    process::Command,
};

use serial_test::serial;
use soldeer::{
    commands::{
        Install,
        Subcommands,
    },
    errors::SoldeerError,
    DEPENDENCY_DIR,
    LOCK_FILE,
};
use std::io::Write;

extern crate soldeer;

#[test]
#[serial]
fn soldeer_install_valid_dependency() {
    let test_project = env::current_dir().unwrap().join("test_project");
    clean_test_env(&test_project);
    let command = Subcommands::Install(Install {
        dependency: Some("forge-std~1.8.2".to_string()),
        remote_url: None,
    });

    match soldeer::run(command) {
        Ok(_) => {}
        Err(_) => {
            assert_eq!("Invalid State", "")
        }
    }

    let path_dependency = DEPENDENCY_DIR.join("forge-std-1.8.2");
    assert!(Path::new(&path_dependency).exists());
    let test_contract = r#"
// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;

contract Increment {
    uint256 i;

    function increment() external {
        i++;
    }
}
    "#;

    let test = r#"
// SPDX-License-Identifier: MIT
pragma solidity 0.8.20;
import "../src/Increment.sol";

contract Test {
    Increment t = new Increment();

    function testIncrement() external {
        t.increment();
    }
}
    "#;

    let _ = fs::create_dir(&test_project);
    let _ = fs::create_dir(test_project.join("src"));
    let _ = fs::create_dir(test_project.join("test"));
    let mut file: std::fs::File = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(test_project.join("src").join("Increment.sol"))
        .unwrap();
    if write!(file, "{}", test_contract).is_err() {
        println!("Error on writing test file");
        assert_eq!("Invalid state", "");
    }

    let mut file: std::fs::File = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(test_project.join("test").join("TestIncrement.sol"))
        .unwrap();
    if write!(file, "{}", test).is_err() {
        println!("Error on writing test file");
        assert_eq!("Invalid state", "");
    }

    let output = Command::new("forge")
        .arg("test")
        .arg("--root")
        .arg(&test_project)
        .output()
        .expect("failed to execute process");
    assert!(String::from_utf8(output.stdout).unwrap().contains("[PASS]"));
    clean_test_env(&test_project);
}

#[test]
#[serial]
fn soldeer_install_invalid_dependency() {
    let command = Subcommands::Install(Install {
        dependency: Some("forge-std".to_string()),
        remote_url: None,
    });

    match soldeer::run(command) {
        Ok(_) => {
            assert_eq!("Invalid State", "")
        }
        Err(err) => {
            assert_eq!(
                err,
                SoldeerError{
                   message: "Dependency forge-std does not specify a version.\nThe format should be [DEPENDENCY]~[VERSION]".to_string()
                }
            );
        }
    }

    let path_dependency = DEPENDENCY_DIR.join("forge-std");
    let path_zip = DEPENDENCY_DIR.join("forge-std.zip");

    assert!(!Path::new(&path_zip).exists());
    assert!(!Path::new(&path_dependency).exists());
}

fn clean_test_env(test_project: &PathBuf) {
    let _ = remove_dir_all(DEPENDENCY_DIR.clone());
    let _ = remove_file(LOCK_FILE.clone());
    let _ = remove_dir_all(test_project);
}
