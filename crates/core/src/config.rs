//! Manage the Soldeer configuration and dependencies list.
use crate::{
    download::{find_install_path, find_install_path_sync},
    errors::ConfigError,
    remappings::RemappingsLocation,
    utils::{get_url_type, UrlType},
};
use derive_more::derive::{Display, From, FromStr};
use serde::Deserialize;
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};
use toml_edit::{value, Array, DocumentMut, InlineTable, Item, Table};

#[cfg(feature = "cli")]
use cliclack::{log::warning, select};

pub type Result<T> = std::result::Result<T, ConfigError>;

/// The paths used by Soldeer.
///
/// The paths are canonicalized on creation of the object.
///
/// To create this object, the [`Paths::new`] and [`Paths::from_root`] methods can be used.
///
/// # Examples
///
/// ```
/// # use soldeer_core::config::Paths;
/// # let dir = testdir::testdir!();
/// # std::env::set_current_dir(&dir).unwrap();
/// # std::fs::write("foundry.toml", "[dependencies]\n").unwrap();
/// let paths = Paths::new().unwrap(); // foundry.toml exists in the current path
/// assert_eq!(paths.root, std::env::current_dir().unwrap());
/// assert_eq!(paths.config, std::env::current_dir().unwrap().join("foundry.toml"));
///
/// let paths = Paths::from_root(&dir).unwrap(); // root is the given path
/// assert_eq!(paths.root, dir);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
// making sure the struct is not constructible from the outside without using the new/from methods
#[non_exhaustive]
pub struct Paths {
    /// The root directory of the project.
    ///
    /// At the moment, the current directory or the path given by the `SOLDEER_PROJECT_ROOT`
    /// environment variable.
    pub root: PathBuf,

    /// The path to the config file.
    ///
    /// `foundry.toml` if it contains a `[dependencies]` table, otherwise `soldeer.toml` if it
    /// exists. If neither file exists, the user is prompted to create one when the `cli`
    /// feature is enabled. If the `cli` feature is not enabled, the function will return the
    /// path to the `foundry.toml` by default. When the config file does not exist, a new one
    /// is created with default contents.
    pub config: PathBuf,

    /// The path to the dependencies folder (does not need to exist).
    ///
    /// This is `/dependencies` inside the root directory.
    pub dependencies: PathBuf,

    /// The path to the lockfile (does not need to exist).
    ///
    /// This is `/soldeer.lock` inside the root directory.
    pub lock: PathBuf,

    /// The path to the remappings file (does not need to exist).
    ///
    /// This path gets ignored if the remappings should be generated in the `foundry.toml` file.
    /// This is `/remappings.txt` inside the root directory.
    pub remappings: PathBuf,
}

impl Paths {
    /// Instantiate all the paths needed for Soldeer.
    ///
    /// The root path defaults to the current directory but can be overridden with the
    /// `SOLDEER_PROJECT_ROOT` environment variable.
    ///
    /// The paths are canonicalized.
    pub fn new() -> Result<Self> {
        Self::with_config(None)
    }

    /// Instantiate all the paths needed for Soldeer.
    ///
    /// The root path defaults to the current directory but can be overridden with the
    /// `SOLDEER_PROJECT_ROOT` environment variable.
    ///
    /// The paths are canonicalized.
    pub fn with_config(config_location: Option<ConfigLocation>) -> Result<Self> {
        let root = dunce::canonicalize(Self::get_root_path())?;
        let config = Self::get_config_path(&root, config_location)?;
        let dependencies = root.join("dependencies");
        let lock = root.join("soldeer.lock");
        let remappings = root.join("remappings.txt");

        Ok(Self { root, config, dependencies, lock, remappings })
    }

    /// Generate the paths object from a known root directory.
    ///
    /// The `SOLDEER_PROJECT_ROOT` environment variable is ignored.
    ///
    /// The paths are canonicalized.
    pub fn from_root(root: impl AsRef<Path>) -> Result<Self> {
        let root = dunce::canonicalize(root.as_ref())?;
        let config = Self::get_config_path(&root, None)?;
        let dependencies = root.join("dependencies");
        let lock = root.join("soldeer.lock");
        let remappings = root.join("remappings.txt");

        Ok(Self { root, config, dependencies, lock, remappings })
    }

    /// Get the root directory path.
    ///
    /// At the moment, this is the current directory, unless overridden by the
    /// `SOLDEER_PROJECT_ROOT` environment variable.
    fn get_root_path() -> PathBuf {
        // TODO: find the project's root directory and use that as the root instead of the current
        // dir
        env::var("SOLDEER_PROJECT_ROOT")
            .map(|p| {
                if p.is_empty() {
                    env::current_dir().expect("could not get current dir")
                } else {
                    PathBuf::from(p)
                }
            })
            .unwrap_or(env::current_dir().expect("could not get current dir"))
    }

    /// Get the path to the config file or prompt the user to choose one (only with `cli` feature
    /// flag).
    fn get_config_path(
        root: impl AsRef<Path>,
        config_location: Option<ConfigLocation>,
    ) -> Result<PathBuf> {
        let foundry_path = root.as_ref().join("foundry.toml");
        let soldeer_path = root.as_ref().join("soldeer.toml");
        // use the user preference if available
        if let Some(location) = config_location {
            return create_or_modify_config(location, &foundry_path, &soldeer_path);
        }

        // auto-detect, or prompt the user if we can't determine the config path and the cli feature
        // is enabled. Otherwise, we use `foundry.toml` by default.
        if let Ok(contents) = fs::read_to_string(&foundry_path) {
            let doc: DocumentMut = contents.parse::<DocumentMut>()?;
            if doc.contains_table("dependencies") {
                return Ok(foundry_path);
            }
        } else if soldeer_path.exists() {
            return Ok(soldeer_path);
        }

        #[cfg(feature = "cli")]
        warning("No soldeer config found")?;
        #[cfg(feature = "cli")]
        let config_option: ConfigLocation = select("Select how you want to configure Soldeer")
            .initial_value("foundry")
            .item("foundry", "Using foundry.toml", "recommended")
            .item("soldeer", "Using soldeer.toml", "for non-foundry projects")
            .interact()?
            .parse()
            .map_err(|_| ConfigError::InvalidPromptOption)?;

        #[cfg(not(feature = "cli"))]
        let config_option = ConfigLocation::Foundry;

        create_or_modify_config(config_option, &foundry_path, &soldeer_path)
    }

