use futures::StreamExt;
use git2::Oid;
use git2::Repository;
use std::error::Error;
use std::fs;
use std::fs::remove_dir_all;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use tokio::{
    fs::File,
    io::AsyncWriteExt,
};
use yansi::Paint;

use crate::config::Dependency;
use crate::errors::DownloadError;
use crate::errors::UnzippingError;
use crate::utils::get_download_tunnel;
use crate::utils::read_file;
use crate::utils::sha256_digest;
use crate::DEPENDENCY_DIR;

pub async fn download_dependencies(
    dependencies: &[Dependency],
    clean: bool,
) -> Result<Vec<String>, DownloadError> {
    // clean dependencies folder if flag is true
    if clean {
        clean_dependency_directory();
    }
    // downloading dependencies to dependencies folder
    let hashes: Vec<String> = futures::future::join_all(
        dependencies
            .iter()
            .map(|dep| async { download_dependency(&dep.clone()).await }),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<String>, DownloadError>>()?;

    Ok(hashes)
}

// un-zip-ing dependencies to dependencies folder
pub fn unzip_dependencies(dependencies: &[Dependency]) -> Result<(), UnzippingError> {
    for dependency in dependencies.iter() {
        let via_http = get_download_tunnel(&dependency.url) != "git";
        if via_http {
            match unzip_dependency(&dependency.name, &dependency.version) {
                Ok(_) => {}
                Err(err) => {
                    return Err(err);
                }
            }
        }
    }
    Ok(())
}

pub async fn download_dependency(dependency: &Dependency) -> Result<String, DownloadError> {
    let dependency_directory: PathBuf = DEPENDENCY_DIR.clone();
    if !DEPENDENCY_DIR.is_dir() {
        fs::create_dir(&dependency_directory).unwrap();
    }

    let tunnel = get_download_tunnel(&dependency.url);
    let hash: String;
    if tunnel == "http" {
        match download_via_http(dependency, &dependency_directory).await {
            Ok(_) => {}
            Err(err) => {
                return Err(DownloadError {
                    name: dependency.name.to_string(),
                    version: dependency.url.to_string(),
                    cause: err.cause,
                });
            }
        }
        hash = sha256_digest(&dependency.name, &dependency.version);
    } else if tunnel == "git" {
        hash = match download_via_git(dependency, &dependency_directory).await {
            Ok(h) => h,
            Err(err) => {
                return Err(DownloadError {
                    name: dependency.name.to_string(),
                    version: dependency.url.to_string(),
                    cause: err.cause,
                });
            }
        };
    } else {
        return Err(DownloadError {
            name: dependency.name.to_string(),
            version: dependency.url.to_string(),
            cause: "Download tunnel unknown".to_string(),
        });
    }
    println!(
        "{}",
        Paint::green(&format!(
            "Dependency {}-{} downloaded!",
            dependency.name, dependency.version
        ))
    );

    Ok(hash)
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

async fn download_via_git(
    dependency: &Dependency,
    dependency_directory: &Path,
) -> Result<String, DownloadError> {
    let target_dir = &format!("{}-{}", dependency.name, dependency.version);
    let path = dependency_directory.join(target_dir);
    if path.exists() {
        let _ = remove_dir_all(path);
    }

    let http_url = transform_git_to_http(&dependency.url);
    let repo = match Repository::clone(http_url.as_str(), dependency_directory.join(target_dir)) {
        Ok(r) => {
            println!(
                "{}",
                Paint::green(&format!("Successfully cloned dependency to {:?}", r.path()))
            );
            r
        }
        Err(err) => {
            return Err(DownloadError {
                name: dependency.name.to_string(),
                version: dependency.version.to_string(),
                cause: format!(
                    "Dependency {}~{} could not be downloaded via git.\nCause: {}",
                    dependency.name.clone(),
                    dependency.version.clone(),
                    err.message()
                ),
            });
        }
    };

    if !dependency.hash.is_empty() {
        // Find the commit
        let oid = Oid::from_str(&dependency.hash).unwrap();
        let commit = repo.find_commit(oid).unwrap();

        // Checkout the commit
        let tree: git2::Tree = commit.tree().unwrap();
        repo.checkout_tree(tree.as_object(), None).unwrap();
        repo.set_head_detached(commit.id()).unwrap();
        return Ok(dependency.hash.clone());
    }

    let head = repo.head().unwrap();

    // Resolve the reference to the actual commit object
    let commit = head.peel_to_commit().unwrap();

    // Get the commit hash
    let commit_hash = commit.id().to_string();
    Ok(commit_hash)
}

async fn download_via_http(
    dependency: &Dependency,
    dependency_directory: &Path,
) -> Result<(), DownloadError> {
    let zip_to_download = &format!("{}-{}.zip", dependency.name, dependency.version);
    let mut stream = match reqwest::get(&dependency.url).await {
        Ok(res) => {
            if res.status() != 200 {
                return Err(DownloadError {
                    name: dependency.name.clone().to_string(),
                    version: dependency.url.clone().to_string(),
                    cause: format!(
                        "Dependency {}~{} could not be downloaded via http.\nStatus: {}",
                        dependency.name.clone(),
                        dependency.version.clone(),
                        res.status()
                    ),
                });
            }
            res.bytes_stream()
        }
        Err(err) => {
            return Err(DownloadError {
                name: dependency.name.clone().to_string(),
                version: dependency.url.clone().to_string(),
                cause: format!("Unknown error: {:?}", err.source()),
            });
        }
    };

    let mut file = File::create(&dependency_directory.join(zip_to_download))
        .await
        .unwrap();

    while let Some(chunk_result) = stream.next().await {
        match file.write_all(&chunk_result.unwrap()).await {
            Ok(_) => {}
            Err(err) => {
                return Err(DownloadError {
                    name: dependency.name.to_string(),
                    version: dependency.version.to_string(),
                    cause: format!("Unknown error: {:?}", err.source()),
                });
            }
        }
    }

    match file.flush().await {
        Ok(_) => {}
        Err(err) => {
            return Err(DownloadError {
                name: dependency.name.to_string(),
                version: dependency.url.to_string(),
                cause: format!("Unknown error: {:?}", err.source()),
            });
        }
    };
    Ok(())
}

fn transform_git_to_http(url: &str) -> String {
    if let Some(stripped) = url.strip_prefix("git@github.com:") {
        let repo_path = stripped;
        format!("https://github.com/{}", repo_path)
    } else if let Some(stripped) = url.strip_prefix("git@gitlab.com:") {
        let repo_path = stripped;
        format!("https://gitlab.com/{}", repo_path)
    } else {
        url.to_string()
    }
}

#[cfg(test)]
#[allow(clippy::vec_init_then_push)]
mod tests {
    use super::*;
    use crate::janitor::healthcheck_dependency;
    use serial_test::serial;
    use std::fs::metadata;
    use std::path::Path;

    #[tokio::test]
    #[serial]
    async fn download_dependencies_http_one_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
            hash: String::new()
        };
        dependencies.push(dependency.clone());
        let hashes = download_dependencies(&dependencies, false).await.unwrap();
        let path_zip =
            DEPENDENCY_DIR.join(format!("{}-{}.zip", &dependency.name, &dependency.version));
        assert!(path_zip.exists());
        assert!(hashes.len() == 1);
        assert!(!hashes[0].is_empty());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_git_one_success() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "git@github.com:transmissions11/solmate.git".to_string(),
            hash: String::new(),
        };
        dependencies.push(dependency.clone());
        let hashes = download_dependencies(&dependencies, false).await.unwrap();
        let path_dir = DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name, &dependency.version));
        assert!(path_dir.exists());
        assert!(path_dir.join("src").join("auth").join("Owned.sol").exists());
        assert!(hashes.len() == 1);
        assert!(!hashes[0].is_empty());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_gitlab_giturl_one_success() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "git@gitlab.com:mario4582928/Mario.git".to_string(),
            hash: String::new(),
        };
        dependencies.push(dependency.clone());
        let hashes = download_dependencies(&dependencies, false).await.unwrap();
        let path_dir = DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name, &dependency.version));
        assert!(path_dir.exists());
        assert!(path_dir.join("README.md").exists());
        assert!(hashes.len() == 1);
        assert_eq!(hashes[0], "bedb1775bd60e3e142a6db863982254a4b891b20"); // this is the last commit, hash == commit
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_gitlab_httpurl_one_success() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://gitlab.com/mario4582928/Mario.git".to_string(),
            hash: String::new(),
        };
        dependencies.push(dependency.clone());
        let hashes = download_dependencies(&dependencies, false).await.unwrap();
        let path_dir = DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name, &dependency.version));
        assert!(path_dir.exists());
        assert!(path_dir.join("README.md").exists());
        assert!(hashes.len() == 1);
        assert_eq!(hashes[0], "bedb1775bd60e3e142a6db863982254a4b891b20"); // this is the last commit, hash == commit
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_http_two_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let  dependency_one = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
            hash: String::new()
        };
        dependencies.push(dependency_one.clone());

        let dependency_two = Dependency {
            name: "@uniswap-v2-core".to_string(),
            version: "1.0.0-beta.4".to_string(),
            url: "https://soldeer-revisions.s3.amazonaws.com/@uniswap-v2-core/1_0_0-beta_4_22-01-2024_13:18:27_v2-core.zip".to_string(),
            hash: String::new()
        };

        dependencies.push(dependency_two.clone());
        let hashes = download_dependencies(&dependencies, false).await.unwrap();
        let mut path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_one.name, &dependency_one.version
        ));
        assert!(path_zip.exists());

        path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_two.name, &dependency_two.version
        ));
        assert!(path_zip.exists());
        assert!(hashes.len() == 2);
        assert!(!hashes[0].is_empty());
        assert!(!hashes[1].is_empty());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_git_two_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency_one = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "git@github.com:transmissions11/solmate.git".to_string(),
            hash: String::new(),
        };
        dependencies.push(dependency_one.clone());

        let dependency_two = Dependency {
            name: "@uniswap-v2-core".to_string(),
            version: "1.0.0-beta.4".to_string(),
            url: "https://github.com/OpenZeppelin/openzeppelin-contracts.git".to_string(),
            hash: String::new(),
        };

        dependencies.push(dependency_two.clone());
        let hashes = download_dependencies(&dependencies, false).await.unwrap();
        let mut path_dir = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_one.name, &dependency_one.version
        ));
        let mut path_dir_two = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_two.name, &dependency_two.version
        ));
        assert!(path_dir.exists());
        assert!(path_dir_two.exists());

        path_dir = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_one.name, &dependency_one.version
        ));
        path_dir_two = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_two.name, &dependency_two.version
        ));
        assert!(path_dir.exists());
        assert!(path_dir_two.exists());
        assert!(hashes.len() == 2);
        assert!(!hashes[0].is_empty());
        assert!(!hashes[1].is_empty());
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
            hash: String::new() };
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
                hash: String::new()};

        dependencies = Vec::new();
        dependencies.push(dependency_two.clone());

        let hashes = download_dependencies(&dependencies, false).await.unwrap();
        let size_of_two = fs::metadata(Path::new(&path_zip)).unwrap().len();

        assert!(size_of_two > size_of_one);
        assert!(hashes.len() == 1);
        assert!(!hashes[0].is_empty());
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
            hash: String::new()};

        dependencies.push(dependency_old.clone());
        download_dependencies(&dependencies, false).await.unwrap();

        // making sure the dependency exists so we can check the deletion
        let path_zip_old = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_old.name, &dependency_old.version
        ));
        assert!(path_zip_old.exists());

        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
            hash: String::new()};
        dependencies = Vec::new();
        dependencies.push(dependency.clone());

        let hashes = download_dependencies(&dependencies, true).await.unwrap();
        let path_zip =
            DEPENDENCY_DIR.join(format!("{}-{}.zip", &dependency.name, &dependency.version));
        assert!(!path_zip_old.exists());
        assert!(path_zip.exists());
        assert!(hashes.len() == 1);
        assert!(!hashes[0].is_empty());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_http_one_fail() {
        let mut dependencies: Vec<Dependency> = Vec::new();

        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~.zip".to_string(),
            hash: String::new()};
        dependencies.push(dependency.clone());

        match download_dependencies(&dependencies, false).await {
            Ok(_) => {
                assert_eq!("Invalid state", "");
            }
            Err(err) => {
                assert_eq!(err.cause, "Dependency @openzeppelin-contracts~2.3.0 could not be downloaded via http.\nStatus: 404 Not Found");
            }
        }
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_git_one_fail() {
        let mut dependencies: Vec<Dependency> = Vec::new();

        let dependency = Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "git@github.com:transmissions11/solmate-wrong.git".to_string(),
            hash: String::new(),
        };
        dependencies.push(dependency.clone());

        match download_dependencies(&dependencies, false).await {
            Ok(_) => {
                assert_eq!("Invalid state", "");
            }
            Err(err) => {
                assert_eq!(err.cause, "Dependency @openzeppelin-contracts~2.3.0 could not be downloaded via git.\nCause: remote authentication required but no callback set");
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
            hash: String::new() };
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
            hash: String::new(),
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
    async fn download_unzip_check_integrity() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "3.3.0-custom-test".to_string(),
            url: "https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip".to_string(),
            hash: String::new()
        });
        download_dependencies(&dependencies, false).await.unwrap();
        unzip_dependency(&dependencies[0].name, &dependencies[0].version).unwrap();
        healthcheck_dependency("@openzeppelin-contracts", "3.3.0-custom-test").unwrap();
        assert!(DEPENDENCY_DIR
            .join("@openzeppelin-contracts-3.3.0-custom-test")
            .join("token")
            .join("ERC20")
            .join("ERC20.sol")
            .exists());
        clean_dependency_directory()
    }

    #[test]
    fn get_download_tunnel_http() {
        assert_eq!(
            get_download_tunnel(
                "https://github.com/foundry-rs/forge-std/archive/refs/tags/v1.9.1.zip"
            ),
            "http"
        );
    }

    #[test]
    fn get_download_tunnel_git_giturl() {
        assert_eq!(
            get_download_tunnel("git@github.com:foundry-rs/forge-std.git"),
            "git"
        );
    }

    #[test]
    fn get_download_tunnel_git_githttp() {
        assert_eq!(
            get_download_tunnel("https://github.com/foundry-rs/forge-std.git"),
            "git"
        );
    }

    #[test]
    fn transform_git_giturl_to_http_success() {
        assert_eq!(
            transform_git_to_http("git@github.com:foundry-rs/forge-std.git"),
            "https://github.com/foundry-rs/forge-std.git"
        );
    }

    #[test]
    fn transform_git_httpurl_to_http_success() {
        assert_eq!(
            transform_git_to_http("https://github.com/foundry-rs/forge-std.git"),
            "https://github.com/foundry-rs/forge-std.git"
        );
    }

    #[test]
    fn transform_gitlab_giturl_to_http_success() {
        assert_eq!(
            transform_git_to_http("git@gitlab.com:mario4582928/Mario.git"),
            "https://gitlab.com/mario4582928/Mario.git"
        );
    }

    #[test]
    fn transform_gitlab_httpurl_to_http_success() {
        assert_eq!(
            transform_git_to_http("https://gitlab.com/mario4582928/Mario.git"),
            "https://gitlab.com/mario4582928/Mario.git"
        );
    }
}
