use once_cell::sync::Lazy;
use regex::Regex;
use simple_home_dir::home_dir;
use std::{
    env,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};
use yansi::Paint as _;

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

pub fn define_security_file_location() -> String {
    let custom_security_file = if cfg!(test) {
        return "./test_save_jwt".to_string();
    } else {
        option_env!("SOLDEER_LOGIN_FILE")
    };

    if let Some(file) = custom_security_file {
        if !file.is_empty() && Path::new(file).exists() {
            return file.to_string();
        }
    }

    let home = home_dir();
    match home {
        Some(_) => {}
        None => {
            println!(
                "{}",
                "HOME(linux) or %UserProfile%(Windows) path variable is not set, we can not determine the user's home directory. Please define this environment variable or define a custom path for the login file using the SOLDEER_LOGIN_FILE environment variable.".red()
            );
        }
    }
    let security_directory = home.unwrap().join(".soldeer");
    if !security_directory.exists() {
        fs::create_dir(&security_directory).unwrap();
    }
    let security_file = &security_directory.join(".soldeer_login");
    String::from(security_file.to_str().unwrap())
}

pub fn remove_empty_lines(path: impl AsRef<Path>) -> Result<(), io::Error> {
    let file = File::open(path.as_ref())?;
    let reader = BufReader::new(file);
    let lines: Vec<_> =
        reader.lines().map_while(Result::ok).filter(|l| !l.trim().is_empty()).collect();
    let mut file = File::create(path.as_ref())?;
    for line in lines {
        writeln!(file, "{}", line)?;
    }
    Ok(())
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

#[cfg(not(test))]
pub fn sha256_digest(dependency_name: &str, dependency_version: &str) -> String {
    use crate::DEPENDENCY_DIR;

    let bytes = std::fs::read(
        DEPENDENCY_DIR.join(format!("{}-{}.zip", dependency_name, dependency_version)),
    )
    .unwrap(); // Vec<u8>
    sha256::digest(bytes)
}

#[cfg(test)]
pub fn sha256_digest(_dependency_name: &str, _dependency_version: &str) -> String {
    "5019418b1e9128185398870f77a42e51d624c44315bb1572e7545be51d707016".to_string()
}
