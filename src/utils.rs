use crate::{
    download::IntegrityChecksum,
    errors::{DownloadError, InstallError},
};
use ignore::{WalkBuilder, WalkState};
use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Digest, Sha256};
use simple_home_dir::home_dir;
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self as std_io, Read},
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::{fs as tokio_fs, process::Command};

static GIT_SSH_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:git@github\.com|git@gitlab)").expect("git ssh regex should compile")
});

// get the current working directory
pub fn get_current_working_dir() -> PathBuf {
    env::current_dir().unwrap()
}

/// Read contents of file at path into a string, or panic
///
/// # Panics
/// If the file cannot be read, due to it being non-existent, not a valid UTF-8 string, etc.
pub fn read_file_to_string(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path.as_ref()).unwrap_or_else(|_| {
        panic!("Could not read file `{:?}`", path.as_ref());
    })
}

// read a file contents into a vector of bytes so we can unzip it
pub fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>, std::io::Error> {
    let f = fs::File::open(path)?;
    let mut reader = std_io::BufReader::new(f);
    let mut buffer = Vec::new();

    // Read file into vector.
    reader.read_to_end(&mut buffer)?;

    Ok(buffer)
}

/// Get the location where the token file is stored or read from
///
/// The token file is stored in the home directory of the user, or in the current working directory
/// if the home cannot be found, in a hidden folder called `.soldeer`. The token file is called
/// `.soldeer_login`.
///
/// For reading (e.g. when pushing to the registry), the path can be overridden by
/// setting the `SOLDEER_LOGIN_FILE` environment variable.
/// For login, the custom path will only be used if the file already exists.
pub fn security_file_path() -> Result<PathBuf, std::io::Error> {
    let custom_security_file = if cfg!(test) {
        return Ok(PathBuf::from("./test_save_jwt"));
    } else {
        env::var("SOLDEER_LOGIN_FILE").ok()
    };

    if let Some(file) = custom_security_file {
        if !file.is_empty() && Path::new(&file).exists() {
            return Ok(file.into());
        }
    }

    // if home dir cannot be found, use the current working directory
    let dir = home_dir().unwrap_or_else(get_current_working_dir);
    let security_directory = dir.join(".soldeer");
    if !security_directory.exists() {
        fs::create_dir(&security_directory)?;
    }
    let security_file = security_directory.join(".soldeer_login");
    Ok(security_file)
}

pub fn get_base_url() -> String {
    if cfg!(test) {
        env::var("base_url").unwrap_or("http://0.0.0.0".to_string())
    } else {
        "https://api.soldeer.xyz".to_string()
    }
}

// Function to check for the presence of sensitive files or directories
pub fn check_dotfiles(path: impl AsRef<Path>) -> bool {
    if !path.as_ref().is_dir() {
        return false;
    }
    fs::read_dir(path)
        .unwrap()
        .map_while(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with('.'))
}

