use crate::utils::{ get_current_working_dir, read_file_to_string };
use serde_derive::Deserialize;
use std::fmt::{ self };
use std::process::{ Command, Output };

pub fn load_repositories() -> Result<Vec<String>, LoadError> {
    println!("Loading list of repositories for NPM");
    let filename: String = get_current_working_dir()
        .unwrap()
        .join(String::from("npm_repositories.toml"))
        .to_str()
        .unwrap()
        .to_string();
    let contents = read_file_to_string(filename.clone()).unwrap();
    let data: Data = match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(err) => {
            eprintln!("Error: {}", err);
            // Write `msg` to `stderr`.
            eprintln!("Unable to load data from npm_repositories.toml");
            // Exit the program with exit code `1`.
            return Err(LoadError);
        }
    };

    let mut repositories: Vec<String> = Vec::new();
    data.npm.iter().for_each(|value: &String| {
        repositories.push(value.clone());
    });

    return Ok(repositories);
}

pub fn npm_retrieve_versions(repository: &String) -> Result<Vec<String>, LoadError> {
    let output: Output = Command::new("npm")
        .arg("view")
        .arg(repository)
        .arg("versions")
        .output()
        .expect("failed to execute process");
    // println!("status: {}", String::from_utf8(output.stdout.clone()).unwrap());
    let json_string: String = String::from_utf8(output.stdout.clone()).unwrap().replace("'", "\"");

    let versions: Vec<String> = serde_json
        ::from_str::<Vec<String>>(json_string.as_str())
        .map_err(|err: serde_json::Error| {
            return err;
        })
        .unwrap();
    println!("versions: {:?}", versions);
    return Ok(versions);
}

// TODO: multi-threading
#[warn(dead_code)]
pub fn check_versions_health(
    repository: &String,
    versions: Vec<String>
) -> Result<Vec<String>, HealthCheckError> {
    let mut valid_versions: Vec<String> = Vec::new();
    for version in versions.iter() {
        println!("Health check version: {:?}", version);
        let output: Output = Command::new("npm")
            .arg("i")
            .arg(format!("{}@{}", repository, version))
            .arg("--dry-run")
            .output()
            .expect("failed to execute process");
        if output.status.success() {
            valid_versions.push(version.clone());
        } else {
            println!("Version {} of {} is not valid", version, repository);
        }
    }
    return Ok(valid_versions);
}
pub fn check_version_health(repository: &String, version: &String) -> Result<(), HealthCheckError> {
    let output: Output = Command::new("npm")
        .arg("i")
        .arg(format!("{}@{}", repository, version))
        .arg("--dry-run")
        .output()
        .expect("failed to execute process");
    if output.status.success() {
        return Ok(());
    } else {
        println!("Version {} of {} is not valid", version, repository);
        return Err(HealthCheckError);
    }
}

#[derive(Deserialize)]
#[derive(Debug)]
struct Data {
    npm: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LoadError;

#[derive(Debug, Clone)]
pub struct HealthCheckError;

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "file not found")
    }
}

impl fmt::Display for HealthCheckError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "healthcheck failed")
    }
}
