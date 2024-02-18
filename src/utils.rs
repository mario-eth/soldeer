use simple_home_dir::home_dir;
use std::env;
use std::fs::{
    self,
    File,
};
use std::io::{
    BufReader,
    Read,
};
use std::path::{
    Path,
    PathBuf,
};
use std::process::exit;
use yansi::Paint;

// get the current working directory
pub fn get_current_working_dir() -> std::io::Result<PathBuf> {
    env::current_dir()
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
pub fn read_file(path: String) -> Result<Vec<u8>, std::io::Error> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut buffer = Vec::new();

    // Read file into vector.
    reader.read_to_end(&mut buffer)?;

    Ok(buffer)
}

pub fn define_security_file_location() -> String {
    let custom_security_file = option_env!("SOLDEER_LOGIN_FILE");
    if custom_security_file.is_some()
        && !custom_security_file.unwrap().is_empty()
        && Path::new(custom_security_file.unwrap()).exists()
    {
        #[allow(clippy::unnecessary_unwrap)]
        return String::from(custom_security_file.unwrap());
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
