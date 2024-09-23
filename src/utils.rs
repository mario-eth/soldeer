use crate::{
    config::HttpDependency, dependency_downloader::IntegrityChecksum, errors::DownloadError,
};
use ignore::{WalkBuilder, WalkState};
use path_slash::PathExt;
use rayon::slice::ParallelSliceMut;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock,
    },
};
use yansi::Paint as _;

static GIT_SSH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:git@github\.com|git@gitlab)").expect("git ssh regex should compile")
});
static GIT_HTTPS_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:https://github\.com|https://gitlab\.com).*\.git$")
        .expect("git https regex should compile")
});

// get the current working directory
pub fn get_current_working_dir() -> PathBuf {
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
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
    fs::read(path)
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
pub fn define_security_file_location() -> Result<PathBuf, std::io::Error> {
    if cfg!(test) {
        return Ok(PathBuf::from("./test_save_jwt"));
    }

    if let Some(path) = env::var_os("SOLDEER_LOGIN_FILE") {
        if !path.is_empty() && Path::new(&path).exists() {
            return Ok(path.into());
        }
    }

    // if home dir cannot be found, use the current working directory
    let dir = home::home_dir().unwrap_or_else(get_current_working_dir);
    let security_directory = dir.join(".soldeer");
    if !security_directory.exists() {
        fs::create_dir(&security_directory)?;
    }
    let security_file = security_directory.join(".soldeer_login");
    Ok(security_file)
}

pub fn get_base_url() -> String {
    if cfg!(test) {
        env::var("base_url").unwrap_or_else(|_| "http://0.0.0.0".to_string())
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

// Function to prompt the user for confirmation
pub fn prompt_user_for_confirmation() -> bool {
    println!(
        "{}",
        "You are about to include some sensitive files in this version. Are you sure you want to continue?".yellow()
    );
    println!(
        "{}",
        "If you are not sure what sensitive files, you can run the dry-run command to check what will be pushed.".cyan()
    );

    print!("{}", "Do you want to continue? (y/n): ".green());
    std::io::stdout().flush().unwrap();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UrlType {
    Git,
    Http,
}

pub fn get_url_type(dependency_url: &str) -> UrlType {
    if GIT_SSH_REGEX.is_match(dependency_url) || GIT_HTTPS_REGEX.is_match(dependency_url) {
        return UrlType::Git;
    }
    UrlType::Http
}

pub fn sanitize_dependency_name(dependency_name: &str) -> String {
    let options =
        sanitize_filename::Options { truncate: true, windows: cfg!(windows), replacement: "-" };

    sanitize_filename::sanitize_with_options(dependency_name, options)
}

pub fn zipfile_hash(dependency: &HttpDependency) -> Result<IntegrityChecksum, DownloadError> {
    use crate::DEPENDENCY_DIR;

    let file_name =
        sanitize_dependency_name(&format!("{}-{}.zip", dependency.name, dependency.version));
    let path = DEPENDENCY_DIR.join(&file_name);
    hash_file(&path).map_err(|e| DownloadError::IOError { path, source: e })
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
    // we use a parallel walker to speed things up
    let walker = WalkBuilder::new(&folder_path).hidden(false).build_parallel();
    let root_path = Arc::new(dunce::canonicalize(folder_path.as_ref())?);
    // if memory usage gets a high, this is a possible culprit
    // shouldn't be much of an issue though based on the size of the packages.
    // Can be replaced with a bounded version of mpsc.
    let (tx, rx) = std::sync::mpsc::channel::<[u8; 32]>();
    let tx = Arc::new(tx);

    let hashes = std::thread::spawn(move || {
        let mut hashes = Vec::new();
        while let Ok(msg) = rx.recv() {
            hashes.push(msg);
        }
        hashes
    });

    walker.run(|| {
        let root_path = Arc::clone(&root_path);
        let ignore_path = ignore_path.clone();
        let seen_ignore_path = Arc::clone(&seen_ignore_path);
        let tx = Arc::clone(&tx);
        // function executed for each DirEntry
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            let path = entry.path();
            // check if that file is `ignore_path`, unless we've seen it already
            if !seen_ignore_path.load(Ordering::Acquire) {
                let ignore_path = ignore_path
                    .as_ref()
                    .expect("ignore_path should always be Some when seen_ignore_path is false");
                if path == ignore_path {
                    // record that we've seen the zip file
                    seen_ignore_path.swap(true, Ordering::Release);
                    return WalkState::Continue;
                }
            }
            // first hash the filename/dirname to make sure it can't be renamed or removed
            let mut hasher = <Sha256 as Digest>::new();
            hasher.update(
                path.strip_prefix(root_path.as_ref())
                    .expect("path should be a child of root")
                    .to_slash_lossy()
                    .as_bytes(),
            );
            // for files, also hash the contents
            if let Some(true) = entry.file_type().map(|t| t.is_file()) {
                if let Ok(file) = File::open(path) {
                    let mut reader = BufReader::new(file);
                    let hash = hash_content(&mut reader);
                    hasher.update(hash);
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
    let mut hashes = hashes.join().expect("Failed to join thread");
    let mut hasher = <Sha256 as Digest>::new();
    hashes.par_sort_unstable();
    // hash the hashes (yo dawg...)
    for hash in hashes.iter() {
        hasher.update(hash);
    }
    let hash: [u8; 32] = hasher.finalize().into();
    Ok(const_hex::encode(hash).into())
}

/// Compute the SHA256 hash of the contents of a file
pub fn hash_file(path: impl AsRef<Path>) -> Result<IntegrityChecksum, std::io::Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let bytes = hash_content(&mut reader);
    Ok(const_hex::encode(bytes).into())
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
            assert_eq!(sanitize_dependency_name(filename), "valid-filename.txt");
        }
        assert_eq!(sanitize_dependency_name("valid~1.0.0"), "valid~1.0.0");
        assert_eq!(sanitize_dependency_name("valid~1*0.0"), "valid~1-0.0");
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
        assert_eq!(hash, "4671014a36f223796de8760df8125ca6e5a749e162dd5690e815132621dd8bfb".into());
    }

    #[test]
    fn test_hash_folder_abs_path_unsensitive() {
        let folder1 = create_test_folder("test", "test_hash_folder1");
        let folder2 = create_test_folder("test", "test_hash_folder2");
        let hash1 = hash_folder(&folder1, None).unwrap();
        let hash2 = hash_folder(&folder2, None).unwrap();
        fs::remove_dir_all(&folder1).unwrap();
        fs::remove_dir_all(&folder2).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_folder_rel_path_sensitive() {
        let folder = create_test_folder("test", "test_hash_folder_rel_path_sensitive");
        let hash1 = hash_folder(&folder, None).unwrap();
        fs::rename(folder.join("a.txt"), folder.join("c.txt")).unwrap();
        let hash2 = hash_folder(&folder, None).unwrap();
        fs::remove_dir_all(&folder).unwrap();
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
        let test_folder = target_dir.as_ref().canonicalize().unwrap().join(dirname);
        fs::create_dir(&test_folder).expect("could not create test folder");
        fs::write(test_folder.join("a.txt"), "this is a test file")
            .expect("could not write to test file a");
        fs::write(test_folder.join("b.txt"), "this is a second test file")
            .expect("could not write to test file b");
        test_folder
    }
}
