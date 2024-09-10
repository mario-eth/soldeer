use crate::errors::{DownloadError, InstallError};
use derive_more::derive::{Display, From};
use ignore::{WalkBuilder, WalkState};
use path_slash::PathExt as _;
use regex::Regex;
use sha2::{Digest as _, Sha256};
use std::{
    borrow::Cow,
    env,
    ffi::OsStr,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
};
use tokio::process::Command;

static GIT_SSH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:git@github\.com|git@gitlab)").expect("git ssh regex should compile")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UrlType {
    Git,
    Http,
}

/// Newtype for the string representation of an integrity checksum (SHA256)
#[derive(Debug, Clone, PartialEq, Eq, Hash, From, Display)]
#[from(Cow<'static, str>, String, &'static str)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntegrityChecksum(pub String);

/// Read a file contents into a vector of bytes
pub fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>, std::io::Error> {
    let f = fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(f);
    let mut buffer = Vec::new();

    // Read file into vector.
    reader.read_to_end(&mut buffer)?;

    Ok(buffer)
}

/// Get the location where the token file is stored or read from
///
/// The token file is stored in the home directory of the user, or in the current directory
/// if the home cannot be found, in a hidden folder called `.soldeer`. The token file is called
/// `.soldeer_login`.
///
/// The path can be overridden by setting the `SOLDEER_LOGIN_FILE` environment variable.
pub fn login_file_path() -> Result<PathBuf, std::io::Error> {
    if let Ok(file_path) = env::var("SOLDEER_LOGIN_FILE") {
        if !file_path.is_empty() {
            return Ok(file_path.into());
        }
    }

    // if home dir cannot be found, use the current dir
    let dir = home::home_dir().unwrap_or(env::current_dir()?);
    let security_directory = dir.join(".soldeer");
    if !security_directory.exists() {
        fs::create_dir(&security_directory)?;
    }
    let security_file = security_directory.join(".soldeer_login");
    Ok(security_file)
}

/// Check if any file starts with a period
pub fn check_dotfiles(files: &[PathBuf]) -> bool {
    files.iter().any(|file| file.file_name().unwrap_or_default().to_string_lossy().starts_with('.'))
}

pub fn get_url_type(dependency_url: &str) -> Result<UrlType, DownloadError> {
    if GIT_SSH_REGEX.is_match(dependency_url) {
        return Ok(UrlType::Git);
    } else if let Ok(url) = reqwest::Url::parse(dependency_url) {
        return Ok(match url.domain() {
            Some("github.com" | "gitlab.com") => {
                if url.path().ends_with(".git") {
                    UrlType::Git
                } else {
                    UrlType::Http
                }
            }
            _ => UrlType::Http,
        });
    }
    Err(DownloadError::InvalidUrl(dependency_url.to_string()))
}

pub fn sanitize_filename(dependency_name: &str) -> String {
    let options =
        sanitize_filename::Options { truncate: true, windows: cfg!(windows), replacement: "-" };

    sanitize_filename::sanitize_with_options(dependency_name, options)
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

/// Walk a folder and compute the SHA256 hash of all non-hidden and non-gitignored files inside the
/// dir, combining them into a single hash.
///
/// We hash the name of the folders and files too, so we can check the integrity of their names.
pub fn hash_folder(folder_path: impl AsRef<Path>) -> Result<IntegrityChecksum, std::io::Error> {
    // a list of hashes, one for each DirEntry
    let all_hashes = Arc::new(Mutex::new(Vec::with_capacity(100)));
    let root_path = Arc::new(dunce::canonicalize(folder_path.as_ref())?);
    // we use a parallel walker to speed things up
    let walker = WalkBuilder::new(folder_path)
        .filter_entry(|entry| {
            !(entry.path().is_dir() && entry.path().file_name().unwrap_or_default() == ".git")
        })
        .hidden(false)
        .build_parallel();
    walker.run(|| {
        let all_hashes = Arc::clone(&all_hashes);
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
                }
            }
            // record the hash for that file/folder in the list
            let hash: [u8; 32] = hasher.finalize().into();
            let mut hashes_lock = all_hashes.lock().expect("mutex should not be poisoned");
            hashes_lock.push(hash);
            WalkState::Continue
        })
    });

    // sort hashes
    let mut hasher = Sha256::new();
    let mut all_hashes = all_hashes.lock().expect("mutex should not be poisoned");
    all_hashes.sort_unstable();
    // hash the hashes (yo dawg...)
    for hash in all_hashes.iter() {
        hasher.update(hash);
    }
    let hash: [u8; 32] = hasher.finalize().into();
    Ok(const_hex::encode(hash).into())
}

/// Compute the SHA256 hash of the contents of a file
pub fn hash_file(path: impl AsRef<Path>) -> Result<IntegrityChecksum, std::io::Error> {
    let file = fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let bytes = hash_content(&mut reader);
    Ok(const_hex::encode(bytes).into())
}

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
        return Err(DownloadError::GitError(String::from_utf8(git.stderr).unwrap_or_default()))
    }
    Ok(String::from_utf8(git.stdout).expect("git command output should be valid utf-8"))
}

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
        return Err(InstallError::ForgeError(String::from_utf8(forge.stderr).unwrap_or_default()))
    }
    Ok(String::from_utf8(forge.stdout).expect("forge command output should be valid utf-8"))
}

pub async fn remove_forge_lib(root: impl AsRef<Path>) -> Result<(), InstallError> {
    let gitmodules_path = root.as_ref().join(".gitmodules");
    let lib_dir = root.as_ref().join("lib");
    let forge_std_dir = lib_dir.join("forge-std");
    run_git_command(&["rm", &forge_std_dir.to_string_lossy()], None).await?;
    if lib_dir.exists() {
        fs::remove_dir_all(&lib_dir)
            .map_err(|e| InstallError::IOError { path: lib_dir.clone(), source: e })?;
    }
    if gitmodules_path.exists() {
        fs::remove_file(&gitmodules_path)
            .map_err(|e| InstallError::IOError { path: lib_dir, source: e })?;
    }
    Ok(())
}

pub async fn canonicalize(path: impl AsRef<Path>) -> Result<PathBuf, std::io::Error> {
    let path = path.as_ref().to_path_buf();
    tokio::task::spawn_blocking(move || dunce::canonicalize(&path)).await?
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
            "4671014a36f223796de8760df8125ca6e5a749e162dd5690e815132621dd8bfb"
        );
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

    #[test]
    fn test_url_type_http() {
        assert_eq!(
            get_url_type("https://github.com/foundry-rs/forge-std/archive/refs/tags/v1.9.1.zip")
                .unwrap(),
            UrlType::Http
        );
    }

    #[test]
    fn test_get_url_git_ssh() {
        assert_eq!(get_url_type("git@github.com:foundry-rs/forge-std.git").unwrap(), UrlType::Git);
        assert_eq!(get_url_type("git@gitlab.com:foo/bar.git").unwrap(), UrlType::Git);
    }

    #[test]
    fn test_get_url_git_https() {
        assert_eq!(
            get_url_type("https://github.com/foundry-rs/forge-std.git").unwrap(),
            UrlType::Git
        );
        assert_eq!(
            get_url_type("https://user:pass@github.com/foundry-rs/forge-std.git").unwrap(),
            UrlType::Git
        );
        assert_eq!(get_url_type("https://gitlab.com/foo/bar.git").unwrap(), UrlType::Git);
    }
}
