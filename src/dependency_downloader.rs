use crate::{
    config::{Dependency, GitDependency, HttpDependency},
    errors::DownloadError,
    remote::get_dependency_url_remote,
    utils::{read_file, sha256_digest},
    DEPENDENCY_DIR,
};
use reqwest::IntoUrl;
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str,
};
use tokio::{fs as tokio_fs, io::AsyncWriteExt, task::JoinSet};
use yansi::Paint as _;

pub type Result<T> = std::result::Result<T, DownloadError>;

pub async fn download_dependencies(
    dependencies: &[Dependency],
    clean: bool,
) -> Result<Vec<DownloadResult>> {
    // clean dependencies folder if flag is true
    if clean {
        // creates the directory
        clean_dependency_directory();
    } else {
        // create the dependency directory if it doesn't exist
        let dir = DEPENDENCY_DIR.clone();
        if tokio_fs::metadata(&dir).await.is_err() {
            tokio_fs::create_dir(&dir)
                .await
                .map_err(|e| DownloadError::IOError { path: dir, source: e })?;
        }
    }

    let mut set = JoinSet::new();
    for dep in dependencies {
        set.spawn({
            let dep = dep.clone();
            async move { download_dependency(&dep, true).await }
        });
    }

    let mut results = Vec::<DownloadResult>::new();
    while let Some(res) = set.join_next().await {
        results.push(res??);
    }

    Ok(results)
}

// un-zip-ing dependencies to dependencies folder
pub fn unzip_dependencies(dependencies: &[Dependency]) -> Result<()> {
    dependencies
        .iter()
        .filter_map(|d| match d {
            Dependency::Http(dep) => Some(dep),
            _ => None,
        })
        .try_for_each(unzip_dependency)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub hash: String,
    pub url: String,
}

pub async fn download_dependency(
    dependency: &Dependency,
    skip_folder_check: bool,
) -> Result<DownloadResult> {
    let dependency_directory: PathBuf = DEPENDENCY_DIR.clone();
    // if we called this method from `download_dependencies` we don't need to check if the folder
    // exists, as it was created by the caller
    if !skip_folder_check && tokio_fs::metadata(&dependency_directory).await.is_err() {
        if let Err(e) = tokio_fs::create_dir(&dependency_directory).await {
            // temp fix for race condition until we use tokio fs everywhere
            if tokio_fs::metadata(&dependency_directory).await.is_err() {
                return Err(DownloadError::IOError { path: dependency_directory, source: e });
            }
        }
    }

    let res = match dependency {
        Dependency::Http(dep) => {
            let url = match &dep.url {
                Some(url) => url.clone(),
                None => get_dependency_url_remote(dependency).await?,
            };
            download_via_http(&url, dep, &dependency_directory).await?;
            DownloadResult { hash: sha256_digest(dep), url }
        }
        Dependency::Git(dep) => {
            let hash = download_via_git(dep, &dependency_directory).await?;
            DownloadResult { hash, url: dep.git.clone() }
        }
    };
    println!(
        "{}",
        format!("Dependency {}-{} downloaded!", dependency.name(), dependency.version()).green()
    );

    Ok(res)
}

pub fn unzip_dependency(dependency: &HttpDependency) -> Result<()> {
    let file_name = format!("{}-{}.zip", dependency.name, dependency.version);
    let target_name = format!("{}-{}/", dependency.name, dependency.version);
    let current_dir = DEPENDENCY_DIR.join(file_name);
    let target = DEPENDENCY_DIR.join(target_name);
    let archive = read_file(current_dir).unwrap();

    zip_extract::extract(Cursor::new(archive), &target, true)?;
    println!("{}", format!("The dependency {dependency} was unzipped!").green());
    Ok(())
}

pub fn clean_dependency_directory() {
    if DEPENDENCY_DIR.is_dir() {
        fs::remove_dir_all(DEPENDENCY_DIR.clone()).unwrap();
        fs::create_dir(DEPENDENCY_DIR.clone()).unwrap();
    }
}

async fn download_via_git(
    dependency: &GitDependency,
    dependency_directory: &Path,
) -> Result<String> {
    println!("{}", format!("Started git download of {dependency}").green());
    let target_dir = &format!("{}-{}", dependency.name, dependency.version);
    let path = dependency_directory.join(target_dir);
    let path_str = path.to_string_lossy().to_string();
    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }

    let http_url = transform_git_to_http(&dependency.git);
    let mut git_clone = Command::new("git");
    let mut git_checkout = Command::new("git");
    let mut git_get_commit = Command::new("git");

    let result = git_clone
        .args(["clone", &http_url, &path_str])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let status = result.status().unwrap();
    let out = result.output().unwrap();

    if !status.success() {
        let _ = fs::remove_dir_all(&path);
        return Err(DownloadError::GitError(
            str::from_utf8(&out.stderr).unwrap().trim().to_string(),
        ));
    }

    let rev = match dependency.rev.clone() {
        Some(rev) => {
            let result = git_get_commit
                .args([
                    format!("--work-tree={}", path_str),
                    format!("--git-dir={}", path.join(".git").to_string_lossy()),
                    "checkout".to_string(),
                    rev.to_string(),
                ])
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let out = result.output().unwrap();
            let status = result.status().unwrap();

            if !status.success() {
                let _ = fs::remove_dir_all(&path);
                return Err(DownloadError::GitError(
                    str::from_utf8(&out.stderr).unwrap().trim().to_string(),
                ));
            }
            rev
        }
        None => {
            let result = git_checkout
                .args([
                    format!("--work-tree={}", path_str),
                    format!("--git-dir={}", path.join(".git").to_string_lossy()),
                    "rev-parse".to_string(),
                    "--verify".to_string(),
                    "HEAD".to_string(),
                ])
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let out = result.output().unwrap();
            let status = result.status().unwrap();
            if !status.success() {
                let _ = fs::remove_dir_all(&path);
                return Err(DownloadError::GitError(
                    str::from_utf8(&out.stderr).unwrap().trim().to_string(),
                ));
            }

            let hash = str::from_utf8(&out.stdout).unwrap().trim().to_string();
            // check the commit hash
            if !hash.is_empty() && hash.len() != 40 {
                let _ = fs::remove_dir_all(&path);
                return Err(DownloadError::GitError(format!("invalid revision hash: {hash}")));
            }
            hash
        }
    };
    println!(
        "{}",
        format!("Successfully downloaded {} the dependency via git", dependency,).green()
    );
    Ok(rev)
}

