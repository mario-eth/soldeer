use crate::{config::Dependency, errors::MissingDependencies, lock::remove_lock, DEPENDENCY_DIR};
use std::fs::{metadata, remove_dir_all, remove_file};

// Health-check dependencies before we clean them, this one checks if they were unzipped
pub fn healthcheck_dependencies(dependencies: &[Dependency]) -> Result<(), MissingDependencies> {
    dependencies.iter().try_for_each(healthcheck_dependency)?;
    Ok(())
}

// Cleanup zips after the download
pub fn cleanup_after(dependencies: &[Dependency]) -> Result<(), MissingDependencies> {
    dependencies.iter().try_for_each(|d| cleanup_dependency(d, false))?;
    Ok(())
}

pub fn healthcheck_dependency(dependency: &Dependency) -> Result<(), MissingDependencies> {
    let file_name: String = format!("{}-{}", dependency.name(), dependency.version());
    let new_path = DEPENDENCY_DIR.join(file_name);
    match metadata(new_path) {
        Ok(_) => Ok(()),
        Err(_) => Err(MissingDependencies::new(dependency.name(), dependency.version())),
    }
}

pub fn cleanup_dependency(dependency: &Dependency, full: bool) -> Result<(), MissingDependencies> {
    let file_name: String = format!("{}-{}.zip", dependency.name(), dependency.version());
    let new_path: std::path::PathBuf = DEPENDENCY_DIR.clone().join(file_name);
    if let Dependency::Http(dep) = dependency {
        match remove_file(new_path) {
            Ok(_) => {}
            Err(_) => {
                return Err(MissingDependencies::new(&dep.name, &dep.version));
            }
        };
    }
    if full {
        let dir = DEPENDENCY_DIR.join(dependency.name());
        remove_dir_all(dir).unwrap();
        match remove_lock(dependency) {
            Ok(_) => {}
            Err(_) => return Err(MissingDependencies::new(dependency.name(), dependency.version())),
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::vec_init_then_push)]
mod tests {
    use super::*;
    use crate::{
        config::HttpDependency,
        dependency_downloader::{
            clean_dependency_directory, download_dependencies, unzip_dependency,
        },
    };
    use serial_test::serial;

    struct CleanupDependency;
    impl Drop for CleanupDependency {
        fn drop(&mut self) {
            clean_dependency_directory();
        }
    }

    #[tokio::test]
    async fn healthcheck_dependency_not_found() {
        let _ = healthcheck_dependency(&Dependency::Http(HttpDependency {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            url: None,
            checksum: None,
        }))
        .unwrap_err();
    }

    #[tokio::test]
    #[serial]
    async fn healthcheck_dependency_found() {
        let _cleanup = CleanupDependency;

        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None}));
        download_dependencies(&dependencies, false).await.unwrap();
        unzip_dependency(dependencies[0].name(), dependencies[0].version()).unwrap();
        healthcheck_dependency(&dependencies[0]).unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn cleanup_existing_dependency() {
        let _cleanup = CleanupDependency;

        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None }));
        download_dependencies(&dependencies, false).await.unwrap();
        unzip_dependency(dependencies[0].name(), dependencies[0].version()).unwrap();
        cleanup_dependency(&dependencies[0], false).unwrap();
    }

    #[test]
    #[serial]
    fn cleanup_nonexisting_dependency() {
        let _cleanup = CleanupDependency;

        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "v-cleanup-nonexisting".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None}));
        cleanup_dependency(&dependencies[0], false).unwrap_err();
    }

    #[tokio::test]
    #[serial]
    async fn cleanup_after_existing_dependency() {
        let _cleanup = CleanupDependency;

        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None}));
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.4.0".to_string(),
            url:Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.4.0.zip".to_string()),
            checksum: None }));

        download_dependencies(&dependencies, false).await.unwrap();
        let _ = unzip_dependency(dependencies[0].name(), dependencies[0].version());
        let result: Result<(), MissingDependencies> = cleanup_after(&dependencies);
        assert!(result.is_ok());
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn cleanup_after_one_existing_one_not_existing_dependency() {
        let _cleanup = CleanupDependency;

        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "cleanup-after-one-existing".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None}));

        download_dependencies(&dependencies, false).await.unwrap();
        unzip_dependency(dependencies[0].name(), dependencies[0].version()).unwrap();
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "cleanup-after-one-existing-2".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.4.0.zip".to_string()),
            checksum: None}));
        match cleanup_after(&dependencies) {
            Ok(_) => {
                assert_eq!("Invalid State", "");
            }
            Err(error) => {
                assert!(error.name == "@openzeppelin-contracts");
                assert!(error.version == "cleanup-after-one-existing-2");
            }
        }
    }
}
