use crate::{
    config::{Dependency, GitIdentifier},
    errors::DownloadError,
    utils::{run_git_command, sanitize_filename},
};
use reqwest::IntoUrl;
use std::{
    fmt, fs,
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

impl fmt::Display for IntegrityChecksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    identifier: Option<&GitIdentifier>,
    path: impl AsRef<Path>,
) -> Result<String> {
    let path = path.as_ref().to_path_buf();
    run_git_command(
        &["clone", "--tags", "--filter=tree:0", url, path.to_string_lossy().as_ref()],
        None,
    )
    .await?;
    if let Some(identifier) = identifier {
        run_git_command(&["checkout", &identifier.to_string()], Some(&path)).await?;
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
    use crate::{config::HttpDependency, push::zip_file};
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
        let res = clone_repo("https://github.com/beeb/test-repo.git", None, &dir).await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "d5d72fa135d28b2e8307650b3ea79115183f2406");
    }

    #[tokio::test]
    async fn test_clone_repo_rev() {
        let dir = testdir!();
        let res = clone_repo(
            "https://github.com/beeb/test-repo.git",
            Some(&GitIdentifier::from_rev("d230f5c588c0ed00821a4eb3ef38e300e4a519dc")),
            &dir,
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "d230f5c588c0ed00821a4eb3ef38e300e4a519dc");
    }

    #[tokio::test]
    async fn test_clone_repo_branch() {
        let dir = testdir!();
        let res = clone_repo(
            "https://github.com/beeb/test-repo.git",
            Some(&GitIdentifier::from_branch("dev")),
            &dir,
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "8d903e557e8f1b6e62bde768aa456d4ddfca72c4");
    }

    #[tokio::test]
    async fn test_clone_repo_tag() {
        let dir = testdir!();
        let res = clone_repo(
            "https://github.com/beeb/test-repo.git",
            Some(&GitIdentifier::from_tag("v0.1.0")),
            &dir,
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(&res.unwrap(), "78c2f6a1a54db26bab6c3f501854a1564eb3707f");
    }

    #[test]
    fn test_find_install_path_sync() {
        let dependency: Dependency =
            HttpDependency::builder().name("lib1").version_req("1.0.0").build().into();
        let dir = testdir!();
        let path = dir.join("lib1-1.0.0");
        fs::create_dir(&path).unwrap();
        let res = find_install_path_sync(&dependency, &dir);
        assert!(res.is_some());
        assert_eq!(res.unwrap(), path);
    }

    #[tokio::test]
    async fn test_find_install_path() {
        let dependency: Dependency =
            HttpDependency::builder().name("lib1").version_req("1.0.0").build().into();
        let dir = testdir!();
        let path = dir.join("lib1-1.0.0");
        fs::create_dir(&path).unwrap();
        let res = find_install_path(&dependency, &dir).await;
        assert!(res.is_some());
        assert_eq!(res.unwrap(), path);
    }
}
