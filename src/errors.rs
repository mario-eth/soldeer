use std::{fmt, io, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct SoldeerError {
    pub message: String,
}

impl fmt::Display for SoldeerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PushError {
    pub name: String,
    pub version: String,
    pub cause: String,
}

impl PushError {
    pub fn new(name: &str, version: &str, cause: &str) -> PushError {
        PushError { name: name.to_string(), version: version.to_string(), cause: cause.to_string() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoginError {
    pub cause: String,
}

impl LoginError {
    pub fn new(cause: &str) -> LoginError {
        LoginError { cause: cause.to_string() }
    }
}

#[derive(Error, Debug)]
pub enum LockError {
    #[error("soldeer.lock is missing")]
    Missing,

    #[error("dependency {0} is already installed")]
    DependencyInstalled(String),

    #[error("IO error for soldeer.lock: {0}")]
    IOError(#[from] io::Error),

    #[error("error generating soldeer.lock contents: {0}")]
    SerializeError(#[from] toml_edit::ser::Error),
}

#[derive(Error, Debug)]
pub enum JanitorError {
    #[error("missing dependency {0}")]
    MissingDependency(String),

    #[error("error during IO operation for {path:?}: {source}")]
    IOError { path: PathBuf, source: io::Error },

    #[error("error during lockfile operation: {0}")]
    LockError(LockError), // TODO: derive from LockError
}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("error downloading dependency: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("error extracting dependency: {0}")]
    UnzipError(#[from] zip_extract::ZipExtractError),

    #[error("error during git operation: {0}")]
    GitError(String),

    #[error("error during IO operation for {path:?}: {source}")]
    IOError { path: PathBuf, source: io::Error },

    #[error("Project {0} not found, please check the dependency name (project name) or create a new project on https://soldeer.xyz")]
    ProjectNotFound(String),

    #[error("Could not get the dependency URL for {0}")]
    URLNotFound(String),

    #[error("Could not get the last forge dependency")]
    ForgeStdError,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("config file is not valid: {0}")]
    Parsing(#[from] toml_edit::TomlError),

    #[error("config file is missing the `[dependencies]` section")]
    MissingDependencies,

    #[error("invalid user input: {source}")]
    PromptError { source: io::Error },

    #[error("invalid prompt option")]
    InvalidPromptOption,

    #[error("error writing to config file: {0}")]
    FileWriteError(#[from] io::Error),

    #[error("empty `version` field in {0}")]
    EmptyVersion(String),

    #[error("missing `{field}` field in {dep}")]
    MissingField { field: String, dep: String },

    #[error("invalid `{field}` field in {dep}")]
    InvalidField { field: String, dep: String },

    #[error("dependency {0} is not valid")]
    InvalidDependency(String),

    #[error("dependency {0} was not found")]
    MissingDependency(String),
}