    pub fn foundry_default() -> PathBuf {
        let root: PathBuf =
            dunce::canonicalize(Self::get_root_path()).expect("could not get the root");
        root.join("foundry.toml")
    }

    pub fn soldeer_default() -> PathBuf {
        let root: PathBuf =
            dunce::canonicalize(Self::get_root_path()).expect("could not get the root");
        root.join("soldeer.toml")
    }
}

/// For clap
fn default_true() -> bool {
    true
}

/// The Soldeer config options.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SoldeerConfig {
    /// Whether to generate remappings or completely leave them untouched.
    ///
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub remappings_generate: bool,

    /// Whether to regenerate the remappings every time and ignore existing content.
    ///
    /// Defaults to `false`.
    #[serde(default)]
    pub remappings_regenerate: bool,

    /// Whether to include the version requirement string in the left part of the remappings.
    ///
    /// Defaults to `true`.
    #[serde(default = "default_true")]
    pub remappings_version: bool,

    /// A prefix to add to each dependency name in the left part of the remappings.
    ///
    /// None by default.
    #[serde(default)]
    pub remappings_prefix: String,

    /// The location where the remappings file should be generated.
    ///
    /// Either inside the `foundry.toml` config file or as a separate `remappings.txt` file.
    /// This gets ignored if the config file is `soldeer.toml`, in which case the remappings
    /// are always generated in a separate file.
    ///
    /// Defaults to [`RemappingsLocation::Txt`].
    #[serde(default)]
    pub remappings_location: RemappingsLocation,

    /// Whether to include dependencies from dependencies.
    ///
    /// For dependencies which use soldeer, the `soldeer install` command will be invoked.
    /// Git dependencies which have submodules will see their submodules cloned as well.
    ///
    /// Defaults to `false`.
    #[serde(default)]
    pub recursive_deps: bool,
}

impl Default for SoldeerConfig {
    fn default() -> Self {
        Self {
            remappings_generate: true,
            remappings_regenerate: false,
            remappings_version: true,
            remappings_prefix: String::new(),
            remappings_location: RemappingsLocation::default(),
            recursive_deps: false,
        }
    }
}

/// A git identifier used to specify a revision, branch or tag.
///
/// # Examples
///
/// ```
/// # use soldeer_core::config::GitIdentifier;
/// let rev = GitIdentifier::from_rev("082692fcb6b5b1ab8f856914897f7f2b46b84fd2");
/// let branch = GitIdentifier::from_branch("feature/foo");
/// let tag = GitIdentifier::from_tag("v1.0.0");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub enum GitIdentifier {
    /// A commit hash
    Rev(String),

    /// A branch name
    Branch(String),

    /// A tag name
    Tag(String),
}

impl GitIdentifier {
    /// Create a new git identifier from a revision hash.
    pub fn from_rev(rev: impl Into<String>) -> Self {
        let rev: String = rev.into();
        Self::Rev(rev)
    }

    /// Create a new git identifier from a branch name.
    pub fn from_branch(branch: impl Into<String>) -> Self {
        let branch: String = branch.into();
        Self::Branch(branch)
    }

    /// Create a new git identifier from a tag name.
    pub fn from_tag(tag: impl Into<String>) -> Self {
        let tag: String = tag.into();
        Self::Tag(tag)
    }
}

/// A git dependency config item.
///
/// This struct is used to represent a git dependency from the config file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bon::Builder)]
#[builder(on(String, into))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub struct GitDependency {
    /// The name of the dependency (user-defined).
    pub name: String,

    /// The version requirement string (semver).
    ///
    /// Example: `>=1.9.3 || ^2.0.0`
    ///
    /// When no operator is used before the version number, it defaults to `=` which pins the
    /// version.
    #[cfg_attr(feature = "serde", serde(rename = "version"))]
    pub version_req: String,

    /// The git URL, must end with `.git`.
    pub git: String,

    /// The git identifier (revision, branch or tag).
    ///
    /// If omitted, the main branch is used.
    pub identifier: Option<GitIdentifier>,
}

impl fmt::Display for GitDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version_req)
    }
}

/// An HTTP dependency config item.
///
/// This struct is used to represent an HTTP dependency from the config file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bon::Builder)]
#[builder(on(String, into))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub struct HttpDependency {
    /// The name of the dependency (user-defined).
    pub name: String,

    /// The version requirement string (semver).
    ///
    /// Example: `>=1.9.3 || ^2.0.0`
    ///
    /// When no operator is used before the version number, it defaults to `=` which pins the
    /// version.
    #[cfg_attr(feature = "serde", serde(rename = "version"))]
    pub version_req: String,

    /// The URL to the dependency.
    ///
    /// If omitted, the registry will be contacted to get the download URL for that dependency (by
    /// name).
    pub url: Option<String>,
}

impl fmt::Display for HttpDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version_req)
    }
}

/// A git or HTTP dependency config item.
///
/// A builder can be used to create the underlying [`HttpDependency`] or [`GitDependency`] and then
/// converted into this type with `.into()`.
///
/// # Examples
///
/// ```
/// # use soldeer_core::config::{Dependency, HttpDependency};
/// let dep: Dependency = HttpDependency::builder()
///     .name("my-dep")
///     .version_req("^1.0.0")
///     .url("https://...")
///     .build()
///     .into();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, From)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub enum Dependency {
    #[from(HttpDependency)]
    Http(HttpDependency),

    #[from(GitDependency)]
    Git(GitDependency),
}

