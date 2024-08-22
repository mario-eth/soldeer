use crate::{
    config::Dependency,
    errors::DownloadError,
    utils::{run_git_command, sanitize_filename},
};
use reqwest::IntoUrl;
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    str,
};
use tokio::{fs as tokio_fs, io::AsyncWriteExt};

pub type Result<T> = std::result::Result<T, DownloadError>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrityChecksum(pub String);

impl<T> From<T> for IntegrityChecksum
where
    T: Into<String>,
{
    fn from(value: T) -> Self {
        let v: String = value.into();
        IntegrityChecksum(v)
    }
}

impl core::fmt::Display for IntegrityChecksum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub async fn download_file(url: impl IntoUrl, folder_path: impl AsRef<Path>) -> Result<PathBuf> {
    let resp = reqwest::get(url).await?;
    let mut resp = resp.error_for_status()?;

    let path = folder_path.as_ref().to_path_buf();
    let mut zip_filename = path
        .file_name()
        .expect("folder path should have a folder name")
        .to_string_lossy()
        .to_string();
    zip_filename.push_str(".zip");
    let path = path.parent().expect("dep folder should have a parent").join(zip_filename);
    let mut file = tokio_fs::File::create(&path)
        .await
        .map_err(|e| DownloadError::IOError { path: path.clone(), source: e })?;
    while let Some(mut chunk) = resp.chunk().await? {
        file.write_all_buf(&mut chunk)
            .await
            .map_err(|e| DownloadError::IOError { path: path.clone(), source: e })?;
    }
    file.flush().await.map_err(|e| DownloadError::IOError { path: path.clone(), source: e })?;
    Ok(path)
}

pub async fn unzip_file(path: impl AsRef<Path>, into: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_path_buf();
    let out_dir = into.as_ref();
    let zip_contents = tokio_fs::read(&path)
        .await
        .map_err(|e| DownloadError::IOError { path: path.clone(), source: e })?;

    zip_extract::extract(Cursor::new(zip_contents), out_dir, true)?;

    tokio_fs::remove_file(&path)
        .await
        .map_err(|e| DownloadError::IOError { path: path.clone(), source: e })
}

pub async fn clone_repo(
    url: &str,
    rev: Option<impl AsRef<str>>,
    path: impl AsRef<Path>,
) -> Result<String> {
    let path = path.as_ref().to_path_buf();
    run_git_command(&["clone", "--tags", url, path.to_string_lossy().as_ref()], None).await?;
    if let Some(rev) = rev {
        run_git_command(&["checkout", rev.as_ref()], Some(&path)).await?;
    }
    let commit =
        run_git_command(&["rev-parse", "--verify", "HEAD"], Some(&path)).await?.trim().to_string();
    Ok(commit)
}

pub fn delete_dependency_files_sync(dependency: &Dependency, deps: impl AsRef<Path>) -> Result<()> {
    let Some(path) = find_install_path_sync(dependency, deps) else {
        return Err(DownloadError::DependencyNotFound(dependency.to_string()));
    };
    fs::remove_dir_all(&path).map_err(|e| DownloadError::IOError { path, source: e })?;
    Ok(())
}

pub fn find_install_path_sync(dependency: &Dependency, deps: impl AsRef<Path>) -> Option<PathBuf> {
    let Ok(read_dir) = fs::read_dir(deps.as_ref()) else {
        return None;
    };
    for entry in read_dir {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(dir_name) = path.file_name() else {
            continue;
        };
        if dir_name
            .to_string_lossy()
            .starts_with(&format!("{}-", sanitize_filename(dependency.name())))
        {
            return Some(path);
        }
    }
    None
}

pub async fn delete_dependency_files(
    dependency: &Dependency,
    deps: impl AsRef<Path>,
) -> Result<()> {
    let Some(path) = find_install_path(dependency, deps).await else {
        return Err(DownloadError::DependencyNotFound(dependency.to_string()));
    };
    tokio_fs::remove_dir_all(&path)
        .await
        .map_err(|e| DownloadError::IOError { path, source: e })?;
    Ok(())
}

