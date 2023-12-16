use std::env;
use std::fs::{self};
use std::path::PathBuf;
use std::process::exit;

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