impl Dependency {
    /// Create a new dependency from a name and version requirement string.
    ///
    /// The string should be in the format `name~version_req`.
    ///
    /// The version requirement string can use the semver format.
    ///
    /// Example: `dependency~^1.0.0`
    ///
    /// If a custom URL is provided, then the version requirement string
    /// cannot contain the `=` character, as it would break the remappings.
    ///
    /// The type of dependency ([`HttpDependency`] or [`GitDependency`]) is inferred from the URL
    /// format, which can be of the form `https://...`, `git@github.com:` or `git@gitlab.com:`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use soldeer_core::config::{Dependency, HttpDependency, GitDependency, GitIdentifier};
    /// assert_eq!(
    ///     Dependency::from_name_version("my-lib~^1.0.0", Some("https://foo.bar/zip.zip"), None)
    ///         .unwrap(),
    ///     HttpDependency::builder()
    ///         .name("my-lib")
    ///         .version_req("^1.0.0")
    ///         .url("https://foo.bar/zip.zip")
    ///         .build()
    ///         .into()
    /// );
    /// assert_eq!(
    ///     Dependency::from_name_version(
    ///         "my-lib~^1.0.0",
    ///         Some("git@github.com:foo/bar.git"),
    ///         Some(GitIdentifier::from_tag("v1.0.0"))
    ///     )
    ///     .unwrap(),
    ///     GitDependency::builder()
    ///         .name("my-lib")
    ///         .version_req("^1.0.0")
    ///         .git("git@github.com:foo/bar.git")
    ///         .identifier(GitIdentifier::from_tag("v1.0.0"))
    ///         .build()
    ///         .into()
    /// );
    /// ```
    pub fn from_name_version(
        name_version: &str,
        custom_url: Option<impl Into<String>>,
        identifier: Option<GitIdentifier>,
    ) -> Result<Self> {
        let (dependency_name, dependency_version_req) = name_version
            .split_once('~')
            .expect("dependency string should have name and version requirement");
        if dependency_version_req.is_empty() {
            return Err(ConfigError::EmptyVersion(dependency_name.to_string()));
        }
        Ok(match custom_url {
            Some(url) => {
                let url: String = url.into();
                // in this case (custom url or git dependency), the version requirement string is
                // going to be used as part of the folder name inside the
                // dependencies folder. As such, it's not allowed to contain the "="
                // character, because that would break the remappings.
                if dependency_version_req.contains('=') {
                    return Err(ConfigError::InvalidVersionReq(dependency_name.to_string()));
                }
                match get_url_type(&url)? {
                    UrlType::Git => GitDependency {
                        name: dependency_name.to_string(),
                        version_req: dependency_version_req.to_string(),
                        git: url,
                        identifier,
                    }
                    .into(),
                    UrlType::Http => HttpDependency {
                        name: dependency_name.to_string(),
                        version_req: dependency_version_req.to_string(),
                        url: Some(url),
                    }
                    .into(),
                }
            }
            None => HttpDependency {
                name: dependency_name.to_string(),
                version_req: dependency_version_req.to_string(),
                url: None,
            }
            .into(),
        })
    }

    /// Get the name of the dependency.
    pub fn name(&self) -> &str {
        match self {
            Self::Http(dep) => &dep.name,
            Self::Git(dep) => &dep.name,
        }
    }

    /// Get the version requirement string of the dependency.
    pub fn version_req(&self) -> &str {
        match self {
            Self::Http(dep) => &dep.version_req,
            Self::Git(dep) => &dep.version_req,
        }
    }

    /// Get the URL of the dependency.
    pub fn url(&self) -> Option<&String> {
        match self {
            Self::Http(dep) => dep.url.as_ref(),
            Self::Git(dep) => Some(&dep.git),
        }
    }

    /// Get the install path of the dependency (must exist already).
    pub fn install_path_sync(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        find_install_path_sync(self, deps)
    }

    /// Get the install path of the dependency in an async way (must exist already).
    pub async fn install_path(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        find_install_path(self, deps).await
    }

    /// Convert the dependency to a TOML value for saving to the config file.
    pub fn to_toml_value(&self) -> (String, Item) {
        match self {
            Self::Http(dep) => (
                dep.name.clone(),
                match &dep.url {
                    Some(url) => {
                        let mut table = InlineTable::new();
                        table.insert(
                            "version",
                            value(&dep.version_req)
                                .into_value()
                                .expect("version should be a valid toml value"),
                        );
                        table.insert(
                            "url",
                            value(url).into_value().expect("url should be a valid toml value"),
                        );
                        value(table)
                    }
                    None => value(&dep.version_req),
                },
            ),
            Self::Git(dep) => {
                let mut table = InlineTable::new();
                table.insert(
                    "version",
                    value(&dep.version_req)
                        .into_value()
                        .expect("version should be a valid toml value"),
                );
                table.insert(
                    "git",
                    value(&dep.git).into_value().expect("git URL should be a valid toml value"),
                );
                match &dep.identifier {
                    Some(GitIdentifier::Rev(rev)) => {
                        table.insert(
                            "rev",
                            value(rev).into_value().expect("rev should be a valid toml value"),
                        );
                    }
                    Some(GitIdentifier::Branch(branch)) => {
                        table.insert(
                            "branch",
                            value(branch)
                                .into_value()
                                .expect("branch should be a valid toml value"),
                        );
                    }
                    Some(GitIdentifier::Tag(tag)) => {
                        table.insert(
                            "tag",
                            value(tag).into_value().expect("tag should be a valid toml value"),
                        );
                    }
                    None => {}
                }
                (dep.name.clone(), value(table))
            }
        }
    }

