use std::env;
use std::path::PathBuf;

// get the current working directory
pub fn get_current_working_dir() -> std::io::Result<PathBuf> {
    env::current_dir()
}
