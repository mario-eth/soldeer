use futures::StreamExt;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use tokio::{
    fs::File,
    io::AsyncWriteExt,
};
use yansi::Paint;

use crate::config::Dependency;
use crate::errors::DownloadError;
use crate::errors::UnzippingError;
use crate::remote::get_dependency_url_remote;
use crate::utils::read_file;
use crate::DEPENDENCY_DIR;

pub async fn download_dependencies(
    dependencies: &[Dependency],
    clean: bool,
) -> Result<(), DownloadError> {
    // clean dependencies folder if flag is true
    if clean {
        clean_dependency_directory();
    }
    // downloading dependencies to dependencies folder
    for dependency in dependencies.iter() {
        let file_name: String = format!("{}-{}.zip", dependency.name, dependency.version);
        match download_dependency(&file_name, &dependency.url).await {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }
    }
    Ok(())
}

// un-zip-ing dependencies to dependencies folder
pub fn unzip_dependencies(dependencies: &[Dependency]) -> Result<(), UnzippingError> {
    for dependency in dependencies.iter() {
        match unzip_dependency(&dependency.name, &dependency.version) {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }
    }
    Ok(())
}

pub async fn download_dependency_remote(
    dependency_name: &String,
    dependency_version: &String,
) -> Result<String, DownloadError> {
    let dependency_url = match get_dependency_url_remote(dependency_name, dependency_version).await
    {
        Ok(url) => url,
        Err(err) => return Err(err),
    };

    match download_dependency(
        &format!("{}-{}.zip", &dependency_name, &dependency_version),
        &dependency_url,
    )
    .await
    {
        Ok(_) => Ok(dependency_url),
        Err(err) => Err(err),
    }
}

pub async fn download_dependency(
    dependency_name: &str,
    dependency_url: &str,
) -> Result<(), DownloadError> {
    let dependency_directory: PathBuf = DEPENDENCY_DIR.clone();
    if !DEPENDENCY_DIR.is_dir() {
        fs::create_dir(&dependency_directory).unwrap();
    }

    let mut stream = match reqwest::get(dependency_url).await {
        Ok(res) => {
            if res.status() != 200 {
                return Err(DownloadError {
                    name: dependency_name.to_string(),
                    version: dependency_url.to_string(),
                    cause: format!("Could not download, status: {:?}", res.status()),
                });
            }
            res.bytes_stream()
        }
        Err(_) => {
            return Err(DownloadError {
                name: dependency_name.to_string(),
                version: dependency_url.to_string(),
                cause: "Unknown error".to_string(),
            });
        }
    };

    let mut file = File::create(&dependency_directory.join(dependency_name))
        .await
        .unwrap();

    while let Some(chunk_result) = stream.next().await {
        match file.write_all(&chunk_result.unwrap()).await {
            Ok(_) => {}
            Err(_) => {
                return Err(DownloadError {
                    name: dependency_name.to_string(),
                    version: dependency_url.to_string(),
                    cause: "Unknown error".to_string(),
                });
            }
        }
    }

    match file.flush().await {
        Ok(_) => {}
        Err(_) => {
            return Err(DownloadError {
                name: dependency_name.to_string(),
                version: dependency_url.to_string(),
                cause: "Unknown error".to_string(),
            });
        }
    };

    println!(
        "{}",
        Paint::green(&format!("Dependency {dependency_name} downloaded!"))
    );

    Ok(())
}

pub fn unzip_dependency(
    dependency_name: &String,
    dependency_version: &String,
) -> Result<(), UnzippingError> {
    let file_name = format!("{}-{}.zip", dependency_name, dependency_version);
    let target_name = format!("{}-{}/", dependency_name, dependency_version);
    let current_dir = DEPENDENCY_DIR.join(file_name);
    let target = DEPENDENCY_DIR.join(target_name);
    let archive = read_file(current_dir).unwrap();

    match zip_extract::extract(Cursor::new(archive), &target, true) {
        Ok(_) => {}
        Err(_) => {
            return Err(UnzippingError {
                name: dependency_name.to_string(),
                version: dependency_version.to_string(),
            });
        }
    }
    println!(
        "{}",
        Paint::green(&format!(
            "The dependency {}-{} was unzipped!",
            dependency_name, dependency_version
        ))
    );
    Ok(())
}

pub fn clean_dependency_directory() {
    if DEPENDENCY_DIR.is_dir() {
        fs::remove_dir_all(DEPENDENCY_DIR.clone()).unwrap();
        fs::create_dir(DEPENDENCY_DIR.clone()).unwrap();
    }
}

#[cfg(test)]
#[allow(clippy::vec_init_then_push)]
mod tests {
    use super::*;
    use crate::janitor::healthcheck_dependency;
    use serial_test::serial;
    use std::env;
    use std::fs::metadata;
    use std::path::Path;