async fn download_via_http(
    url: impl IntoUrl,
    dependency: &HttpDependency,
    dependency_directory: &Path,
) -> Result<()> {
    println!("{}", format!("Started HTTP download of {dependency}").green());
    let zip_to_download = &format!("{}-{}.zip", dependency.name, dependency.version);
    let resp = reqwest::get(url).await?;
    let mut resp = resp.error_for_status()?;

    let file_path = dependency_directory.join(zip_to_download);
    let mut file = tokio_fs::File::create(&file_path)
        .await
        .map_err(|e| DownloadError::IOError { path: file_path.clone(), source: e })?;

    while let Some(mut chunk) = resp.chunk().await? {
        file.write_all_buf(&mut chunk)
            .await
            .map_err(|e| DownloadError::IOError { path: file_path.clone(), source: e })?;
    }
    // make sure we finished writing the file
    file.flush().await.map_err(|e| DownloadError::IOError { path: file_path, source: e })?;
    Ok(())
}

pub fn delete_dependency_files(dependency: &Dependency) -> Result<()> {
    let path = DEPENDENCY_DIR.join(format!("{}-{}", dependency.name(), dependency.version()));
    fs::remove_dir_all(&path).map_err(|e| DownloadError::IOError { path, source: e })?;
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
    use crate::{
        janitor::healthcheck_dependency,
        utils::{get_url_type, UrlType},
    };
    use serial_test::serial;
    use std::{fs::metadata, path::Path};

    #[tokio::test]
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
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_git_one_success() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "git@github.com:transmissions11/solmate.git".to_string(),
            rev: None,
        });
        dependencies.push(dependency.clone());
        let results = download_dependencies(&dependencies, false).await.unwrap();
        let path_dir =
            DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name(), &dependency.version()));
        assert!(path_dir.exists());
        assert!(path_dir.join("src").join("auth").join("Owned.sol").exists());
        assert!(results.len() == 1);
        assert!(!results[0].hash.is_empty());
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_gitlab_giturl_one_success() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "git@gitlab.com:mario4582928/Mario.git".to_string(),
            rev: None,
        });
        dependencies.push(dependency.clone());
        let results = download_dependencies(&dependencies, false).await.unwrap();
        let path_dir =
            DEPENDENCY_DIR.join(format!("{}-{}", &dependency.name(), &dependency.version()));
        assert!(path_dir.exists());
        assert!(path_dir.join("JustATest3.md").exists());
        assert!(results.len() == 1);
        assert_eq!(results[0].hash, "22868f426bd4dd0e682b5ec5f9bd55507664240c"); // this is the last commit, hash == commit
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependency_gitlab_giturl_with_a_specific_revision() {
        clean_dependency_directory();
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "git@gitlab.com:mario4582928/Mario.git".to_string(),
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

        clean_dependency_directory()
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
        clean_dependency_directory()
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
        clean_dependency_directory()
    }

    #[tokio::test]
    #[serial]
    async fn download_dependencies_git_two_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        let dependency_one = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "git@github.com:transmissions11/solmate.git".to_string(),
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
        clean_dependency_directory()
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
        clean_dependency_directory()
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
        clean_dependency_directory()
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
        clean_dependency_directory()
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
        clean_dependency_directory()
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
        clean_dependency_directory()
    }

    #[test]
    fn get_download_tunnel_http() {
        assert_eq!(
            get_url_type("https://github.com/foundry-rs/forge-std/archive/refs/tags/v1.9.1.zip"),
            UrlType::Http
        );
    }

    #[test]
    fn get_download_tunnel_git_giturl() {
        assert_eq!(get_url_type("git@github.com:foundry-rs/forge-std.git"), UrlType::Git);
    }

    #[test]
    fn get_download_tunnel_git_githttp() {
        assert_eq!(get_url_type("https://github.com/foundry-rs/forge-std.git"), UrlType::Git);
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

    #[tokio::test]
    #[serial]
    async fn remove_one_dependency() {
        let mut dependencies: Vec<Dependency> = Vec::new();

        let dependency = Dependency::Git(GitDependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            git: "git@github.com:transmissions11/solmate.git".to_string(),
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
    }
}
