//! Utility functions used throughout the codebase.
use crate::{
    config::Dependency,
    errors::{DownloadError, InstallError},
    registry::parse_version_req,
};
use derive_more::derive::{Display, From};
use ignore::{WalkBuilder, WalkState};
use log::{debug, warn};
use path_slash::PathExt as _;
use rayon::prelude::*;
use semver::Version;
use sha2::{Digest as _, Sha256};
use std::{
    borrow::Cow,
    env,
    ffi::OsStr,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};
use tokio::process::Command;

/// Newtype for the string representation of an integrity checksum (SHA256).
#[derive(Debug, Clone, PartialEq, Eq, Hash, From, Display)]
#[from(Cow<'static, str>, String, &'static str)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntegrityChecksum(pub String);

/// Get the location where the token file is stored or read from.
///
/// The token file is stored in the home directory of the user, or in the current directory
/// if the home cannot be found, in a hidden folder called `.soldeer`. The token file is called
/// `.soldeer_login`.
///
/// The path can be overridden by setting the `SOLDEER_LOGIN_FILE` environment variable.
pub fn login_file_path() -> Result<PathBuf, std::io::Error> {
    if let Ok(file_path) = env::var("SOLDEER_LOGIN_FILE") {
        if !file_path.is_empty() {
            debug!("using soldeer login file defined in environment variable");
            return Ok(file_path.into());
        }
    }

    // if home dir cannot be found, use the current dir
    let dir = home::home_dir().unwrap_or(env::current_dir()?);
    let security_directory = dir.join(".soldeer");
    if !security_directory.exists() {
        debug!(dir:? = dir; ".soldeer folder does not exist, creating it");
        fs::create_dir(&security_directory)?;
    }
    let login_file = security_directory.join(".soldeer_login");
    debug!(login_file:? = login_file; "path to login file");
    Ok(login_file)
}

/// Check if any filename in the list of paths starts with a period.
pub fn check_dotfiles(files: &[PathBuf]) -> bool {
    files
        .par_iter()
        .any(|file| file.file_name().unwrap_or_default().to_string_lossy().starts_with('.'))
}

/// Sanitize a filename by replacing invalid characters with a dash.
pub fn sanitize_filename(dependency_name: &str) -> String {
    let options =
        sanitize_filename::Options { truncate: true, windows: cfg!(windows), replacement: "-" };

    let filename = sanitize_filename::sanitize_with_options(dependency_name, options);
    debug!(filename; "sanitized filename");
    filename
}

/// Hash the contents of a Reader with SHA256
pub fn hash_content<R: Read>(content: &mut R) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut buf = [0; 1024];
    while let Ok(size) = content.read(&mut buf) {
        if size == 0 {
            break;
        }
        hasher.update(&buf[0..size]);
    }
    hasher.finalize().into()
}

/// Walk a folder and compute the SHA256 hash of all non-hidden and non-ignored files inside the
/// dir, combining them into a single hash.
///
/// The paths of the folders and files are hashes too, so we can the integrity of their names and
/// location can be checked.
pub fn hash_folder(folder_path: impl AsRef<Path>) -> Result<IntegrityChecksum, std::io::Error> {
    debug!(path:? = folder_path.as_ref(); "hashing folder");
    // a list of hashes, one for each DirEntry
    let root_path = Arc::new(dunce::canonicalize(folder_path.as_ref())?);

    let (tx, rx) = mpsc::channel::<[u8; 32]>();

    // we use a parallel walker to speed things up
    let walker = WalkBuilder::new(&folder_path)
        .filter_entry(|entry| {
            !(entry.path().is_dir() && entry.path().file_name().unwrap_or_default() == ".git")
        })
        .hidden(false)
        .require_git(false)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .build_parallel();
    walker.run(|| {
        let tx = tx.clone();
        let root_path = Arc::clone(&root_path);
        // function executed for each DirEntry
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            let path = entry.path();
            // first hash the filename/dirname to make sure it can't be renamed or removed
            let mut hasher = Sha256::new();
            hasher.update(
                path.strip_prefix(root_path.as_ref())
                    .expect("path should be a child of root")
                    .to_slash_lossy()
                    .as_bytes(),
            );
            // for files, also hash the contents
            if let Some(true) = entry.file_type().map(|t| t.is_file()) {
                if let Ok(file) = fs::File::open(path) {
                    let mut reader = std::io::BufReader::new(file);
                    let hash = hash_content(&mut reader);
                    hasher.update(hash);
                } else {
                    warn!(path:? = path; "could not read file while hashing folder");
                }
            }
            // record the hash for that file/folder in the list
            let hash: [u8; 32] = hasher.finalize().into();
            tx.send(hash)
                .expect("Channel receiver should never be dropped before end of function scope");
            WalkState::Continue
        })
    });
    drop(tx);
    let mut hasher = Sha256::new();
    // this cannot happen before tx is dropped safely
    let mut hashes = Vec::new();
    while let Ok(msg) = rx.recv() {
        hashes.push(msg);
    }
    // sort hashes
    hashes.par_sort_unstable();
    // hash the hashes (yo dawg...)
    for hash in hashes.iter() {
        hasher.update(hash);
    }
    let hash: [u8; 32] = hasher.finalize().into();
    let hash = const_hex::encode(hash);
    debug!(path:? = folder_path.as_ref(), hash; "folder hash was computed");
    Ok(hash.into())
}

