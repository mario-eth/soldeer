//! Download and/or extract dependencies
use crate::{
    config::{Dependency, GitIdentifier},
    errors::DownloadError,
    utils::{path_matches, run_git_command, sanitize_filename},
};
use log::{debug, trace, warn};
use reqwest::{IntoUrl, Url};
use std::{
    fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
    str,
};
use tokio::io::AsyncWriteExt as _;
use zip::{ZipArchive, read::root_dir_common_filter};

pub type Result<T> = std::result::Result<T, DownloadError>;

/// Download a zip file into the provided folder.
///
/// Depending on the platform, the folder path must exist prior to calling this function.
/// The filename for the zip file will be the provided base name with the ".zip" extension
pub async fn download_file(
    url: impl IntoUrl,
    folder_path: impl AsRef<Path>,
    base_name: &str,
) -> Result<PathBuf> {
    let url: Url = url.into_url()?;
    debug!(name = base_name, url:% = url; "downloading file");
    let resp = reqwest::get(url).await?;
    let mut resp = resp.error_for_status()?;

    let zip_path = folder_path.as_ref().join(sanitize_filename(&format!("{base_name}.zip")));
    let mut file = tokio::fs::File::create(&zip_path)
        .await
        .map_err(|e| DownloadError::IOError { path: zip_path.clone(), source: e })?;
    while let Some(mut chunk) = resp.chunk().await? {
        file.write_all_buf(&mut chunk)
            .await
            .map_err(|e| DownloadError::IOError { path: zip_path.clone(), source: e })?;
    }
    file.flush().await.map_err(|e| DownloadError::IOError { path: zip_path.clone(), source: e })?;
    debug!(path:? = zip_path; "saved downloaded file");
    Ok(zip_path)
}

/// Unzip a file into a directory and then delete it.
///
/// If `strip_root` is `true` and all archive entries are contained in a single top-level
/// directory, that directory is stripped during extraction. This is desirable for source archives
/// which wrap the contents in a root folder (e.g. GitHub-generated zips for custom URL
/// dependencies), but must not be done for registry packages, where any top-level directory is
/// part of the published package's layout.
///
/// Git repository metadata contained in the archive is never extracted, see
/// [`extract_dependency_archive`].
pub async fn unzip_file(
    path: impl AsRef<Path>,
    into: impl AsRef<Path>,
    strip_root: bool,
) -> Result<()> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking({
        let path = path.clone();
        let out_dir = into.as_ref().to_path_buf();
        move || {
            let file = fs::File::open(&path)
                .map_err(|e| DownloadError::IOError { path: path.clone(), source: e })?;
            extract_dependency_archive(file, &out_dir, strip_root)
        }
    })
    .await??;
    debug!(file:? = path, dest:? = into.as_ref(); "unzipped file");

    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| DownloadError::IOError { path: path.clone(), source: e })?;
    debug!(path:?; "removed zip file");
    Ok(())
}

/// Clone a git repo into the given path, optionally checking out a reference.
///
/// The repository is cloned without trees, which can speed up cloning when the full history is not
/// needed. Contrary to a shallow clone, it's possible to checkout any ref and the missing trees
/// will be retrieved as they are needed.
///
/// This function returns the commit hash corresponding to  the checked out reference (branch, tag,
/// commit).
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
    debug!(repo:? = path; "git repo cloned");
    if let Some(identifier) = identifier {
        match identifier {
            GitIdentifier::Tag(tag) => {
                run_git_command(&["checkout", &format!("refs/tags/{tag}")], Some(&path)).await?;
            }
            GitIdentifier::Branch(branch) => {
                // a tag with the same name would shadow the branch during a plain checkout, so
                // the local branch is created explicitly from the remote-tracking ref
                run_git_command(
                    &[
                        "checkout",
                        "--track",
                        "-B",
                        branch,
                        &format!("refs/remotes/origin/{branch}"),
                    ],
                    Some(&path),
                )
                .await?;
            }
            GitIdentifier::Rev(rev) => {
                run_git_command(&["checkout", rev], Some(&path)).await?;
            }
        }
        debug!(ref:? = identifier, repo:? = path; "checked out ref");
    }
    let commit =
        run_git_command(&["rev-parse", "--verify", "HEAD"], Some(&path)).await?.trim().to_string();
    debug!(repo:? = path; "checked out commit is {commit}");
    Ok(commit)
}

