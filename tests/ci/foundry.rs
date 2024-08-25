/*
#[test]
#[serial]
fn soldeer_install_valid_dependency() {
    let test_project = env::current_dir().unwrap().join("test_project");
    clean_test_env(&test_project);
    let command = Subcommands::Install(Install {
        dependency: Some("forge-std~1.8.2".to_string()),
        remote_url: None,
        rev: None,
        regenerate_remappings: false,
        recursive_deps: false,
        clean: false,
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
        env::current_dir().unwrap().join("dependencies").join("forge-std-1.8.2"),
        test_project.join("dependencies").join("forge-std-1.8.2"),
    );
    let foundry_content = r#"

# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
forge-std = "1.8.2"

"#;

    let _ = fs::write(test_project.join("foundry.toml"), foundry_content);

    let _ = fs::write(
        test_project.join("remappings.txt"),
        "@forge-std-1.8.2=dependencies/forge-std-1.8.2",
    );

    let output = Command::new("forge")
        .arg("test")
        .arg("--root")
        .arg(&test_project)
        .output()
        .expect("failed to execute process");

    let passed = String::from_utf8(output.stdout).unwrap().contains("[PASS]");
    if !passed {
        eprintln!("This failed with: {:?}", String::from_utf8(output.stderr).unwrap());
    }
    assert!(passed);
    clean_test_env(&test_project);
}

#[test]
#[serial]
fn soldeer_install_invalid_dependency() {
    assert!(Args::try_parse_from(["soldeer", "install", "forge-std"]).is_err());

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
 */