/// Compute the SHA256 hash of the contents of a file
pub fn hash_file(path: impl AsRef<Path>) -> Result<IntegrityChecksum, std::io::Error> {
    debug!(path:? = path.as_ref(); "hashing file");
    let file = fs::File::open(&path)?;
    let mut reader = std::io::BufReader::new(file);
    let bytes = hash_content(&mut reader);
    let hash = const_hex::encode(bytes);
    debug!(path:? = path.as_ref(), hash; "file hash was computed");
    Ok(hash.into())
}

/// Run a `git` command with the given arguments in the given directory.
///
/// The function output is parsed as a UTF-8 string and returned.
pub async fn run_git_command<I, S>(
    args: I,
    current_dir: Option<&PathBuf>,
) -> Result<String, DownloadError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut git = Command::new("git");
    git.args(args).env("GIT_TERMINAL_PROMPT", "0");
    if let Some(current_dir) = current_dir {
        git.current_dir(
            canonicalize(current_dir)
                .await
                .map_err(|e| DownloadError::IOError { path: current_dir.clone(), source: e })?,
        );
    }
    let git = git.output().await.map_err(|e| DownloadError::GitError(e.to_string()))?;
    if !git.status.success() {
        return Err(DownloadError::GitError(String::from_utf8(git.stderr).unwrap_or_default()));
    }
    Ok(String::from_utf8(git.stdout).expect("git command output should be valid utf-8"))
}

/// Run a `forge` command with the given arguments in the given directory.
///
/// The function output is parsed as a UTF-8 string and returned.
pub async fn run_forge_command<I, S>(
    args: I,
    current_dir: Option<&PathBuf>,
) -> Result<String, InstallError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut forge = Command::new("forge");
    forge.args(args);
    if let Some(current_dir) = current_dir {
        forge.current_dir(
            canonicalize(current_dir)
                .await
                .map_err(|e| InstallError::IOError { path: current_dir.clone(), source: e })?,
        );
    }
    let forge = forge.output().await.map_err(|e| InstallError::ForgeError(e.to_string()))?;
    if !forge.status.success() {
        return Err(InstallError::ForgeError(String::from_utf8(forge.stderr).unwrap_or_default()));
    }
    Ok(String::from_utf8(forge.stdout).expect("forge command output should be valid utf-8"))
}

/// Remove/uninstall the `forge-std` library installed as a git submodule in a foundry project.
///
/// This function removes the `forge-std` submodule, the `.gitmodules` file and the `lib` directory
/// from the project.
pub async fn remove_forge_lib(root: impl AsRef<Path>) -> Result<(), InstallError> {
    debug!("removing forge-std installed as a git submodule");
    let gitmodules_path = root.as_ref().join(".gitmodules");
    let lib_dir = root.as_ref().join("lib");
    let forge_std_dir = lib_dir.join("forge-std");
    if forge_std_dir.exists() {
        run_git_command(
            &["rm", &forge_std_dir.to_string_lossy()],
            Some(&root.as_ref().to_path_buf()),
        )
        .await?;
        debug!("removed lib/forge-std");
    }
    if lib_dir.exists() {
        fs::remove_dir_all(&lib_dir)
            .map_err(|e| InstallError::IOError { path: lib_dir.clone(), source: e })?;
        debug!("removed lib dir");
    }
    if gitmodules_path.exists() {
        fs::remove_file(&gitmodules_path)
            .map_err(|e| InstallError::IOError { path: lib_dir, source: e })?;
        debug!("removed .gitmodules file");
    }
    Ok(())
}

/// Canonicalize a path, resolving symlinks and relative paths.
///
/// This function also normalizes paths on Windows to use the MS-DOS format (as opposed to UNC)
/// whenever possible.
pub async fn canonicalize(path: impl AsRef<Path>) -> Result<PathBuf, std::io::Error> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || dunce::canonicalize(&path)).await?
}

