use std::{fmt, io};

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
pub struct MissingDependencies {
    pub name: String,
    pub version: String,
}

impl MissingDependencies {
    pub fn new(name: &str, version: &str) -> MissingDependencies {
        MissingDependencies { name: name.to_string(), version: version.to_string() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnzippingError {
    pub name: String,
    pub version: String,
}

impl UnzippingError {
    pub fn new(name: &str, version: &str) -> UnzippingError {
        UnzippingError { name: name.to_string(), version: version.to_string() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncorrectDependency {
    pub name: String,
    pub version: String,
}

impl IncorrectDependency {
    pub fn new(name: &str, version: &str) -> IncorrectDependency {
        IncorrectDependency { name: name.to_string(), version: version.to_string() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LockError {
    pub cause: String,
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "lock failed")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadError {
    pub name: String,
    pub version: String,
    pub cause: String,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "download failed for {}~{}", &self.name, &self.version)
    }
}

impl DownloadError {
    pub fn new(name: &str, version: &str, cause: &str) -> DownloadError {
        DownloadError {
            name: name.to_string(),
            version: version.to_string(),
            cause: cause.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectNotFound {
    pub name: String,
    pub cause: String,
}

impl ProjectNotFound {
    pub fn new(name: &str, cause: &str) -> ProjectNotFound {
        ProjectNotFound { name: name.to_string(), cause: cause.to_string() }
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
pub enum ConfigError {
    #[error("config file not found")]
    NotFound,

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

#[derive(Debug, Clone, PartialEq)]
pub struct DependencyError {
    pub name: String,
    pub version: String,
    pub cause: String,
}

impl fmt::Display for DependencyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "dependency operation failed for {}~{}", &self.name, &self.version)
    }
}

impl DependencyError {
    pub fn new(name: &str, version: &str, cause: &str) -> DependencyError {
        DependencyError {
            name: name.to_string(),
            version: version.to_string(),
            cause: cause.to_string(),
        }
    }
}
