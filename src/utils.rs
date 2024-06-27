use simple_home_dir::home_dir;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::exit;
use yansi::Paint;

// get the current working directory
pub fn get_current_working_dir() -> PathBuf {
    env::current_dir().unwrap()
}

pub fn read_file_to_string(path: &String) -> String {
    let contents: String = match fs::read_to_string(path) {
        // If successful return the files text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(_) => {
            // Write `msg` to `stderr`.
            eprintln!("Could not read file `{}`", path);
            // Exit the program with exit code `1`.
            exit(1);
        }
    };
    contents
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
                Paint::red(
                    "HOME(linux) or %UserProfile%(Windows) path variable is not set, we can not determine the user's home directory. Please define this environment variable or define a custom path for the login file using the SOLDEER_LOGIN_FILE environment variable.",
                    )
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

pub fn remove_empty_lines(filename: &str) {
    let file: File = File::open(filename).unwrap();

    let reader: BufReader<File> = BufReader::new(file);
    let mut new_content: String = String::new();
    let lines: Vec<_> = reader.lines().collect();
    let total: usize = lines.len();
    for (index, line) in lines.into_iter().enumerate() {
        let line: &String = line.as_ref().unwrap();
        // Making sure the line contains something
        if line.len() > 2 {
            if index == total - 1 {
                new_content.push_str(&line.to_string());
            } else {
                new_content.push_str(&format!("{}\n", line));
            }
        }
    }

    // Removing the annoying new lines at the end and beginning of the file
    new_content = String::from(new_content.trim_end_matches('\n'));
    new_content = String::from(new_content.trim_start_matches('\n'));
    let mut file: std::fs::File = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .append(false)
        .open(Path::new(filename))
        .unwrap();

    match write!(file, "{}", &new_content) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Couldn't write to file: {}", e);
        }
    }
}

pub fn get_base_url() -> String {
    if cfg!(test) {
        env::var("base_url").unwrap_or("http://0.0.0.0".to_string())
    } else {
        "https://api.soldeer.xyz".to_string()
    }
}

// Function to check for the presence of sensitive files or directories
pub fn check_for_sensitive_files_or_directories(path: &Path) -> bool {
    let env_file_exists = path.join(".env").exists();
    let git_dir_exists = path.join(".git").exists();
    env_file_exists || git_dir_exists
}

// Function to recursively check for sensitive files or directories in a given path
pub fn check_for_sensitive_files_or_directories_recursive(path: &Path) -> bool {
    if check_for_sensitive_files_or_directories(path) {
        return true;
    }

    if path.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let entry_path = entry.path();
            if check_for_sensitive_files_or_directories_recursive(&entry_path) {
                return true;
            }
        }
    }

    false
}

// Function to prompt the user for confirmation
pub fn prompt_user_for_confirmation() -> bool {
    println!("{}", Paint::yellow(
        "You are about to include some sensitive files in this version. Are you sure you want to continue?"
    ));
    println!("{}", Paint::cyan(
        "If you are not sure what sensitive files, you can run the dry-run command to check what will be pushed."
    ));

    print!("{}", Paint::green("Do you want to continue? (y/n): "));
    std::io::stdout().flush().unwrap();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}
