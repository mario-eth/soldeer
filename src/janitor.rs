use std::fs::{
    metadata,
    remove_file,
};

use crate::config::Dependency;
use crate::errors::MissingDependencies;
use crate::utils::get_current_working_dir;

// Health-check dependencies before we clean them, this one checks if they were unzipped
pub fn healthcheck_dependencies(dependencies: &[Dependency]) -> Result<(), MissingDependencies> {
    for dependency in dependencies.iter() {
        match healthcheck_dependency(&dependency.name, &dependency.version) {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }
    }
    Ok(())
}

// Cleanup dependencies after we are downloaded them
pub fn cleanup_after(dependencies: &[Dependency]) -> Result<(), MissingDependencies> {
    for dependency in dependencies.iter() {
        match cleanup_dependency(&dependency.name, &dependency.version) {
            Ok(_) => {}
            Err(err) => {
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
    let file_name: String = format!("{}-{}", &dependency_name, &dependency_version);
    let new_path: std::path::PathBuf = get_current_working_dir().unwrap().join("dependencies");
    match metadata(new_path.join(file_name)) {
        Ok(_) => Ok(()),
        Err(_) => {
            Err(MissingDependencies::new(
                dependency_name,
                dependency_version,
            ))
        }
    }
}

pub fn cleanup_dependency(
    dependency_name: &str,
    dependency_version: &str,
) -> Result<(), MissingDependencies> {
    let file_name: String = format!("{}-{}.zip", dependency_name, dependency_version);
    let new_path: std::path::PathBuf = get_current_working_dir().unwrap().join("dependencies");
    match remove_file(new_path.join(&file_name)) {
        Ok(_) => Ok(()),
        Err(err) => {
            println!("{:?}", err);
            println!("{:?}", new_path.join(&file_name));
            Err(MissingDependencies::new(
                dependency_name,
                dependency_version,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency_downloader::{
        clean_dependency_directory,
        download_dependencies,
        unzip_dependency,
    };
    use serial_test::serial;

    // Helper macro to run async tests
    macro_rules! aw {
        ($e:expr) => {
            tokio_test::block_on($e)
        };
    }
    #[test]
    fn healthcheck_dependency_not_found() {
        let result: Result<(), MissingDependencies> = healthcheck_dependency("test", "1.0.0");
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn healthcheck_dependency_found() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        let _ = aw!(download_dependencies(&dependencies, false));
        let _ = unzip_dependency(&dependencies[0].name, &dependencies[0].version);
        let result: Result<(), MissingDependencies> =
            healthcheck_dependency("@openzeppelin-contracts", "2.3.0");
        assert!(result.is_ok());

        clean_dependency_directory();
    }

    #[test]
    #[serial]
    fn cleanup_existing_dependency() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        let _ = aw!(download_dependencies(&dependencies, false));
        let _ = unzip_dependency(&dependencies[0].name, &dependencies[0].version);
        let result: Result<(), MissingDependencies> =
            cleanup_dependency("@openzeppelin-contracts", "2.3.0");
        assert!(result.is_ok());
        clean_dependency_directory();
    }

    #[test]
    #[serial]
    fn cleanup_nonexisting_dependency() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        let result: Result<(), MissingDependencies> =
            cleanup_dependency("@openzeppelin-contracts", "2.3.0");
        assert!(result.is_err());
        clean_dependency_directory();
    }

    #[test]
    #[serial]
    fn cleanup_after_existing_dependency() {
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

        let _ = aw!(download_dependencies(&dependencies, false));
        let _ = unzip_dependency(&dependencies[0].name, &dependencies[0].version);
        let result: Result<(), MissingDependencies> = cleanup_after(&dependencies);
        assert!(result.is_ok());
        clean_dependency_directory();
    }

    #[test]
    #[serial]
    fn cleanup_after_one_existing_one_not_existing_dependency() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });

        let _ = aw!(download_dependencies(&dependencies, false));
        let _ = unzip_dependency(&dependencies[0].name, &dependencies[0].version);
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.4.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.4.0.zip".to_string(),
        });
        let result: Result<(), MissingDependencies> = cleanup_after(&dependencies);
        assert!(result.is_err());
        let error = result.err().unwrap();
        assert!(error.name == "@openzeppelin-contracts");
        assert!(error.version == "2.4.0");
        clean_dependency_directory();
    }
}