    #[tokio::test]
    #[serial]
    async fn download_dependencies_one_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        };
        dependencies.push(dependency.clone());
        download_dependencies(&dependencies, false).await.unwrap();
        let path_zip =
            DEPENDENCY_DIR.join(format!("{}-{}.zip", &dependency.name, &dependency.version));
        assert!(Path::new(&path_zip).exists());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_two_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let  dependency_one = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        };
        dependencies.push(dependency_one.clone());

        let dependency_two = Dependency {
            name: "@uniswap-v2-core".to_string(),
            version: "1.0.0-beta.4".to_string(),
            url: "https://soldeer-revisions.s3.amazonaws.com/@uniswap-v2-core/1_0_0-beta_4_22-01-2024_13:18:27_v2-core.zip".to_string(),
        };

        dependencies.push(dependency_two.clone());
        download_dependencies(&dependencies, false).await.unwrap();
        let mut path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_one.name, &dependency_one.version
        ));
        assert!(Path::new(&path_zip).exists());

        path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_two.name, &dependency_two.version
        ));
        assert!(Path::new(&path_zip).exists());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependency_should_replace_existing_zip() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let  dependency_one = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "download-dep-v1".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        };
        dependencies.push(dependency_one.clone());

        download_dependencies(&dependencies, false).await.unwrap();
        let path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_one.name, &dependency_one.version
        ));
        let size_of_one = fs::metadata(Path::new(&path_zip)).unwrap().len();

        let  dependency_two = Dependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "download-dep-v1".to_string(),
                url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.4.0.zip".to_string(),
            };

        dependencies = Vec::new();
        dependencies.push(dependency_two.clone());

        download_dependencies(&dependencies, false).await.unwrap();
        let size_of_two = fs::metadata(Path::new(&path_zip)).unwrap().len();

        assert!(size_of_two > size_of_one);
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_one_with_clean_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency_old = Dependency {
            name: "@uniswap-v2-core".to_string(),
            version: "1.0.0-beta.4".to_string(),
            url: "https://soldeer-revisions.s3.amazonaws.com/@uniswap-v2-core/1_0_0-beta_4_22-01-2024_13:18:27_v2-core.zip".to_string(),
        };

        dependencies.push(dependency_old.clone());
        download_dependencies(&dependencies, false).await.unwrap();

        // making sure the dependency exists so we can check the deletion
        let path_zip_old = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_old.name, &dependency_old.version
        ));
        assert!(Path::new(&path_zip_old).exists());

        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        };
        dependencies = Vec::new();
        dependencies.push(dependency.clone());

        download_dependencies(&dependencies, true).await.unwrap();
        let path_zip =
            DEPENDENCY_DIR.join(format!("{}-{}.zip", &dependency.name, &dependency.version));
        assert!(!Path::new(&path_zip_old).exists());
        assert!(Path::new(&path_zip).exists());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_one_fail() {
        let mut dependencies: Vec<Dependency> = Vec::new();

        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~.zip".to_string(),
        };
        dependencies.push(dependency.clone());

        match download_dependencies(&dependencies, false).await {
            Ok(_) => {
                assert_eq!("Invalid state", "");
            }
            Err(err) => {
                assert_eq!(err.cause, "Could not download, status: 404");
            }
        }
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn unzip_dependency_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        };
        dependencies.push(dependency.clone());
        download_dependencies(&dependencies, false).await.unwrap();
        let path = DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name, &dependency.version));
        match unzip_dependencies(&dependencies) {
            Ok(_) => {
                assert!(path.exists());
                assert!(metadata(&path).unwrap().len() > 0);
            }
            Err(_) => {
                clean_dependency_directory();
                assert_eq!("Error", "");
            }
        }
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn unzip_non_zip_file_error() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://freetestdata.com/wp-content/uploads/2022/02/Free_Test_Data_117KB_JPG.jpg"
                .to_string(),
        };
        dependencies.push(dependency.clone());
        download_dependencies(&dependencies, false).await.unwrap();
        match unzip_dependencies(&dependencies) {
            Ok(_) => {
                clean_dependency_directory();
                assert_eq!("Wrong State", "");
            }
            Err(err) => {
                assert_eq!(
                    err,
                    UnzippingError {
                        name: dependency.name.to_string(),
                        version: dependency.version.to_string(),
                    }
                );
            }
        }
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependency_remote_success() {
        env::set_var("base_url", "https://api.soldeer.xyz");

        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "".to_string(),
        };
        download_dependency_remote(&dependency.name, &dependency.version)
            .await
            .unwrap();

        let path_zip =
            DEPENDENCY_DIR.join(format!("{}-{}.zip", &dependency.name, &dependency.version));
        assert!(Path::new(&path_zip).exists());
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependency_remote_non_existent_fail() {
        env::set_var("base_url", "https://api.soldeer.xyz");

        let dependency = Dependency {
            name: "@wrong-dependency".to_string(),
            version: "2.3.0".to_string(),
            url: "".to_string(),
        };
        match download_dependency_remote(&dependency.name, &dependency.version).await {
            Ok(_) => {
                clean_dependency_directory();
                assert_eq!("Wrong State", "");
            }
            Err(err) => {
                assert_eq!(
                    err,
                    DownloadError {
                        name: dependency.name.to_string(),
                        version: dependency.version.to_string(),
                        cause: "Could not get the dependency URL".to_string(),
                    }
                )
            }
        }

        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_unzip_check_integrity() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "3.3.0-custom-test".to_string(),
            url: "https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip".to_string(),
        });
        download_dependencies(&dependencies, false).await.unwrap();
        unzip_dependency(&dependencies[0].name, &dependencies[0].version).unwrap();
        healthcheck_dependency("@openzeppelin-contracts", "3.3.0-custom-test").unwrap();
        assert!(Path::new(
            &DEPENDENCY_DIR
                .join("@openzeppelin-contracts-3.3.0-custom-test")
                .join("token")
                .join("ERC20")
                .join("ERC20.sol")
        )
        .exists());
        clean_dependency_directory()
    }
}
