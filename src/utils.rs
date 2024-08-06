use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Digest, Sha256};
use simple_home_dir::home_dir;
use std::{
    env,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};
use yansi::Paint as _;

use crate::config::HttpDependency;

static GIT_SSH_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:git@github\.com|git@gitlab)").expect("git ssh regex should compile")
});
static GIT_HTTPS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:https://github\.com|https://gitlab\.com).*\.git$")
        .expect("git https regex should compile")
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
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
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
pub fn define_security_file_location() -> Result<PathBuf, std::io::Error> {
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

#[cfg(not(test))]
pub fn sha256_digest(dependency: &HttpDependency) -> String {
    use crate::DEPENDENCY_DIR;

    let file_name =
        sanitize_dependency_name(&format!("{}-{}.zip", dependency.name, dependency.version));

    let bytes = std::fs::read(DEPENDENCY_DIR.join(file_name)).unwrap(); // Vec<u8>
    sha256::digest(bytes)
}

#[cfg(test)]
pub fn sha256_digest(_dependency: &HttpDependency) -> String {
    "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
}

pub fn hash_content<R: Read>(content: &mut R) -> [u8; 32] {
    let mut hasher = <Sha256 as Digest>::new();
    let mut buf = [0; 1024];
    while let Ok(size) = content.read(&mut buf) {
        hasher.update(&buf[0..size]);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
