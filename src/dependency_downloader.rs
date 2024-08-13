use crate::{
    config::{Dependency, GitDependency, HttpDependency},
    errors::DownloadError,
    remote::get_dependency_url_remote,
    utils::{hash_folder, read_file, sanitize_dependency_name, zipfile_hash},
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

/// Download the dependencies from the list in parallel
///
/// Note: the dependencies list should be sorted by name and version
pub async fn download_dependencies(
    dependencies: &[Dependency],
    clean: bool,
) -> Result<Vec<DownloadResult>> {
    // clean dependencies folder if flag is true
    if clean {
        // creates the directory
        clean_dependency_directory();
    }

    // create the dependency directory if it doesn't exist
    let dir = DEPENDENCY_DIR.clone();
    if tokio_fs::metadata(&dir).await.is_err() {
        tokio_fs::create_dir(&dir)
            .await
            .map_err(|e| DownloadError::IOError { path: dir, source: e })?;
    }

    let mut set = JoinSet::new();
    for dep in dependencies {
        set.spawn({
            let d = dep.clone();
            async move { download_dependency(&d, true).await }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res??);
    }
    // sort to make the order consistent with the input dependencies list (which should be sorted)
    results.sort_unstable_by(|a, b| a.name.cmp(&b.name).then_with(|| a.version.cmp(&b.version)));

    Ok(results)
}

// un-zip-ing dependencies to dependencies folder
pub fn unzip_dependencies(dependencies: &[Dependency]) -> Result<Vec<Option<IntegrityChecksum>>> {
    let res: Vec<_> = dependencies
        .iter()
        .map(|d| match d {
            Dependency::Http(dep) => unzip_dependency(dep).map(Some),
            _ => Ok(None),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(res)
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub name: String,
    pub version: String,
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
            DownloadResult {
                name: dep.name.clone(),
                version: dep.version.clone(),
                hash: zipfile_hash(dep)?.to_string(),
                url,
            }
        }
        Dependency::Git(dep) => {
            let hash = download_via_git(dep, &dependency_directory).await?;
            DownloadResult {
                name: dep.name.clone(),
                version: dep.version.clone(),
                hash,
                url: dep.git.clone(),
            }
        }
    };

    println!("{}", format!("Dependency {dependency} downloaded!").green());

    Ok(res)
}

pub fn unzip_dependency(dependency: &HttpDependency) -> Result<IntegrityChecksum> {
    let file_name =
        sanitize_dependency_name(&format!("{}-{}", dependency.name, dependency.version));
    let target_name = format!("{}/", file_name);
    let zip_path = DEPENDENCY_DIR.join(format!("{file_name}.zip"));
    let target_dir = DEPENDENCY_DIR.join(target_name);
    let zip_contents = read_file(&zip_path).unwrap();

    zip_extract::extract(Cursor::new(zip_contents), &target_dir, true)?;
    println!("{}", format!("The dependency {dependency} was unzipped!").green());

    hash_folder(&target_dir, Some(zip_path))
        .map_err(|e| DownloadError::IOError { path: target_dir, source: e })
}

pub fn clean_dependency_directory() {
    if fs::metadata(DEPENDENCY_DIR.clone()).is_ok() {
        fs::remove_dir_all(DEPENDENCY_DIR.clone()).unwrap();
        fs::create_dir(DEPENDENCY_DIR.clone()).unwrap();
    }
}

async fn download_via_git(
    dependency: &GitDependency,
    dependency_directory: &Path,
) -> Result<String> {
    println!("{}", format!("Started GIT download of {dependency}").green());
    let target_dir =
        sanitize_dependency_name(&format!("{}-{}", dependency.name, dependency.version));
    let path = dependency_directory.join(target_dir);
    let path_str = path.to_string_lossy().to_string();
    if path.exists() {
        let _ = fs::remove_dir_all(&path);
    }

    let mut git_clone = Command::new("git");

    let result = git_clone
        .args(["clone", &dependency.git, &path_str])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let status = result.status().expect("Getting clone status failed");
    let out = result.output().expect("Getting clone output failed");

    if !status.success() {
        let _ = fs::remove_dir_all(&path);
        return Err(DownloadError::GitError(
            str::from_utf8(&out.stderr).unwrap().trim().to_string(),
        ));
    }

    let rev = match dependency.rev.clone() {
        Some(rev) => {
            let mut git_get_commit = Command::new("git");
            let result = git_get_commit
                .args(["checkout".to_string(), rev.to_string()])
                .env("GIT_TERMINAL_PROMPT", "0")
                .current_dir(&path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let out = result.output().expect("Checkout to revision status failed");
            let status = result.status().expect("Checkout to revision getting output failed");

            if !status.success() {
                let _ = fs::remove_dir_all(&path);
                return Err(DownloadError::GitError(
                    str::from_utf8(&out.stderr).unwrap().trim().to_string(),
                ));
            }
            rev
        }
        None => {
            let mut git_checkout = Command::new("git");

            let result = git_checkout
                .args(["rev-parse".to_string(), "--verify".to_string(), "HEAD".to_string()])
                .env("GIT_TERMINAL_PROMPT", "0")
                .current_dir(&path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            let out = result.output().expect("Getting revision status failed");
            let status = result.status().expect("Getting revision output failed");
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
    let zip_to_download =
        sanitize_dependency_name(&format!("{}-{}.zip", dependency.name, dependency.version));

    let resp = reqwest::get(url).await?;
    let mut resp = resp.error_for_status()?;

    let file_path = dependency_directory.join(&zip_to_download);
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
    let path = DEPENDENCY_DIR.join(sanitize_dependency_name(&format!(
        "{}-{}",
        dependency.name(),
        dependency.version()
    )));
    fs::remove_dir_all(&path).map_err(|e| DownloadError::IOError { path, source: e })?;
    Ok(())
}

pub fn install_subdependencies(dependency: &Dependency) -> Result<()> {
    let dep_name =
        sanitize_dependency_name(&format!("{}-{}", dependency.name(), dependency.version()));

    let dep_dir = DEPENDENCY_DIR.join(dep_name);
    if !dep_dir.exists() {
        return Err(DownloadError::SubdependencyError(
            "Dependency directory does not exists".to_string(),
        ));
    }

    let mut git = Command::new("git");

    let result = git
        .args(["submodule", "update", "--init", "--recursive"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(&dep_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let status = result.status().expect("Subdependency via GIT failed");

    if !status.success() {
        println!("{}", "Dependency has no submodule dependency.".yellow());
    }

    let mut soldeer = Command::new("forge");

    let result = soldeer
        .args(["soldeer", "install"])
        .current_dir(&dep_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let status = result.status().expect("Subdependency via Soldeer failed");

    if !status.success() {
        println!("{}", "Dependency has no Soldeer dependency.".yellow());
    }

    Ok(())
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

    #[tokio::test]
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
    }
}