pub async fn find_install_path(dependency: &Dependency, deps: impl AsRef<Path>) -> Option<PathBuf> {
    let Ok(mut read_dir) = tokio_fs::read_dir(deps.as_ref()).await else {
        return None;
    };
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(dir_name) = path.file_name() else {
            continue;
        };
        if dir_name
            .to_string_lossy()
            .starts_with(&format!("{}-", sanitize_filename(dependency.name())))
        {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::zip_file;
    use std::fs;
    use testdir::testdir;

    #[tokio::test]
    async fn test_download_file() {
        let path = testdir!().join("my-dependency");
        fs::create_dir(&path).unwrap();
        let res = download_file(
            "https://raw.githubusercontent.com/mario-eth/soldeer/main/README.md",
            &path,
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        let zip_path = path.with_file_name("my-dependency.zip");
        assert!(zip_path.exists());
    }

    #[tokio::test]
    async fn test_unzip_file() {
        let dir = testdir!();
        // create dummy zip
        let file_path = dir.join("file.txt");
        fs::write(&file_path, "foobar").unwrap();
        let zip_path = dir.join("my-dependency.zip");
        zip_file(&dir, &[file_path], &zip_path).unwrap();

        let out_dir = dir.join("out");
        let res = unzip_file(&zip_path, &out_dir).await;
        assert!(res.is_ok(), "{res:?}");
        let file_path = out_dir.join("file.txt");
        assert!(file_path.exists());
        assert!(!zip_path.exists());
    }

    #[tokio::test]
    async fn test_clone_repo() {
        let dir = testdir!();
        let res = clone_repo("https://github.com/beeb/test-repo.git", None::<&str>, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "d5d72fa135d28b2e8307650b3ea79115183f2406");
    }

    #[tokio::test]
    async fn test_clone_repo_rev() {
        let dir = testdir!();
        let res = clone_repo(
            "https://github.com/beeb/test-repo.git",
            Some("d230f5c588c0ed00821a4eb3ef38e300e4a519dc"),
            &dir,
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "d230f5c588c0ed00821a4eb3ef38e300e4a519dc");
    }

    #[tokio::test]
    async fn test_clone_repo_branch() {
        let dir = testdir!();
        let res = clone_repo("https://github.com/beeb/test-repo.git", Some("dev"), &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "8d903e557e8f1b6e62bde768aa456d4ddfca72c4");
    }

    #[tokio::test]
    async fn test_clone_repo_tag() {
        let dir = testdir!();
        let res = clone_repo("https://github.com/beeb/test-repo.git", Some("v0.1.0"), &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "78c2f6a1a54db26bab6c3f501854a1564eb3707f");
    }

    /* #[tokio::test]
    #[serial]
    async fn download_dependencies_http_one_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None
        });
        dependencies.push(dependency.clone());
        let results = download_dependencies(&dependencies, false).await.unwrap();
        let path_zip =
            DEPENDENCY_DIR.join(format!("{}-{}.zip", &dependency.name(), &dependency.version()));
        assert!(path_zip.exists());
        assert!(results.len() == 1);
        assert!(!results[0].hash.is_empty());
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependency_gitlab_httpurl_with_a_specific_revision() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "https://gitlab.com/mario4582928/Mario.git".to_string(),
            rev: Some("7a0663eaf7488732f39550be655bad6694974cb3".to_string()),
        });
        dependencies.push(dependency.clone());
        let results = download_dependencies(&dependencies, false).await.unwrap();
        let path_dir =
            DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name(), &dependency.version()));
        assert!(path_dir.exists());
        assert!(path_dir.join("README.md").exists());
        assert!(results.len() == 1);
        assert_eq!(results[0].hash, "7a0663eaf7488732f39550be655bad6694974cb3"); // this is the last commit, hash == commit

        // at this revision, this file should exists
        let test_right_revision = DEPENDENCY_DIR
            .join(format!("{}-{}", &dependency.name(), &dependency.version()))
            .join("JustATest2.md");
        assert!(test_right_revision.exists());

        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_gitlab_httpurl_one_success() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "https://gitlab.com/mario4582928/Mario.git".to_string(),
            rev: None,
        });
        dependencies.push(dependency.clone());
        let results = download_dependencies(&dependencies, false).await.unwrap();
        let path_dir =
            DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name(), &dependency.version()));
        assert!(path_dir.exists());
        assert!(path_dir.join("README.md").exists());
        assert!(results.len() == 1);
        assert_eq!(results[0].hash, "22868f426bd4dd0e682b5ec5f9bd55507664240c"); // this is the last commit, hash == commit
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_http_two_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let  dependency_one = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None
        });
        dependencies.push(dependency_one.clone());

        let dependency_two = Dependency::Http(HttpDependency {
            name: "@uniswap-v2-core".to_string(),
            version: "1.0.0-beta.4".to_string(),
            url: Some("https://soldeer-revisions.s3.amazonaws.com/@uniswap-v2-core/1_0_0-beta_4_22-01-2024_13:18:27_v2-core.zip".to_string()),
            checksum: None
        });

        dependencies.push(dependency_two.clone());
        let results = download_dependencies(&dependencies, false).await.unwrap();
        let mut path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_one.name(),
            &dependency_one.version()
        ));
        assert!(path_zip.exists());

        path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_two.name(),
            &dependency_two.version()
        ));
        assert!(path_zip.exists());
        assert!(results.len() == 2);
        assert!(!results[0].hash.is_empty());
        assert!(!results[1].hash.is_empty());
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_git_http_two_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency_one = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "https://github.com/transmissions11/solmate.git".to_string(),
            rev: None,
        });
        dependencies.push(dependency_one.clone());

        let dependency_two = Dependency::Git(GitDependency {
            name: "@uniswap-v2-core".to_string(),
            version: "1.0.0-beta.4".to_string(),
            git: "https://gitlab.com/mario4582928/Mario.git".to_string(),
            rev: None,
        });

        dependencies.push(dependency_two.clone());
        let results = download_dependencies(&dependencies, false).await.unwrap();
        let mut path_dir = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_one.name(),
            &dependency_one.version()
        ));
        let mut path_dir_two = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_two.name(),
            &dependency_two.version()
        ));
        assert!(path_dir.exists());
        assert!(path_dir_two.exists());

        path_dir = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_one.name(),
            &dependency_one.version()
        ));
        path_dir_two = DEPENDENCY_DIR.join(format!(
            "{}-{}",
            &dependency_two.name(),
            &dependency_two.version()
        ));
        assert!(path_dir.exists());
        assert!(path_dir_two.exists());
        assert!(results.len() == 2);
        assert!(!results[0].hash.is_empty());
        assert!(!results[1].hash.is_empty());
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependency_should_replace_existing_zip() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency_one = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "download-dep-v1".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None
        });
        dependencies.push(dependency_one.clone());

        download_dependencies(&dependencies, false).await.unwrap();
        let path_zip = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_one.name(),
            &dependency_one.version()
        ));
        let size_of_one = fs::metadata(Path::new(&path_zip)).unwrap().len();

        let dependency_two = Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "download-dep-v1".to_string(),
                url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.4.0.zip".to_string()),
                checksum: None
            });

        dependencies = Vec::new();
        dependencies.push(dependency_two.clone());

        let results = download_dependencies(&dependencies, false).await.unwrap();
        let size_of_two = fs::metadata(Path::new(&path_zip)).unwrap().len();

        assert!(size_of_two > size_of_one);
        assert!(results.len() == 1);
        assert!(!results[0].hash.is_empty());
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_one_with_clean_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency_old = Dependency::Http(HttpDependency {
            name: "@uniswap-v2-core".to_string(),
            version: "1.0.0-beta.4".to_string(),
            url: Some("https://soldeer-revisions.s3.amazonaws.com/@uniswap-v2-core/1_0_0-beta_4_22-01-2024_13:18:27_v2-core.zip".to_string()),
            checksum: None
        });

        dependencies.push(dependency_old.clone());
        download_dependencies(&dependencies, false).await.unwrap();

        // making sure the dependency exists so we can check the deletion
        let path_zip_old = DEPENDENCY_DIR.join(format!(
            "{}-{}.zip",
            &dependency_old.name(),
            &dependency_old.version()
        ));
        assert!(path_zip_old.exists());

        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None
        });
        dependencies = Vec::new();
        dependencies.push(dependency.clone());

        let results = download_dependencies(&dependencies, true).await.unwrap();
        let path_zip =
            DEPENDENCY_DIR.join(format!("{}-{}.zip", &dependency.name(), &dependency.version()));
        assert!(!path_zip_old.exists());
        assert!(path_zip.exists());
        assert!(results.len() == 1);
        assert!(!results[0].hash.is_empty());
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_http_one_fail() {
        let mut dependencies: Vec<Dependency> = Vec::new();

        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~.zip".to_string()),
            checksum: None
        });
        dependencies.push(dependency.clone());

        match download_dependencies(&dependencies, false).await {
            Ok(_) => {
                assert_eq!("Invalid state", "");
            }
            Err(err) => {
                assert_eq!(err.to_string(), "error downloading dependency: HTTP status client error (404 Not Found) for url (https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~.zip)");
            }
        }
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_git_one_fail() {
        let mut dependencies: Vec<Dependency> = Vec::new();

        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "git@github.com:transmissions11/solmate-wrong.git".to_string(),
            rev: None,
        });
        dependencies.push(dependency.clone());

        match download_dependencies(&dependencies, false).await {
            Ok(_) => {
                assert_eq!("Invalid state", "");
            }
            Err(err) => {
                // we assert this as the message contains various absolute paths that can not be
                // hardcoded here
                assert!(err.to_string().contains("Cloning into"));
            }
        }
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn unzip_dependency_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some("https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string()),
            checksum: None
        });
        dependencies.push(dependency.clone());
        download_dependencies(&dependencies, false).await.unwrap();
        let path = DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name(), &dependency.version()));
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
        let dependency = Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: Some(
                "https://freetestdata.com/wp-content/uploads/2022/02/Free_Test_Data_117KB_JPG.jpg"
                    .to_string(),
            ),
            checksum: None,
        });
        dependencies.push(dependency.clone());
        download_dependencies(&dependencies, false).await.unwrap();
        match unzip_dependencies(&dependencies) {
            Ok(_) => {
                clean_dependency_directory();
                assert_eq!("Wrong State", "");
            }
            Err(err) => {
                assert!(matches!(err, DownloadError::UnzipError(_)));
            }
        }
        clean_dependency_directory();
    }

    #[tokio::test]
    #[serial]
    async fn download_unzip_check_integrity() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency::Http(HttpDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "3.3.0-custom-test".to_string(),
            url: Some("https://soldeer-revisions.s3.amazonaws.com/@openzeppelin-contracts/3_3_0-rc_2_22-01-2024_13:12:57_contracts.zip".to_string()),
            checksum: None,
        }));
        download_dependencies(&dependencies, false).await.unwrap();
        unzip_dependency(dependencies[0].as_http().unwrap()).unwrap();
        healthcheck_dependency(&dependencies[0]).unwrap();
        assert!(DEPENDENCY_DIR
            .join("@openzeppelin-contracts-3.3.0-custom-test")
            .join("token")
            .join("ERC20")
            .join("ERC20.sol")
            .exists());
        clean_dependency_directory();
    } */

    /*     #[tokio::test]
    #[serial]
    async fn remove_one_dependency() {
        let mut dependencies: Vec<Dependency> = Vec::new();

        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "https://github.com/transmissions11/solmate.git".to_string(),
            rev: None,
        });
        dependencies.push(dependency.clone());

        match download_dependencies(&dependencies, false).await {
            Ok(_) => {}
            Err(_) => {
                assert_eq!("Invalid state", "");
            }
        }
        let _ = delete_dependency_files(&dependency);
        assert!(!DEPENDENCY_DIR
            .join(format!("{}~{}", dependency.name(), dependency.version()))
            .exists());
    } */
}
