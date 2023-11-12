use std::fs::{
    metadata,
    remove_file,
};

use crate::config::Dependency;
use crate::utils::get_current_working_dir;

#[derive(Debug)]
pub struct MissingDependencies {
    pub name: String,
}

impl MissingDependencies {
    fn new(msg: &str) -> MissingDependencies {
        MissingDependencies {
            name: msg.to_string(),
        }
    }
}

// Health-check dependencies before we clean them, this one checks if they were unzipped
pub fn healthcheck_dependencies(dependencies: &[Dependency]) -> Result<(), MissingDependencies> {
    println!("Health-checking dependencies...");
    for dependency in dependencies.iter() {
        match healthcheck_dependency(&dependency.name, &dependency.version) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Error health-checking dependency: {:?}", err);
                return Err(err);
            }
        }
    }
    Ok(())
}

// Cleanup dependencies after we are downloaded them
pub fn cleanup_after(dependencies: &[Dependency]) -> Result<(), MissingDependencies> {
    println!("Cleanup dependencies...");
    for dependency in dependencies.iter() {
        match cleanup_dependency(&dependency.name, &dependency.version) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Error cleanup dependency: {:?}", err);
                return Err(err);
            }
        }
    }
    Ok(())
}

pub fn healthcheck_dependency(
    dependency_name: &str,
    dependency_version: &str,
) -> Result<(), MissingDependencies> {
    println!(
        "Health-checking dependency {}-{}",
        dependency_name, dependency_version
    );
    let file_name: String = format!("{}-{}", &dependency_name, &dependency_version);
    let new_path: std::path::PathBuf = get_current_working_dir().unwrap().join("dependencies");
    match metadata(new_path.join(file_name)) {
        Ok(_) => Ok(()),
        Err(_) => Err(MissingDependencies::new(dependency_name)),
    }
}

pub fn cleanup_dependency(
    dependency_name: &str,
    dependency_version: &str,
) -> Result<(), MissingDependencies> {
    println!(
        "Cleaning up dependency {}-{}",
        dependency_name, dependency_version
    );
    let file_name: String = format!("{}-{}.zip", dependency_name, dependency_version);
    let new_path: std::path::PathBuf = get_current_working_dir().unwrap().join("dependencies");
    match remove_file(new_path.join(file_name)) {
        Ok(_) => Ok(()),
        Err(_) => Err(MissingDependencies::new(dependency_name)),
    }
}