/// Remove the files for a dependency (synchronous).
///
/// This function should only be called in sync contexts. For a version that is safe to run in
/// multithreaded async contexts, see [`delete_dependency_files`].
pub fn delete_dependency_files_sync(dependency: &Dependency, deps: impl AsRef<Path>) -> Result<()> {
    let Some(path) = find_install_path_sync(dependency, deps) else {
        return Err(DownloadError::DependencyNotFound(dependency.to_string()));
    };
    fs::remove_dir_all(&path).map_err(|e| DownloadError::IOError { path, source: e })?;
    debug!(dep:% = dependency; "removed all files for dependency (sync)");
    Ok(())
}

/// Find the install path of a dependency by reading the dependencies directory and matching on the
/// folder name.
///
/// If a dependency version requirement string is a semver requirement, any folder which version
/// matches the requirements is returned.
pub fn find_install_path_sync(dependency: &Dependency, deps: impl AsRef<Path>) -> Option<PathBuf> {
    let res = fs::read_dir(deps.as_ref())
        .map(|read_dir| {
            read_dir.into_iter().find_map(|e| {
                e.ok().filter(|e| install_path_matches(dependency, e.path())).map(|e| e.path())
            })
        })
        .ok()
        .flatten()
        .inspect(|res| debug!(path:? = res, dep:% = dependency; "folder name matches dependency"));
    if res.is_none() {
        debug!(dep:% = dependency; "could not find install path of dependency");
    }
    res
}

/// Find the install path of a dependency by reading the dependencies directory and matching on the
/// folder name (async version).
///
/// If a dependency version requirement string is a semver requirement, any folder which version
/// matches the requirements is returned.
pub async fn find_install_path(dependency: &Dependency, deps: impl AsRef<Path>) -> Option<PathBuf> {
    let Ok(mut read_dir) = tokio::fs::read_dir(deps.as_ref()).await else {
        warn!(path:? = deps.as_ref(); "could not list files in deps folder");
        return None;
    };

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        trace!(path:?; "found folder in deps");
        if install_path_matches(dependency, &path) {
            debug!(path:?, dep:% = dependency; "folder name matches dependency");
            return Some(path);
        }
    }
    debug!(dep:% = dependency; "could not find install path of dependency");
    None
}

/// Remove the files for a dependency from the dependencies folder.
///
/// A folder must exist for the dependency.
pub async fn delete_dependency_files(
    dependency: &Dependency,
    deps: impl AsRef<Path>,
) -> Result<()> {
    let Some(path) = find_install_path(dependency, deps).await else {
        return Err(DownloadError::DependencyNotFound(dependency.to_string()));
    };
    tokio::fs::remove_dir_all(&path)
        .await
        .map_err(|e| DownloadError::IOError { path, source: e })?;
    debug!(dep:% = dependency; "removed all files for dependency (async)");
    Ok(())
}

/// Check if a path corresponds to the provided dependency.
///
/// The path must exist and be a folder, and the folder name must start with the dependency name
/// (sanitized). For dependencies with a semver-compliant version requirement, any folder with a
/// version that matches will give a result of `true`. Otherwise, the folder name must contain the
/// version requirement string after the dependency name.
fn install_path_matches(dependency: &Dependency, path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    if !path.is_dir() {
        trace!(path:?; "path is not a directory");
        return false;
    }
    path_matches(dependency, path)
}

