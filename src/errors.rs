use std::{
    io,
    path::{PathBuf, StripPrefixError},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SoldeerError {
    #[error("error during login: {0}")]
    AuthError(#[from] AuthError),

    #[error("error during config operation: {0}")]
    ConfigError(#[from] ConfigError),

    #[error("error during downloading ({dep}): {source}")]
    DownloadError { dep: String, source: DownloadError },

    #[error("error during janitor operation: {0}")]
    JanitorError(#[from] JanitorError),

    #[error("error during lockfile operation: {0}")]
    LockError(#[from] LockError),

    #[error("error during publishing: {0}")]
    PublishError(#[from] PublishError),
}

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("login error: invalid email")]
    InvalidEmail,

    #[error("login error: invalid email or password")]
    InvalidCredentials,

    #[error("missing token, you are not connected")]
    MissingToken,

    #[error("error during IO operation for the security file: {0}")]
    IOError(#[from] io::Error),

    #[error("http error during login: {0}")]
    HttpError(#[from] reqwest::Error),
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

    #[error("error writing to remappings file: {0}")]
    RemappingsError(io::Error),

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

    #[error("error parsing config file: {0}")]
    DeserializeError(#[from] toml_edit::de::Error),
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

    #[error("error during async operation: {0}")]
    AsyncError(#[from] tokio::task::JoinError),

    #[error("error during dependency sanitization operation: The dependency name contains illegal characters")]
    FileNameError,
}

#[derive(Error, Debug)]
pub enum JanitorError {
    #[error("missing dependency {0}")]
    MissingDependency(String),

    #[error("error during IO operation for {path:?}: {source}")]
    IOError { path: PathBuf, source: io::Error },

    #[error("error during lockfile operation: {0}")]
    LockError(LockError), // TODO: derive from LockError

    #[error("error during dependency sanitization operation: The dependency name contains illegal characters")]
    FileNameError,
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
pub enum PublishError {
    #[error("no files to publish")]
    NoFiles,

    #[error("error during zipping: {0}")]
    ZipError(#[from] zip::result::ZipError),

    #[error("error during IO operation for {path:?}: {source}")]
    IOError { path: PathBuf, source: io::Error },

    #[error("error while computing the relative path: {0}")]
    RelativePathError(#[from] StripPrefixError),

    #[error("auth error: {0}")]
    AuthError(#[from] AuthError),

    #[error("error during publishing: {0}")]
    DownloadError(#[from] DownloadError),

    #[error("Project not found. Make sure you send the right dependency name. The dependency name is the project name you created on https://soldeer.xyz")]
    ProjectNotFound,

    #[error("dependency already exists")]
    AlreadyExists,

    #[error("the package is too big (over 50 MB)")]
    PayloadTooLarge,

    #[error("http error during publishing: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("invalid package name, only alphanumeric characters, `-` and `@` are allowed")]
    InvalidName,

    #[error("unknown http error")]
    UnknownError,
}
