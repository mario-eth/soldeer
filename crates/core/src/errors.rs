use std::{
    io,
    path::{PathBuf, StripPrefixError},
};
use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum SoldeerError {
    #[error("error during login: {0}")]
    AuthError(#[from] AuthError),

    #[error("error during config operation: {0}")]
    ConfigError(#[from] ConfigError),

    #[error("error during downloading ({dep}): {source}")]
    DownloadError { dep: String, source: DownloadError },

    #[error("error during install operation: {0}")]
    InstallError(#[from] InstallError),

    #[error("error during lockfile operation: {0}")]
    LockError(#[from] LockError),

    #[error("error during publishing: {0}")]
    PublishError(#[from] PublishError),

    #[error("error during remappings operation: {0}")]
    RemappingsError(#[from] RemappingsError),

    #[error("error during registry operation: {0}")]
    RegistryError(#[from] RegistryError),

    #[error("error during update operation: {0}")]
    UpdateError(#[from] UpdateError),

    #[error("error during IO operation: {0}")]
    IOError(#[from] io::Error),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum AuthError {
    #[error("login error: invalid email or password")]
    InvalidCredentials,

    #[error("missing token, run `soldeer login`")]
    MissingToken,

    #[error("error during IO operation for the security file: {0}")]
    IOError(#[from] io::Error),

    #[error("http error during login: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("TUI disabled and no credentials passed via CLI")]
    TuiDisabled,
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ConfigError {
    #[error("config file is not valid: {0}")]
    Parsing(#[from] toml_edit::TomlError),

    #[error("error writing to config file: {0}")]
    FileWriteError(#[from] io::Error),

    #[error("empty `version` field in {0}")]
    EmptyVersion(String),

    #[error("missing `{field}` field in {dep}")]
    MissingField { field: String, dep: String },

    #[error("invalid `{field}` field in {dep}")]
    InvalidField { field: String, dep: String },

    #[error("field `{field}` conflicts with `{conflicts_with}` in {dep}")]
    FieldConflict { field: String, conflicts_with: String, dep: String },

    #[error("only one of `rev`, `branch` or `tag` can be specified for git dependency {0}")]
    GitIdentifierConflict(String),

    #[error("dependency {0} is not valid")]
    InvalidDependency(String),

    #[error("dependency {0} was not found")]
    MissingDependency(String),

    #[error("error parsing config file: {0}")]
    DeserializeError(#[from] toml_edit::de::Error),

    #[error("error generating config file: {0}")]
    SerializeError(#[from] toml_edit::ser::Error),

    #[error("error during config operation: {0}")]
    DownloadError(#[from] DownloadError),

    #[error("the version requirement string for {0} cannot contain the equal symbol for git dependencies and http dependencies with a custom URL")]
    InvalidVersionReq(String),

    #[error("dependency specifier {0} cannot be parsed as name~version")]
    InvalidNameAndVersion(String),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum DownloadError {
    #[error("error downloading dependency: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("error extracting dependency: {0}")]
    UnzipError(#[from] zip_extract::ZipExtractError),

    #[error("error during git operation: {0}")]
    GitError(String),

    #[error("error during IO operation for {path:?}: {source}")]
    IOError { path: PathBuf, source: io::Error },

    #[error("error during async operation: {0}")]
    AsyncError(#[from] tokio::task::JoinError),

    #[error("could download the dependencies of this dependency {0}")]
    SubdependencyError(String),

    #[error("the provided URL is invalid: {0}")]
    InvalidUrl(String),

    #[error("error during registry operation: {0}")]
    RegistryError(#[from] RegistryError),

    #[error("dependency not found: {0}")]
    DependencyNotFound(String),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum InstallError {
    #[error("zip checksum for {path} does not match lock file: expected {expected}, got {actual}")]
    ZipIntegrityError { path: PathBuf, expected: String, actual: String },

    #[error("error during IO operation for {path:?}: {source}")]
    IOError { path: PathBuf, source: io::Error },

    #[error("error during git command: {0}")]
    GitError(String),

    #[error("error during dependency installation: {0}")]
    DownloadError(#[from] DownloadError),

    #[error("error during dependency installation: {0}")]
    ConfigError(#[from] ConfigError),

    #[error("error during async operation: {0}")]
    AsyncError(#[from] tokio::task::JoinError),

    #[error("error during forge command: {0}")]
    ForgeError(String),

    #[error("error during registry operation: {0}")]
    RegistryError(#[from] RegistryError),

    #[error("error with lockfile: {0}")]
    LockError(#[from] LockError),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum LockError {
    #[error("soldeer.lock is missing")]
    Missing,

    #[error("dependency {0} is already installed")]
    DependencyInstalled(String),

    #[error("IO error for soldeer.lock: {0}")]
    IOError(#[from] io::Error),

    #[error("error generating soldeer.lock contents: {0}")]
    SerializeError(#[from] toml_edit::ser::Error),

    #[error("lock entry does not match expected type")]
    TypeMismatch,

    #[error("missing `{field}` field in lock entry for {dep}")]
    MissingField { field: String, dep: String },
}

#[derive(Error, Debug)]
#[non_exhaustive]
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

    #[error("registry error during publishing: {0}")]
    DownloadError(#[from] RegistryError),

    #[error("Project not found. Make sure you send the right dependency name. The dependency name is the project name you created on https://soldeer.xyz")]
    ProjectNotFound,

    #[error("dependency already exists")]
    AlreadyExists,

    #[error("the package is too big (over 50 MB)")]
    PayloadTooLarge,

    #[error("http error during publishing: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("invalid package name, only alphanumeric characters, `-` and `@` are allowed. Length must be between 3 and 100 characters")]
    InvalidName,

    #[error("package version cannot be empty")]
    EmptyVersion,

    #[error("user cancelled operation")]
    UserAborted,

    #[error("unknown http error")]
    UnknownError,
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum RegistryError {
    #[error("error with registry request: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("could not get the dependency URL for {0}")]
    URLNotFound(String),

    #[error("project {0} not found, please check the dependency name (project name) or create a new project on https://soldeer.xyz")]
    ProjectNotFound(String),

    #[error("package {0} has no version")]
    NoVersion(String),

    #[error("no matching version found for {dependency} with version requirement {version_req}")]
    NoMatchingVersion { dependency: String, version_req: String },
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum RemappingsError {
    #[error("error writing to remappings file: {0}")]
    FileWriteError(#[from] io::Error),

    #[error("error while interacting with the config file: {0}")]
    ConfigError(#[from] ConfigError),

    #[error("dependency not found: {0}")]
    DependencyNotFound(String),
}

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum UpdateError {
    #[error("registry error: {0}")]
    RegistryError(#[from] RegistryError),

    #[error("download error: {0}")]
    DownloadError(#[from] DownloadError),

    #[error("error during install operation: {0}")]
    InstallError(#[from] InstallError),

    #[error("error during async operation: {0}")]
    AsyncError(#[from] tokio::task::JoinError),
}