    /// Check if the dependency is an HTTP dependency.
    pub fn is_http(&self) -> bool {
        matches!(self, Self::Http(_))
    }

    /// Cast to a HTTP dependency if it is one.
    pub fn as_http(&self) -> Option<&HttpDependency> {
        if let Self::Http(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Cast to a mutable HTTP dependency if it is one.
    pub fn as_http_mut(&mut self) -> Option<&mut HttpDependency> {
        if let Self::Http(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Check if the dependency is a git dependency.
    pub fn is_git(&self) -> bool {
        matches!(self, Self::Git(_))
    }

    /// Cast to a git dependency if it is one.
    pub fn as_git(&self) -> Option<&GitDependency> {
        if let Self::Git(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Cast to a mutable git dependency if it is one.
    pub fn as_git_mut(&mut self) -> Option<&mut GitDependency> {
        if let Self::Git(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl From<&HttpDependency> for Dependency {
    fn from(dep: &HttpDependency) -> Self {
        Self::Http(dep.clone())
    }
}

impl From<&GitDependency> for Dependency {
    fn from(dep: &GitDependency) -> Self {
        Self::Git(dep.clone())
    }
}

/// The location where the Soldeer config should be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, FromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub enum ConfigLocation {
    /// The `foundry.toml` file.
    Foundry,

    /// The `soldeer.toml` file.
    Soldeer,
}

impl From<ConfigLocation> for PathBuf {
    fn from(value: ConfigLocation) -> Self {
        match value {
            ConfigLocation::Foundry => Paths::foundry_default(),
            ConfigLocation::Soldeer => Paths::soldeer_default(),
        }
    }
}

/// Read the list of dependencies from the config file.
///
/// Dependencies are stored in a TOML table under the `dependencies` key.
/// Each key inside of the table is the name of the dependency and the value can be:
/// - a string representing the version requirement
/// - a table with the following fields:
///   - `version` (required): the version requirement string
///   - `url` (optional): the URL to the dependency's zip file
///   - `git` (optional): the git URL for git dependencies
///   - `rev` (optional): the revision hash for git dependencies
///   - `branch` (optional): the branch name for git dependencies
///   - `tag` (optional): the tag name for git dependencies
pub fn read_config_deps(path: impl AsRef<Path>) -> Result<Vec<Dependency>> {
    let contents = fs::read_to_string(path)?;
    let doc: DocumentMut = contents.parse::<DocumentMut>()?;
    let Some(Some(data)) = doc.get("dependencies").map(|v| v.as_table()) else {
        return Ok(Vec::new());
    };

    let mut dependencies: Vec<Dependency> = Vec::new();
    for (name, v) in data {
        dependencies.push(parse_dependency(name, v)?);
    }

    Ok(dependencies)
}

/// Read the Soldeer config from the config file.
pub fn read_soldeer_config(path: impl AsRef<Path>) -> Result<SoldeerConfig> {
    #[derive(Deserialize)]
    struct SoldeerConfigParsed {
        #[serde(default)]
        soldeer: SoldeerConfig,
    }

    let contents = fs::read_to_string(path)?;

    let config: SoldeerConfigParsed = toml_edit::de::from_str(&contents)?;

    Ok(config.soldeer)
}

/// Add a dependency to the config file.
pub fn add_to_config(dependency: &Dependency, config_path: impl AsRef<Path>) -> Result<()> {
    let contents = fs::read_to_string(&config_path)?;
    let mut doc: DocumentMut = contents.parse::<DocumentMut>()?;

    // in case we don't have the dependencies section defined in the config file, we add it
    if !doc.contains_table("dependencies") {
        doc.insert("dependencies", Item::Table(Table::default()));
    }

    let (name, value) = dependency.to_toml_value();
    doc["dependencies"]
        .as_table_mut()
        .expect("dependencies should be a table")
        .insert(&name, value);

    fs::write(config_path, doc.to_string())?;

    Ok(())
}

/// Delete a dependency from the config file.
pub fn delete_from_config(dependency_name: &str, path: impl AsRef<Path>) -> Result<Dependency> {
    let contents = fs::read_to_string(&path)?;
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    let Some(dependencies) = doc["dependencies"].as_table_mut() else {
        return Err(ConfigError::MissingDependency(dependency_name.to_string()));
    };
    let Some(item_removed) = dependencies.remove(dependency_name) else {
        return Err(ConfigError::MissingDependency(dependency_name.to_string()));
    };

    let dependency = parse_dependency(dependency_name, &item_removed)?;

    fs::write(path, doc.to_string())?;
    Ok(dependency)
}

/// Update the config file to add the `dependencies` folder as a source for libraries and the
/// `[dependencies]` table.
pub fn update_config_libs(foundry_config: impl AsRef<Path>) -> Result<()> {
    let contents = fs::read_to_string(&foundry_config)?;
    let mut doc: DocumentMut = contents.parse::<DocumentMut>()?;

    if !doc.contains_key("profile") {
        let mut profile = Table::default();
        profile["default"] = Item::Table(Table::default());
        profile.set_implicit(true);
        doc["profile"] = Item::Table(profile);
    }

    let profile = doc["profile"].as_table_mut().expect("profile should be a table");
    if !profile.contains_key("default") {
        profile["default"] = Item::Table(Table::default());
    }

    let default_profile =
        profile["default"].as_table_mut().expect("default profile should be a table");
    if !default_profile.contains_key("libs") {
        default_profile["libs"] = value(Array::from_iter(&["dependencies".to_string()]));
    }

    let libs = default_profile["libs"].as_array_mut().expect("libs should be an array");
    if !libs.iter().any(|v| v.as_str() == Some("dependencies")) {
        libs.push("dependencies");
    }

    // in case we don't have the dependencies section defined in the config file, we add it
    if !doc.contains_table("dependencies") {
        doc.insert("dependencies", Item::Table(Table::default()));
    }

    fs::write(foundry_config, doc.to_string())?;
    Ok(())
}

/// Parse a dependency from a TOML value.
///
/// The value can be a string (version requirement) or a table.
/// The table can have the following fields:
/// - `version` (required): the version requirement string
/// - `url` (optional): the URL to the dependency's zip file
/// - `git` (optional): the git URL for git dependencies
/// - `rev` (optional): the revision hash for git dependencies
/// - `branch` (optional): the branch name for git dependencies
/// - `tag` (optional): the tag name for git dependencies
///
/// Note that the version requirement string cannot contain the `=` symbol for git dependencies
/// and HTTP dependencies with a custom URL.
fn parse_dependency(name: impl Into<String>, value: &Item) -> Result<Dependency> {
    let name: String = name.into();
    if let Some(version_req) = value.as_str() {
        if version_req.is_empty() {
            return Err(ConfigError::EmptyVersion(name));
        }
        // this function does not retrieve the url
        return Ok(HttpDependency { name, version_req: version_req.to_string(), url: None }.into());
    }

    // we should have a table or inline table
    let table = {
        match value.as_inline_table() {
            Some(table) => table,
            None => match value.as_table() {
                // we normalize to inline table
                Some(table) => &table.clone().into_inline_table(),
                None => {
                    return Err(ConfigError::InvalidDependency(name));
                }
            },
        }
    };

    // version is needed in both cases
    let version_req = match table.get("version").map(|v| v.as_str()) {
        Some(None) => {
            return Err(ConfigError::InvalidField { field: "version".to_string(), dep: name });
        }
        None => {
            return Err(ConfigError::MissingField { field: "version".to_string(), dep: name });
        }
        Some(Some(version_req)) => version_req.to_string(),
    };
    if version_req.is_empty() {
        return Err(ConfigError::EmptyVersion(name));
    }

    // check if it's a git dependency
    match table.get("git").map(|v| v.as_str()) {
        Some(None) => {
            return Err(ConfigError::InvalidField { field: "git".to_string(), dep: name });
        }
        Some(Some(git)) => {
            // for git dependencies, the version requirement string is going to be used as part of
            // the folder name inside the dependencies folder. As such, it's not allowed to contain
            // the "=" character, because that would break the remappings.
            if version_req.contains('=') {
                return Err(ConfigError::InvalidVersionReq(name));
            }
            // rev/branch/tag fields are optional but need to be a string if present
            let rev = match table.get("rev").map(|v| v.as_str()) {
                Some(Some(rev)) => Some(rev.to_string()),
                Some(None) => {
                    return Err(ConfigError::InvalidField { field: "rev".to_string(), dep: name });
                }
                None => None,
            };
            let branch = match table.get("branch").map(|v| v.as_str()) {
                Some(Some(tag)) => Some(tag.to_string()),
                Some(None) => {
                    return Err(ConfigError::InvalidField {
                        field: "branch".to_string(),
                        dep: name,
                    });
                }
                None => None,
            };
            let tag = match table.get("tag").map(|v| v.as_str()) {
                Some(Some(tag)) => Some(tag.to_string()),
                Some(None) => {
                    return Err(ConfigError::InvalidField { field: "tag".to_string(), dep: name });
                }
                None => None,
            };
            let identifier = match (rev, branch, tag) {
                (Some(rev), None, None) => Some(GitIdentifier::from_rev(rev)),
                (None, Some(branch), None) => Some(GitIdentifier::from_branch(branch)),
                (None, None, Some(tag)) => Some(GitIdentifier::from_tag(tag)),
                (None, None, None) => None,
                _ => {
                    return Err(ConfigError::GitIdentifierConflict(name));
                }
            };
            return Ok(Dependency::Git(GitDependency {
                name,
                git: git.to_string(),
                version_req,
                identifier,
            }));
        }
        None => {}
    }

    // we should have a HTTP dependency
    match table.get("url").map(|v| v.as_str()) {
        Some(None) => Err(ConfigError::InvalidField { field: "url".to_string(), dep: name }),
        None => Ok(HttpDependency { name, version_req, url: None }.into()),
        Some(Some(url)) => {
            // for HTTP dependencies with custom URL, the version requirement string is going to be
            // used as part of the folder name inside the dependencies folder. As such,
            // it's not allowed to contain the "=" character, because that would break
            // the remappings.
            if version_req.contains('=') {
                return Err(ConfigError::InvalidVersionReq(name));
            }
            Ok(HttpDependency { name, version_req, url: Some(url.to_string()) }.into())
        }
    }
}

/// Create a basic config file with default contents if it doesn't exist, otherwise add
/// `[dependencies]`.
fn create_or_modify_config(
    location: ConfigLocation,
    foundry_path: impl AsRef<Path>,
    soldeer_path: impl AsRef<Path>,
) -> Result<PathBuf> {
    match location {
        ConfigLocation::Foundry => {
            let foundry_path = foundry_path.as_ref();
            if foundry_path.exists() {
                update_config_libs(foundry_path)?;
                return Ok(foundry_path.to_path_buf());
            }
            let contents = r#"[profile.default]
src = "src"
out = "out"
libs = ["dependencies"]

[dependencies]

# See more config options https://github.com/foundry-rs/foundry/blob/master/crates/config/README.md#all-options
"#;

            fs::write(foundry_path, contents)?;
            Ok(foundry_path.to_path_buf())
        }
        ConfigLocation::Soldeer => {
            let soldeer_path = soldeer_path.as_ref();
            if soldeer_path.exists() {
                return Ok(soldeer_path.to_path_buf());
            }

            fs::write(soldeer_path, "[dependencies]\n")?;
            Ok(soldeer_path.to_path_buf())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ConfigError;
    use path_slash::PathBufExt;
    use std::{fs, path::PathBuf};
    use temp_env::with_var;
    use testdir::testdir;

    fn write_to_config(content: &str, filename: &str) -> PathBuf {
        let path = testdir!().join(filename);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_paths_config_soldeer() {
        let config_path = write_to_config("[dependencies]\n", "soldeer.toml");
        with_var(
            "SOLDEER_PROJECT_ROOT",
            Some(config_path.parent().unwrap().to_string_lossy().to_string()),
            || {
                let res = Paths::new();
                assert!(res.is_ok(), "{res:?}");
                assert_eq!(res.unwrap().config.to_slash_lossy(), config_path.to_slash_lossy());
            },
        );
    }

    #[test]
    fn test_paths_config_foundry() {
        let config_contents = r#"[profile.default]
libs = ["dependencies"]

[dependencies]
"#;
        let config_path = write_to_config(config_contents, "foundry.toml");
        with_var(
            "SOLDEER_PROJECT_ROOT",
            Some(config_path.parent().unwrap().to_string_lossy().to_string()),
            || {
                let res = Paths::new();
                assert!(res.is_ok(), "{res:?}");
                assert_eq!(res.unwrap().config, config_path);
            },
        );
    }

    #[test]
    fn test_paths_from_root() {
        let config_path = write_to_config("[dependencies]\n", "soldeer.toml");
        let root = config_path.parent().unwrap();
        let res = Paths::from_root(root);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().root, root);
    }

    #[test]
    fn test_from_name_version_no_url() {
        let res = Dependency::from_name_version("dependency~1.0.0", None::<&str>, None);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            HttpDependency::builder().name("dependency").version_req("1.0.0").build().into()
        );
    }

    #[test]
    fn test_from_name_version_with_http_url() {
        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("https://github.com/user/repo/archive/123.zip"),
            None,
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            HttpDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .url("https://github.com/user/repo/archive/123.zip")
                .build()
                .into()
        );
    }

    #[test]
    fn test_from_name_version_with_git_url() {
        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("https://github.com/user/repo.git"),
            None,
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            GitDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .git("https://github.com/user/repo.git")
                .build()
                .into()
        );

        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("https://test:test@gitlab.com/user/repo.git"),
            None,
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            GitDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .git("https://test:test@gitlab.com/user/repo.git")
                .build()
                .into()
        );
    }

    #[test]
    fn test_from_name_version_with_git_url_rev() {
        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("https://github.com/user/repo.git"),
            Some(GitIdentifier::from_rev("123456")),
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            GitDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .git("https://github.com/user/repo.git")
                .identifier(GitIdentifier::from_rev("123456"))
                .build()
                .into()
        );
    }

    #[test]
    fn test_from_name_version_with_git_url_branch() {
        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("https://github.com/user/repo.git"),
            Some(GitIdentifier::from_branch("dev")),
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            GitDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .git("https://github.com/user/repo.git")
                .identifier(GitIdentifier::from_branch("dev"))
                .build()
                .into()
        );
    }

    #[test]
    fn test_from_name_version_with_git_url_tag() {
        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("https://github.com/user/repo.git"),
            Some(GitIdentifier::from_tag("v1.0.0")),
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            GitDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .git("https://github.com/user/repo.git")
                .identifier(GitIdentifier::from_tag("v1.0.0"))
                .build()
                .into()
        );
    }

    #[test]
    fn test_from_name_version_with_git_ssh() {
        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("git@github.com:user/repo.git"),
            None,
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            GitDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .git("git@github.com:user/repo.git")
                .build()
                .into()
        );
    }

    #[test]
    fn test_from_name_version_with_git_ssh_rev() {
        let res = Dependency::from_name_version(
            "dependency~1.0.0",
            Some("git@github.com:user/repo.git"),
            Some(GitIdentifier::from_rev("123456")),
        );
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            GitDependency::builder()
                .name("dependency")
                .version_req("1.0.0")
                .git("git@github.com:user/repo.git")
                .identifier(GitIdentifier::from_rev("123456"))
                .build()
                .into()
        );
    }

    #[test]
    fn test_from_name_version_empty_version() {
        let res = Dependency::from_name_version("dependency~", None::<&str>, None);
        assert!(matches!(res, Err(ConfigError::EmptyVersion(_))), "{res:?}");
    }

    #[test]
    fn test_from_name_version_invalid_version() {
        // for http deps, having the "=" character in the version requirement is ok
        let res = Dependency::from_name_version("dependency~asdf=", None::<&str>, None);
        assert!(res.is_ok(), "{res:?}");

        let res =
            Dependency::from_name_version("dependency~asdf=", Some("https://example.com"), None);
        assert!(matches!(res, Err(ConfigError::InvalidVersionReq(_))), "{res:?}");

        let res = Dependency::from_name_version(
            "dependency~asdf=",
            Some("git@github.com:user/repo.git"),
            None,
        );
        assert!(matches!(res, Err(ConfigError::InvalidVersionReq(_))), "{res:?}");
    }

    #[test]
    fn test_read_soldeer_config_default() {
        let config_contents = r#"[profile.default]
libs = ["dependencies"]
"#;
        let config_path = write_to_config(config_contents, "foundry.toml");
        let res = read_soldeer_config(config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), SoldeerConfig::default());
    }

