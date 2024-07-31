use std::io;
use std::{
    env,
    fs::{
        self,
        create_dir_all,
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
        rev: None,
    });

    match soldeer::run(command) {
        Ok(_) => {}
        Err(_) => {
            assert_eq!("Invalid State", "")
        }
    }

    let path_dependency = DEPENDENCY_DIR.join("forge-std-1.8.2");
    assert!(path_dependency.exists());
    let test_contract = r#"
// SPDX-License-Identifier: MIT
pragma solidity >=  0.8.20;

contract Increment {
    uint256 i;

    function increment() external {
        i++;
    }
}
    "#;

    let test = r#"
// SPDX-License-Identifier: MIT
pragma solidity >= 0.8.20;
import "../src/Increment.sol";
import "@forge-std-1.8.2/src/Test.sol";

contract TestSoldeer is Test {
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

    let _ = create_dir_all(test_project.join("dependencies").join("forge-std-1.8.2"));

    let _ = copy_dir_all(
        env::current_dir()
            .unwrap()
            .join("dependencies")
            .join("forge-std-1.8.2"),
        test_project.join("dependencies").join("forge-std-1.8.2"),
    );

    let _ = fs::copy(
        env::current_dir().unwrap().join("foundry.toml"),
        test_project.join("foundry.toml"),
    );

    let _ = fs::copy(
        env::current_dir().unwrap().join("remappings.txt"),
        test_project.join("remappings.txt"),
    );

    let output = Command::new("forge")
        .arg("test")
        .arg("--root")
        .arg(&test_project)
        .output()
        .expect("failed to execute process");

    let passed = String::from_utf8(output.stdout).unwrap().contains("[PASS]");
    if !passed {
        println!(
            "This will fail with: {:?}",
            String::from_utf8(output.stderr).unwrap()
        );
    }
    assert!(passed);
    clean_test_env(&test_project);
}

#[test]
#[serial]
fn soldeer_install_invalid_dependency() {
    let command = Subcommands::Install(Install {
        dependency: Some("forge-std".to_string()),
        remote_url: None,
        rev: None,
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

    assert!(!path_zip.exists());
    assert!(!path_dependency.exists());
}

fn clean_test_env(test_project: &PathBuf) {
    let _ = remove_dir_all(DEPENDENCY_DIR.clone());
    let _ = remove_file(LOCK_FILE.clone());
    let _ = remove_dir_all(test_project);
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