/// Check if a path corresponds to the provided dependency.
///
/// The folder does not need to exist. The folder name must start with the dependency name
/// (sanitized). For dependencies with a semver-compliant version requirement, any folder with a
/// version that matches will give a result of `true`. Otherwise, the folder name must contain the
/// version requirement string after the dependency name.
pub fn path_matches(dependency: &Dependency, path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    let Some(dir_name) = path.file_name() else {
        return false;
    };
    let dir_name = dir_name.to_string_lossy();
    let prefix = format!("{}-", sanitize_filename(dependency.name()));
    if !dir_name.starts_with(&prefix) {
        return false;
    }
    match (
        parse_version_req(dependency.version_req()),
        Version::parse(dir_name.strip_prefix(&prefix).expect("prefix should be present")),
    ) {
        (None, _) | (Some(_), Err(_)) => {
            // not semver compliant
            dir_name == format!("{prefix}{}", sanitize_filename(dependency.version_req()))
        }
        (Some(version_req), Ok(version)) => version_req.matches(&version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use testdir::testdir;

    fn create_test_folder(name: Option<&str>) -> PathBuf {
        let dir = testdir!();
        let named_dir = match name {
            None => dir,
            Some(name) => {
                let d = dir.join(name);
                fs::create_dir(&d).unwrap();
                d
            }
        };
        fs::write(named_dir.join("a.txt"), "this is a test file").unwrap();
        fs::write(named_dir.join("b.txt"), "this is a second test file").unwrap();
        fs::write(named_dir.join("ignored.txt"), "this file should be ignored").unwrap();
        fs::write(named_dir.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(
            named_dir.parent().unwrap().join(".gitignore"),
            format!("{}/a.txt", named_dir.file_name().unwrap().to_string_lossy()),
        )
        .unwrap(); // this file should be ignored because it's in the parent dir
        dunce::canonicalize(named_dir).unwrap()
    }

    #[test]
    fn test_hash_content() {
        let mut content = "this is a test file".as_bytes();
        let hash = hash_content(&mut content);
        assert_eq!(
            const_hex::encode(hash),
            "5881707e54b0112f901bc83a1ffbacac8fab74ea46a6f706a3efc5f7d4c1c625".to_string()
        );
    }

    #[test]
    fn test_hash_content_content_sensitive() {
        let mut content = "foobar".as_bytes();
        let hash = hash_content(&mut content);
        let mut content2 = "baz".as_bytes();
        let hash2 = hash_content(&mut content2);
        assert_ne!(hash, hash2);
    }

    #[test]
    fn test_hash_file() {
        let path = testdir!().join("test.txt");
        fs::write(&path, "this is a test file").unwrap();
        let hash = hash_file(&path).unwrap();
        assert_eq!(hash, "5881707e54b0112f901bc83a1ffbacac8fab74ea46a6f706a3efc5f7d4c1c625".into());
    }

    #[test]
    fn test_hash_folder_abs_path_insensitive() {
        let folder1 = create_test_folder(Some("dir1"));
        let folder2 = create_test_folder(Some("dir2"));
        let hash1 = hash_folder(&folder1).unwrap();
        let hash2 = hash_folder(&folder2).unwrap();
        assert_eq!(
            hash1.to_string(),
            "c5328a2c3db7582b9074d5f5263ef111b496bbf9cda9b6c5fb0f97f1dc17b766"
        );
        assert_eq!(hash1, hash2);
        // ignored.txt should be ignored in the checksum calculation, so removing it should yield
        // the same checksum
        fs::remove_file(folder1.join("ignored.txt")).unwrap();
        let hash1 = hash_folder(&folder1).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_folder_rel_path_sensitive() {
        let folder = create_test_folder(Some("dir"));
        let hash1 = hash_folder(&folder).unwrap();
        fs::rename(folder.join("a.txt"), folder.join("c.txt")).unwrap();
        let hash2 = hash_folder(&folder).unwrap();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_folder_content_sensitive() {
        let folder = create_test_folder(Some("dir"));
        let hash1 = hash_folder(&folder).unwrap();
        fs::create_dir(folder.join("test")).unwrap();
        let hash2 = hash_folder(&folder).unwrap();
        assert_ne!(hash1, hash2);
        fs::write(folder.join("test/c.txt"), "this is a third test file").unwrap();
        let hash3 = hash_folder(&folder).unwrap();
        assert_ne!(hash2, hash3);
        assert_ne!(hash1, hash3);
    }
}