/// Extract a downloaded dependency archive into `dir`, which is created if it does not exist.
///
/// Git repository metadata is skipped, we never need the publisher's metadata and it's dangerous
/// since we then run git commands in that folder. Other dotfiles such as `.gitmodules`,
/// `.gitignore` and `.gitattributes` are part of the package and are extracted as usual.
///
/// Symlink entries are written as regular files holding their target path. Entries whose path would
/// land outside of `dir` are rejected, and on unix the file permissions recorded in the archive are
/// preserved.
///
/// If `strip_root` is `true` and the archive wraps everything in a single top-level directory,
/// that directory is stripped. This is desirable for source archives which wrap their contents in
/// a root folder, but must not be done for registry packages.
fn extract_dependency_archive(
    source: impl Read + Seek,
    dir: &Path,
    strip_root: bool,
) -> Result<()> {
    let mut archive = ZipArchive::new(source)?;
    let root = if strip_root { archive.root_dir(root_dir_common_filter)? } else { None };
    let mut skipped = 0usize;
    fs::create_dir_all(dir)
        .map_err(|e| DownloadError::IOError { path: dir.to_path_buf(), source: e })?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(name) = entry.enclosed_name() else {
            // entry cannot be placed inside the destination safely
            return Err(DownloadError::InvalidArchivePath(entry.name().to_string()));
        };
        // entries filtered out of the root detection may not live under the root
        let name = root.as_ref().map_or(name.as_path(), |r| name.strip_prefix(r).unwrap_or(&name));
        if name.as_os_str().is_empty() {
            continue; // the stripped top-level directory itself
        }
        if name.components().any(|c| c.as_os_str().eq_ignore_ascii_case(".git")) {
            trace!(entry = entry.name(); "skipping git metadata found in archive");
            skipped += 1;
            continue;
        }
        let out_path = dir.join(name);
        trace!(entry = entry.name(), path:? = out_path; "extracting archive entry");
        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|e| DownloadError::IOError { path: out_path.clone(), source: e })?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| DownloadError::IOError { path: parent.to_path_buf(), source: e })?;
        }
        let mut out_file = fs::File::create(&out_path)
            .map_err(|e| DownloadError::IOError { path: out_path.clone(), source: e })?;
        std::io::copy(&mut entry, &mut out_file)
            .map_err(|e| DownloadError::IOError { path: out_path.clone(), source: e })?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode().filter(|_| !entry.is_symlink()) {
            // a symlink's mode is `0o120777`, which `chmod` would turn into a world-writable file,
            // and a directory could be made read-only before we extract its children
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))
                .map_err(|e| DownloadError::IOError { path: out_path.clone(), source: e })?;
        }
    }
    if skipped > 0 {
        warn!(skipped, dir:?; "skipped git metadata entries found in archive");
    }
    debug!(dir:?; "extracted archive");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::HttpDependency, push::zip_file};
    use std::{fs, io::Write as _};
    use testdir::testdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[tokio::test]
    async fn test_download_file() {
        let path = testdir!().join("my-dependency");
        fs::create_dir(&path).unwrap();
        let res = download_file(
            "https://raw.githubusercontent.com/mario-eth/soldeer/main/README.md",
            &path,
            "my-dependency",
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        let zip_path = path.join("my-dependency.zip");
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
        let res = unzip_file(&zip_path, &out_dir, true).await;
        assert!(res.is_ok(), "{res:?}");
        let file_path = out_dir.join("file.txt");
        assert!(file_path.exists());
        assert!(!zip_path.exists());
    }

    #[tokio::test]
    async fn test_unzip_file_strips_git_metadata() {
        let dir = testdir!();
        let zip_path = dir.join("metadata.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default();
        zip.start_file(".git/config", opts).unwrap();
        zip.write_all(b"[submodule \"lib/dep\"]\n\tupdate = !touch pwned\n").unwrap();
        zip.start_file("lib/dep/.git", opts).unwrap();
        zip.write_all(b"gitdir: ../../.git/modules/dep\n").unwrap();
        zip.start_file(".gitmodules", opts).unwrap();
        zip.write_all(b"[submodule \"lib/dep\"]\n\tpath = lib/dep\n").unwrap();
        zip.start_file("src/Contract.sol", opts).unwrap();
        zip.write_all(b"contract Contract {}\n").unwrap();
        zip.finish().unwrap();

        let out_dir = dir.join("out");
        unzip_file(&zip_path, &out_dir, false).await.unwrap();
        // the archive-controlled git metadata never lands on disk
        assert!(!out_dir.join(".git").exists());
        assert!(!out_dir.join("lib/dep/.git").exists());
        // everything else is extracted as usual
        assert!(out_dir.join(".gitmodules").exists());
        assert!(out_dir.join("src/Contract.sol").exists());
    }

    #[tokio::test]
    async fn test_unzip_file_rejects_path_escape() {
        let dir = testdir!();
        let zip_path = dir.join("escape.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("../escaped.txt", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"escaped\n").unwrap();
        zip.finish().unwrap();

        let out_dir = dir.join("out");
        let res = unzip_file(&zip_path, &out_dir, false).await;
        assert!(
            matches!(&res, Err(DownloadError::InvalidArchivePath(entry)) if entry == "../escaped.txt"),
            "{res:?}"
        );
        assert!(!dir.join("escaped.txt").exists());
    }

    #[tokio::test]
    async fn test_unzip_file_accepts_absolute_entry_names() {
        // some archives in the wild record entries with a leading slash, they are extracted
        // relative to the destination like any other entry
        let dir = testdir!();
        let zip_path = dir.join("absolute.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("/LICENSE-APACHE", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"Apache-2.0\n").unwrap();
        zip.start_file("src/Contract.sol", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"contract Contract {}\n").unwrap();
        zip.finish().unwrap();

        let out_dir = dir.join("out");
        unzip_file(&zip_path, &out_dir, false).await.unwrap();
        assert_eq!(fs::read_to_string(out_dir.join("LICENSE-APACHE")).unwrap(), "Apache-2.0\n");
        assert!(out_dir.join("src/Contract.sol").exists());
    }

    #[tokio::test]
    async fn test_unzip_file_does_not_create_symlinks() {
        let dir = testdir!();
        let zip_path = dir.join("symlink.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.add_symlink("link", "../../secret", SimpleFileOptions::default()).unwrap();
        zip.start_file("file.txt", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"ok\n").unwrap();
        zip.finish().unwrap();

        let out_dir = dir.join("out");
        unzip_file(&zip_path, &out_dir, false).await.unwrap();
        // symlink entries are written as regular files, they never become links out of the folder
        let link = out_dir.join("link");
        assert!(!fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&link).unwrap(), "../../secret");
        // the entry records the symlink mode `0o120777`, which must not land as a world-writable
        // file once the format bits are dropped
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            assert_ne!(fs::metadata(&link).unwrap().permissions().mode() & 0o777, 0o777);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unzip_file_read_only_dir_entry() {
        // a read-only directory recorded in the archive must not stop its children being written
        let dir = testdir!();
        let zip_path = dir.join("readonly.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.add_directory("locked", SimpleFileOptions::default().unix_permissions(0o555)).unwrap();
        zip.start_file("locked/file.txt", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"readable\n").unwrap();
        zip.finish().unwrap();

        let out_dir = dir.join("out");
        unzip_file(&zip_path, &out_dir, false).await.unwrap();
        assert_eq!(fs::read_to_string(out_dir.join("locked/file.txt")).unwrap(), "readable\n");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unzip_file_preserves_unix_mode() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = testdir!();
        let zip_path = dir.join("modes.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        zip.start_file("run.sh", SimpleFileOptions::default().unix_permissions(0o755)).unwrap();
        zip.write_all(b"#!/bin/sh\n").unwrap();
        zip.start_file("plain.txt", SimpleFileOptions::default().unix_permissions(0o644)).unwrap();
        zip.write_all(b"plain\n").unwrap();
        zip.finish().unwrap();

        let out_dir = dir.join("out");
        unzip_file(&zip_path, &out_dir, false).await.unwrap();
        let mode =
            |name: &str| fs::metadata(out_dir.join(name)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode("run.sh"), 0o755);
        assert_eq!(mode("plain.txt"), 0o644);
    }

    #[tokio::test]
    async fn test_unzip_file_strip_root() {
        // archive wrapping all contents in a single root directory, like GitHub source archives
        let dir = testdir!();
        let root = dir.join("my-repo");
        fs::create_dir_all(root.join("src")).unwrap();
        let file_path = root.join("src/Contract.sol");
        fs::write(&file_path, "contract Contract {}\n").unwrap();
        let zip_path = dir.join("archive.zip");
        zip_file(&dir, &[file_path], &zip_path).unwrap();

        // with strip_root, the wrapping directory is removed
        let out_dir = dir.join("stripped");
        unzip_file(&zip_path, &out_dir, true).await.unwrap();
        assert!(out_dir.join("src/Contract.sol").exists());
        assert!(!out_dir.join("my-repo").exists());

        // without strip_root, the layout is preserved as-is
        let zip_path = dir.join("archive2.zip");
        zip_file(&dir, &[dir.join("my-repo/src/Contract.sol")], &zip_path).unwrap();
        let out_dir = dir.join("preserved");
        unzip_file(&zip_path, &out_dir, false).await.unwrap();
        assert!(out_dir.join("my-repo/src/Contract.sol").exists());
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

    /// Run a git command in `dir` and return its trimmed stdout.
    ///
    /// The global and system configuration files are ignored to make sure the
    /// local git config doesn't interfere.
    fn git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", dir.join("nonexistent-global-config"))
            .env("GIT_CONFIG_SYSTEM", dir.join("nonexistent-system-config"))
            .output()
            .unwrap();
        assert!(output.status.success(), "git failed: {args:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    /// Create a repository with a deterministic identity.
    fn init_repo(dir: &Path) {
        fs::create_dir(dir).unwrap();
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test"]);
    }

    #[tokio::test]
    async fn test_clone_repo_tag_prefers_tag_over_branch() {
        let dir = testdir!();
        let source = dir.join("source");
        init_repo(&source);
        fs::write(source.join("version"), "tag").unwrap();
        git(&source, &["add", "version"]);
        git(&source, &["commit", "-m", "tag"]);
        let tag_commit = git(&source, &["rev-parse", "HEAD"]);
        git(&source, &["tag", "main"]);
        fs::write(source.join("version"), "branch").unwrap();
        git(&source, &["commit", "-am", "branch"]);

        let clone_path = dir.join("clone");
        let res = clone_repo(
            source.to_string_lossy().as_ref(),
            Some(&GitIdentifier::from_tag("main")),
            &clone_path,
        )
        .await
        .unwrap();
        assert_eq!(res, tag_commit);
        assert_eq!(fs::read_to_string(clone_path.join("version")).unwrap(), "tag");
    }

    #[tokio::test]
    async fn test_clone_repo_branch_prefers_branch_over_tag() {
        let dir = testdir!();
        let source = dir.join("source");
        init_repo(&source);
        fs::write(source.join("version"), "tag").unwrap();
        git(&source, &["add", "version"]);
        git(&source, &["commit", "-m", "tag"]);
        // tag pointing at the first commit, shadowing the branch name
        git(&source, &["tag", "release"]);
        git(&source, &["checkout", "-b", "release"]);
        fs::write(source.join("version"), "branch").unwrap();
        git(&source, &["commit", "-am", "branch"]);
        let branch_commit = git(&source, &["rev-parse", "refs/heads/release"]);
        git(&source, &["checkout", "main"]);

        let clone_path = dir.join("clone");
        let res = clone_repo(
            source.to_string_lossy().as_ref(),
            Some(&GitIdentifier::from_branch("release")),
            &clone_path,
        )
        .await
        .unwrap();
        assert_eq!(res, branch_commit);
        assert_eq!(fs::read_to_string(clone_path.join("version")).unwrap(), "branch");
    }

    #[test]
    fn test_install_path_matches() {
        let dependency: Dependency =
            HttpDependency::builder().name("lib1").version_req("^1.0.0").build().into();
        let dir = testdir!();
        let path = dir.join("lib1-1.1.1");
        fs::create_dir(&path).unwrap();
        assert!(install_path_matches(&dependency, &path));

        let path = dir.join("lib1-2.0.0");
        fs::create_dir(&path).unwrap();
        assert!(!install_path_matches(&dependency, &path));

        let path = dir.join("lib2-1.0.0");
        fs::create_dir(&path).unwrap();
        assert!(!install_path_matches(&dependency, &path));
    }

    #[test]
    fn test_install_path_matches_nosemver() {
        let dependency: Dependency =
            HttpDependency::builder().name("lib1").version_req("foobar").build().into();
        let dir = testdir!();
        let path = dir.join("lib1-foobar");
        fs::create_dir(&path).unwrap();
        assert!(install_path_matches(&dependency, &path));

        let path = dir.join("lib1-somethingelse");
        fs::create_dir(&path).unwrap();
        assert!(!install_path_matches(&dependency, &path));
    }

    #[test]
    fn test_find_install_path_sync() {
        let dependency: Dependency =
            HttpDependency::builder().name("lib1").version_req("^1.0.0").build().into();
        let dir = testdir!();
        let path = dir.join("lib1-1.1.1");
        fs::create_dir(&path).unwrap();
        let res = find_install_path_sync(&dependency, &dir);
        assert!(res.is_some());
        assert_eq!(res.unwrap(), path);
    }

    #[tokio::test]
    async fn test_find_install_path() {
        let dependency: Dependency =
            HttpDependency::builder().name("lib1").version_req("^1.0.0").build().into();
        let dir = testdir!();
        let path = dir.join("lib1-1.2.5");
        fs::create_dir(&path).unwrap();
        let res = find_install_path(&dependency, &dir).await;
        assert!(res.is_some());
        assert_eq!(res.unwrap(), path);
    }
}