    #[test]
    fn test_read_soldeer_config() {
        let config_contents = r#"[soldeer]
remappings_generate = false
remappings_regenerate = true
remappings_version = false
remappings_prefix = "@"
remappings_location = "config"
recursive_deps = true
"#;
        let expected = SoldeerConfig {
            remappings_generate: false,
            remappings_regenerate: true,
            remappings_version: false,
            remappings_prefix: "@".to_string(),
            remappings_location: RemappingsLocation::Config,
            recursive_deps: true,
        };

        let config_path = write_to_config(config_contents, "soldeer.toml");
        let res = read_soldeer_config(config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), expected);

        let config_path = write_to_config(config_contents, "foundry.toml");
        let res = read_soldeer_config(config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), expected);
    }

    #[test]
    fn test_read_foundry_config_deps() {
        let config_contents = r#"[profile.default]
libs = ["dependencies"]

[dependencies]
"lib1" = "1.0.0"
"lib2" = { version = "2.0.0" }
"lib3" = { version = "3.0.0", url = "https://example.com" }
"lib4" = { version = "4.0.0", git = "https://example.com/repo.git" }
"lib5" = { version = "5.0.0", git = "https://example.com/repo.git", rev = "123456" }
"lib6" = { version = "6.0.0", git = "https://example.com/repo.git", branch = "dev" }
"lib7" = { version = "7.0.0", git = "https://example.com/repo.git", tag = "v7.0.0" }
"#;
        let config_path = write_to_config(config_contents, "foundry.toml");
        let res = read_config_deps(config_path);
        assert!(res.is_ok(), "{res:?}");
        let result = res.unwrap();

        assert_eq!(
            result[0],
            HttpDependency::builder().name("lib1").version_req("1.0.0").build().into()
        );
        assert_eq!(
            result[1],
            HttpDependency::builder().name("lib2").version_req("2.0.0").build().into()
        );
        assert_eq!(
            result[2],
            HttpDependency::builder()
                .name("lib3")
                .version_req("3.0.0")
                .url("https://example.com")
                .build()
                .into()
        );
        assert_eq!(
            result[3],
            GitDependency::builder()
                .name("lib4")
                .version_req("4.0.0")
                .git("https://example.com/repo.git")
                .build()
                .into()
        );
        assert_eq!(
            result[4],
            GitDependency::builder()
                .name("lib5")
                .version_req("5.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_rev("123456"))
                .build()
                .into()
        );
        assert_eq!(
            result[5],
            GitDependency::builder()
                .name("lib6")
                .version_req("6.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_branch("dev"))
                .build()
                .into()
        );
        assert_eq!(
            result[6],
            GitDependency::builder()
                .name("lib7")
                .version_req("7.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_tag("v7.0.0"))
                .build()
                .into()
        );
    }

    #[test]
    fn test_read_soldeer_config_deps() {
        let config_contents = r#"[dependencies]
"lib1" = "1.0.0"
"lib2" = { version = "2.0.0" }
"lib3" = { version = "3.0.0", url = "https://example.com" }
"lib4" = { version = "4.0.0", git = "https://example.com/repo.git" }
"lib5" = { version = "5.0.0", git = "https://example.com/repo.git", rev = "123456" }
"lib6" = { version = "6.0.0", git = "https://example.com/repo.git", branch = "dev" }
"lib7" = { version = "7.0.0", git = "https://example.com/repo.git", tag = "v7.0.0" }
"#;
        let config_path = write_to_config(config_contents, "soldeer.toml");
        let res = read_config_deps(config_path);
        assert!(res.is_ok(), "{res:?}");
        let result = res.unwrap();

        assert_eq!(
            result[0],
            HttpDependency::builder().name("lib1").version_req("1.0.0").build().into()
        );
        assert_eq!(
            result[1],
            HttpDependency::builder().name("lib2").version_req("2.0.0").build().into()
        );
        assert_eq!(
            result[2],
            HttpDependency::builder()
                .name("lib3")
                .version_req("3.0.0")
                .url("https://example.com")
                .build()
                .into()
        );
        assert_eq!(
            result[3],
            GitDependency::builder()
                .name("lib4")
                .version_req("4.0.0")
                .git("https://example.com/repo.git")
                .build()
                .into()
        );
        assert_eq!(
            result[4],
            GitDependency::builder()
                .name("lib5")
                .version_req("5.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_rev("123456"))
                .build()
                .into()
        );
        assert_eq!(
            result[5],
            GitDependency::builder()
                .name("lib6")
                .version_req("6.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_branch("dev"))
                .build()
                .into()
        );
        assert_eq!(
            result[6],
            GitDependency::builder()
                .name("lib7")
                .version_req("7.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_tag("v7.0.0"))
                .build()
                .into()
        );
    }

    #[test]
    fn test_read_soldeer_config_deps_bad_version() {
        for dep in [
            r#""lib1" = """#,
            r#""lib1" = { version = "" }"#,
            r#""lib1" = { version = "", url = "https://example.com" }"#,
            r#""lib1" = { version = "", git = "https://example.com/repo.git" }"#,
            r#""lib1" = { version = "", git = "https://example.com/repo.git", rev = "123456" }"#,
        ] {
            let config_contents = format!("[dependencies]\n{dep}");
            let config_path = write_to_config(&config_contents, "soldeer.toml");
            let res = read_config_deps(config_path);
            assert!(matches!(res, Err(ConfigError::EmptyVersion(_))), "{res:?}");
        }

        for dep in [
            r#""lib1" = { version = "asdf=", url = "https://example.com" }"#,
            r#""lib1" = { version = "asdf=", git = "https://example.com/repo.git" }"#,
            r#""lib1" = { version = "asdf=", git = "https://example.com/repo.git", rev = "123456" }"#,
        ] {
            let config_contents = format!("[dependencies]\n{dep}");
            let config_path = write_to_config(&config_contents, "soldeer.toml");
            let res = read_config_deps(config_path);
            assert!(matches!(res, Err(ConfigError::InvalidVersionReq(_))), "{res:?}");
        }

        // it's ok to have the "=" character in the version requirement for HTTP dependencies
        // without a custom URL
        let config_contents = r#"[dependencies]
"lib1" = "asdf="
"lib2" = { version = "asdf=" }
"#;
        let config_path = write_to_config(config_contents, "soldeer.toml");
        let res = read_config_deps(config_path);
        assert!(res.is_ok(), "{res:?}");
    }

    #[test]
    fn test_read_soldeer_config_deps_bad_git() {
        for dep in [
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", rev = "123456", branch = "dev" }"#,
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", rev = "123456", tag = "v1.0.0" }"#,
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", branch = "dev", tag = "v1.0.0" }"#,
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", rev = "123456", branch = "dev", tag = "v1.0.0" }"#,
        ] {
            let config_contents = format!("[dependencies]\n{dep}");
            let config_path = write_to_config(&config_contents, "soldeer.toml");
            let res = read_config_deps(config_path);
            assert!(matches!(res, Err(ConfigError::GitIdentifierConflict(_))), "{res:?}");
        }
    }

    #[test]
    fn test_add_to_config() {
        let config_path = write_to_config("[dependencies]\n", "soldeer.toml");

        let deps: &[Dependency] = &[
            HttpDependency::builder().name("lib1").version_req("1.0.0").build().into(),
            HttpDependency::builder()
                .name("lib2")
                .version_req("1.0.0")
                .url("https://test.com/test.zip")
                .build()
                .into(),
            GitDependency::builder()
                .name("lib3")
                .version_req("1.0.0")
                .git("https://example.com/repo.git")
                .build()
                .into(),
            GitDependency::builder()
                .name("lib4")
                .version_req("1.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_rev("123456"))
                .build()
                .into(),
            GitDependency::builder()
                .name("lib5")
                .version_req("1.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_branch("dev"))
                .build()
                .into(),
            GitDependency::builder()
                .name("lib6")
                .version_req("1.0.0")
                .git("https://example.com/repo.git")
                .identifier(GitIdentifier::from_tag("v1.0.0"))
                .build()
                .into(),
        ];
        for dep in deps {
            let res = add_to_config(dep, &config_path);
            assert!(res.is_ok(), "{dep}: {res:?}");
        }

        let parsed = read_config_deps(&config_path).unwrap();
        for (dep, parsed) in deps.iter().zip(parsed.iter()) {
            assert_eq!(dep, parsed);
        }
    }

    #[test]
    fn test_add_to_config_no_section() {
        let config_path = write_to_config("", "soldeer.toml");
        let dep = Dependency::from_name_version("lib1~1.0.0", None::<&str>, None).unwrap();
        let res = add_to_config(&dep, &config_path);
        assert!(res.is_ok(), "{res:?}");
        let parsed = read_config_deps(&config_path).unwrap();
        assert_eq!(parsed[0], dep);
    }

    #[test]
    fn test_delete_from_config() {
        let config_contents = r#"[dependencies]
"lib1" = "1.0.0"
"lib2" = { version = "2.0.0" }
"lib3" = { version = "3.0.0", url = "https://example.com" }
"lib4" = { version = "4.0.0", git = "https://example.com/repo.git" }
"lib5" = { version = "5.0.0", git = "https://example.com/repo.git", rev = "123456" }
"lib6" = { version = "6.0.0", git = "https://example.com/repo.git", branch = "dev" }
"lib7" = { version = "7.0.0", git = "https://example.com/repo.git", tag = "v7.0.0" }
        "#;
        let config_path = write_to_config(config_contents, "soldeer.toml");
        let res = delete_from_config("lib1", &config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().name(), "lib1");
        assert_eq!(read_config_deps(&config_path).unwrap().len(), 6);

        let res = delete_from_config("lib2", &config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().name(), "lib2");
        assert_eq!(read_config_deps(&config_path).unwrap().len(), 5);

        let res = delete_from_config("lib3", &config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().name(), "lib3");
        assert_eq!(read_config_deps(&config_path).unwrap().len(), 4);

        let res = delete_from_config("lib4", &config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().name(), "lib4");
        assert_eq!(read_config_deps(&config_path).unwrap().len(), 3);

        let res = delete_from_config("lib5", &config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().name(), "lib5");
        assert_eq!(read_config_deps(&config_path).unwrap().len(), 2);

        let res = delete_from_config("lib6", &config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().name(), "lib6");
        assert_eq!(read_config_deps(&config_path).unwrap().len(), 1);

        let res = delete_from_config("lib7", &config_path);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap().name(), "lib7");
        assert!(read_config_deps(&config_path).unwrap().is_empty());
    }

    #[test]
    fn test_delete_from_config_missing() {
        let config_contents = r#"[dependencies]
"lib1" = "1.0.0"
        "#;
        let config_path = write_to_config(config_contents, "soldeer.toml");
        let res = delete_from_config("libfoo", &config_path);
        assert!(matches!(res, Err(ConfigError::MissingDependency(_))), "{res:?}");
    }

    #[test]
    fn test_update_config_libs() {
        let config_contents = r#"[profile.default]
libs = ["lib"]

[dependencies]
"#;
        let config_path = write_to_config(config_contents, "foundry.toml");
        let res = update_config_libs(&config_path);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&config_path).unwrap();
        assert_eq!(
            contents,
            r#"[profile.default]
libs = ["lib", "dependencies"]

[dependencies]
"#
        );
    }

    #[test]
    fn test_update_config_profile_empty() {
        let config_contents = r#"[dependencies]
"#;
        let config_path = write_to_config(config_contents, "foundry.toml");
        let res = update_config_libs(&config_path);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&config_path).unwrap();
        assert_eq!(
            contents,
            r#"[dependencies]

[profile.default]
libs = ["dependencies"]
"#
        );
    }

    #[test]
    fn test_update_config_libs_empty() {
        let config_contents = r#"[profile.default]
src = "src"

[dependencies]
"#;
        let config_path = write_to_config(config_contents, "foundry.toml");
        let res = update_config_libs(&config_path);
        assert!(res.is_ok(), "{res:?}");
        let contents = fs::read_to_string(&config_path).unwrap();
        assert_eq!(
            contents,
            r#"[profile.default]
src = "src"
libs = ["dependencies"]

[dependencies]
"#
        );
    }
}