// Function to recursively check for sensitive files or directories in a given path
pub fn check_dotfiles_recursive(path: impl AsRef<Path>) -> bool {
    if check_dotfiles(&path) {
        return true;
    }

    if path.as_ref().is_dir() {
        return fs::read_dir(path)
            .unwrap()
            .map_while(Result::ok)
            .any(|entry| check_dotfiles(entry.path()));
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UrlType {
    Git,
    Http,
}

pub fn get_url_type(dependency_url: &str) -> Result<UrlType, DownloadError> {
    if GIT_SSH_REGEX.is_match(dependency_url) {
        return Ok(UrlType::Git);
    } else if let Ok(url) = reqwest::Url::parse(dependency_url) {
        return Ok(match url.domain() {
            Some("github.com") | Some("gitlab.com") => {
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
    let mut hasher = <Sha256 as Digest>::new();
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
///
/// Since the folder contains the zip file still, we need to skip it. TODO: can we remove the zip
/// file right after unzipping so this is not necessary?
pub fn hash_folder(
    folder_path: impl AsRef<Path>,
    ignore_path: Option<PathBuf>,
) -> Result<IntegrityChecksum, std::io::Error> {
    // perf: it's easier to check a boolean than to compare paths, so when we find the zip we skip
    // the check afterwards
    let seen_ignore_path = Arc::new(AtomicBool::new(ignore_path.is_none()));
    // a list of hashes, one for each DirEntry
    let hashes = Arc::new(Mutex::new(Vec::with_capacity(100)));
    // we use a parallel walker to speed things up
    let walker = WalkBuilder::new(folder_path)
        .filter_entry(|entry| {
            !(entry.path().is_dir() && entry.path().file_name().unwrap_or_default() == ".git")
        })
        .hidden(false)
        .build_parallel();
    walker.run(|| {
        let ignore_path = ignore_path.clone();
        let seen_ignore_path = Arc::clone(&seen_ignore_path);
        let hashes = Arc::clone(&hashes);
        // function executed for each DirEntry
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            let path = entry.path();
            // check if that file is `ignore_path`, unless we've seen it already
            if !seen_ignore_path.load(Ordering::SeqCst) {
                let ignore_path = ignore_path
                    .as_ref()
                    .expect("ignore_path should always be Some when seen_ignore_path is false");
                if path == ignore_path {
                    // record that we've seen the zip file
                    seen_ignore_path.swap(true, Ordering::SeqCst);
                    return WalkState::Continue;
                }
            }
            // first hash the filename/dirname to make sure it can't be renamed or removed
            let mut hasher = <Sha256 as Digest>::new();
            hasher.update(path.as_os_str().as_bytes());
            // for files, also hash the contents
            if let Some(true) = entry.file_type().map(|t| t.is_file()) {
                if let Ok(file) = fs::File::open(path) {
                    let mut reader = std_io::BufReader::new(file);
                    let hash = hash_content(&mut reader);
                    hasher.update(hash);
                }
            }
            // record the hash for that file/folder in the list
            let hash: [u8; 32] = hasher.finalize().into();
            let mut hashes_lock = hashes.lock().expect("mutex should not be poisoned");
            hashes_lock.push(hash);
            WalkState::Continue
        })
    });

    // sort hashes
    let mut hasher = <Sha256 as Digest>::new();
    let mut hashes = hashes.lock().expect("mutex should not be poisoned");
    hashes.sort_unstable();
    // hash the hashes (yo dawg...)
    for hash in hashes.iter() {
        hasher.update(hash);
    }
    let hash: [u8; 32] = hasher.finalize().into();
    Ok(const_hex::encode(hash).into())
}

/// Compute the SHA256 hash of the contents of a file
pub fn hash_file(path: impl AsRef<Path>) -> Result<IntegrityChecksum, std::io::Error> {
    let file = fs::File::open(path)?;
    let mut reader = std_io::BufReader::new(file);
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
            tokio_fs::canonicalize(current_dir)
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
            tokio_fs::canonicalize(current_dir)
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

#[cfg(test)]
mod tests {
    use rand::{distributions::Alphanumeric, Rng as _};

    use super::*;
    use std::fs;

    #[test]
    fn filename_sanitization() {
        let filenames = vec![
            "valid|filename.txt",
            "valid:filename.txt",
            "valid\"filename.txt",
            "valid\\filename.txt",
            "valid<filename.txt",
            "valid>filename.txt",
            "valid*filename.txt",
            "valid?filename.txt",
            "valid/filename.txt",
        ];

        for filename in filenames {
            assert_eq!(sanitize_filename(filename), "valid-filename.txt");
        }
        assert_eq!(sanitize_filename("valid~1.0.0"), "valid~1.0.0");
        assert_eq!(sanitize_filename("valid~1*0.0"), "valid~1-0.0");
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
        let file = create_random_file("test", "txt");
        let hash = hash_file(&file).unwrap();
        fs::remove_file(&file).unwrap();
        assert_eq!(hash, "5881707e54b0112f901bc83a1ffbacac8fab74ea46a6f706a3efc5f7d4c1c625".into());
    }

    #[test]
    fn test_hash_folder() {
        let folder = create_test_folder("test", "test_hash_folder");
        let hash = hash_folder(&folder, None).unwrap();
        fs::remove_dir_all(&folder).unwrap();
        assert_eq!(hash, "b0bbe5dbf490a7120cce269564ed7a1f1f016ff50ccbb38eb288849f0ce7ab49".into());
    }

    #[test]
    fn test_hash_folder_path_sensitive() {
        let folder1 = create_test_folder("test", "test_hash_folder_path_sensitive");
        let folder2 = create_test_folder("test", "test_hash_folder_path_sensitive2");
        let hash1 = hash_folder(&folder1, None).unwrap();
        let hash2 = hash_folder(&folder2, None).unwrap();
        fs::remove_dir_all(&folder1).unwrap();
        fs::remove_dir_all(&folder2).unwrap();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_folder_ignore_path() {
        let folder = create_test_folder("test", "test_hash_folder_ignore_path");
        let hash1 = hash_folder(&folder, None).unwrap();
        let hash2 = hash_folder(&folder, Some(folder.join("a.txt"))).unwrap();
        fs::remove_dir_all(&folder).unwrap();
        assert_ne!(hash1, hash2);
    }

    fn create_random_file(target_dir: impl AsRef<Path>, extension: &str) -> PathBuf {
        let s: String =
            rand::thread_rng().sample_iter(&Alphanumeric).take(7).map(char::from).collect();
        let random_file = target_dir.as_ref().join(format!("random{}.{}", s, extension));
        fs::write(&random_file, "this is a test file").expect("could not write to test file");
        random_file
    }

    fn create_test_folder(target_dir: impl AsRef<Path>, dirname: &str) -> PathBuf {
        let test_folder = target_dir.as_ref().join(dirname);
        fs::create_dir(&test_folder).expect("could not create test folder");
        fs::write(test_folder.join("a.txt"), "this is a test file")
            .expect("could not write to test file a");
        fs::write(test_folder.join("b.txt"), "this is a second test file")
            .expect("could not write to test file b");
        test_folder
    }
}
