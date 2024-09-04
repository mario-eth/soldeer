use crate::{
    download::{find_install_path, find_install_path_sync},
    errors::ConfigError,
    remappings::RemappingsLocation,
    utils::{get_url_type, UrlType},
};
use cliclack::{log::warning, select};
use serde::Deserialize;
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};
use toml_edit::{value, DocumentMut, InlineTable, Item, Table};

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
// make sure the struct is not constructible from the outside without using the new/from methods
#[non_exhaustive]
pub struct Paths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub dependencies: PathBuf,
    pub lock: PathBuf,
    pub remappings: PathBuf,
}

impl Paths {
    /// Instantiate all the paths needed for Soldeer
    ///
    /// The root path defaults to the current directory but can be overridden with the
    /// `SOLDEER_PROJECT_ROOT` environment variable.
    ///
    /// The paths are canonicalized.
    pub fn new() -> Result<Self> {
        let root = dunce::canonicalize(Self::get_root_path())?;
        let config = Self::get_config_path(&root)?;
        let dependencies = root.join("dependencies");
        let lock = root.join("soldeer.lock");
        let remappings = root.join("remappings.txt");

        Ok(Self { root, config, dependencies, lock, remappings })
    }

    /// Generate the paths object from a known root directory
    ///
    /// The `SOLDEER_PROJECT_ROOT` environment variable is ignored.
    ///
    /// The paths are canonicalized.
    #[allow(unused)]
    pub fn from_root(root: impl AsRef<Path>) -> Result<Self> {
        let root = dunce::canonicalize(root.as_ref())?;
        let config = Self::get_config_path(&root)?;
        let dependencies = root.join("dependencies");
        let lock = root.join("soldeer.lock");
        let remappings = root.join("remappings.txt");

        Ok(Self { root, config, dependencies, lock, remappings })
    }

    /// TODO: find the project's root directory and use that as the root instead of the current dir
    fn get_root_path() -> PathBuf {
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

    /// Get the path to the config file or prompt the user to create one
    fn get_config_path(root: impl AsRef<Path>) -> Result<PathBuf> {
        let foundry_path = root.as_ref().join("foundry.toml");
        if let Ok(contents) = fs::read_to_string(&foundry_path) {
            let doc: DocumentMut = contents.parse::<DocumentMut>()?;
            if doc.contains_table("dependencies") {
                return Ok(foundry_path);
            }
        }

        let soldeer_path = root.as_ref().join("soldeer.toml");
        if soldeer_path.exists() {
            return Ok(soldeer_path);
        }

        warning("No soldeer config found")?;
        let config_option: ConfigLocation = select("Select how you want to configure Soldeer")
            .initial_value("foundry")
            .item("foundry", "Using foundry.toml", "recommended")
            .item("soldeer", "Using soldeer.toml", "for non-foundry projects")
            .interact()?
            .try_into()?;

        create_example_config(config_option, &foundry_path, &soldeer_path)
    }
}

/// For clap
fn default_true() -> bool {
    true
}

/// The Soldeer config options
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SoldeerConfig {
    #[serde(default = "default_true")]
    pub remappings_generate: bool,

    #[serde(default)]
    pub remappings_regenerate: bool,

    #[serde(default = "default_true")]
    pub remappings_version: bool,

    #[serde(default)]
    pub remappings_prefix: String,

    #[serde(default)]
    pub remappings_location: RemappingsLocation,

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub enum GitIdentifier {
    Rev(String),
    Branch(String),
    Tag(String),
}

impl GitIdentifier {
    pub fn from_rev(rev: impl Into<String>) -> Self {
        let rev: String = rev.into();
        Self::Rev(rev)
    }

    pub fn from_branch(branch: impl Into<String>) -> Self {
        let branch: String = branch.into();
        Self::Branch(branch)
    }

    pub fn from_tag(tag: impl Into<String>) -> Self {
        let tag: String = tag.into();
        Self::Tag(tag)
    }
}

impl fmt::Display for GitIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let val = match self {
            Self::Rev(rev) => rev,
            Self::Branch(branch) => branch,
            Self::Tag(tag) => tag,
        };
        write!(f, "{val}")
    }
}

#[bon::builder(on(String, into))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub struct GitDependency {
    pub name: String,
    #[cfg_attr(feature = "serde", serde(rename = "version"))]
    pub version_req: String,
    pub git: String,
    pub identifier: Option<GitIdentifier>,
}

impl GitDependency {
    pub fn install_path_sync(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        find_install_path_sync(&self.into(), deps)
    }

    pub async fn install_path(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        find_install_path(&self.into(), deps).await
    }
}

impl fmt::Display for GitDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version_req)
    }
}

#[bon::builder(on(String, into))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub struct HttpDependency {
    pub name: String,
    #[cfg_attr(feature = "serde", serde(rename = "version"))]
    pub version_req: String,
    pub url: Option<String>,
}

impl HttpDependency {
    pub fn install_path_sync(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        find_install_path_sync(&self.into(), deps)
    }

    pub async fn install_path(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        find_install_path(&self.into(), deps).await
    }
}

impl fmt::Display for HttpDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version_req)
    }
}

// Dependency object used to store a dependency data
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub enum Dependency {
    Http(HttpDependency),
    Git(GitDependency),
}

impl Dependency {
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

    pub fn name(&self) -> &str {
        match self {
            Self::Http(dep) => &dep.name,
            Self::Git(dep) => &dep.name,
        }
    }

    pub fn version_req(&self) -> &str {
        match self {
            Self::Http(dep) => &dep.version_req,
            Self::Git(dep) => &dep.version_req,
        }
    }

    pub fn url(&self) -> Option<&String> {
        match self {
            Self::Http(dep) => dep.url.as_ref(),
            Self::Git(dep) => Some(&dep.git),
        }
    }

    pub fn install_path_sync(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        match self {
            Self::Http(dep) => dep.install_path_sync(deps),
            Self::Git(dep) => dep.install_path_sync(deps),
        }
    }

    pub async fn install_path(&self, deps: impl AsRef<Path>) -> Option<PathBuf> {
        match self {
            Self::Http(dep) => dep.install_path(deps).await,
            Self::Git(dep) => dep.install_path(deps).await,
        }
    }

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

    #[allow(dead_code)]
    pub fn is_http(&self) -> bool {
        matches!(self, Self::Http(_))
    }

    #[allow(dead_code)]
    pub fn as_http(&self) -> Option<&HttpDependency> {
        if let Self::Http(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn as_http_mut(&mut self) -> Option<&mut HttpDependency> {
        if let Self::Http(v) = self {
            Some(v)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn is_git(&self) -> bool {
        matches!(self, Self::Git(_))
    }

    #[allow(dead_code)]
    pub fn as_git(&self) -> Option<&GitDependency> {
        if let Self::Git(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_git_mut(&mut self) -> Option<&mut GitDependency> {
        if let Self::Git(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Http(dep) => write!(f, "{dep}"),
            Self::Git(dep) => write!(f, "{dep}"),
        }
    }
}

impl From<HttpDependency> for Dependency {
    fn from(dep: HttpDependency) -> Self {
        Self::Http(dep)
    }
}

impl From<&HttpDependency> for Dependency {
    fn from(dep: &HttpDependency) -> Self {
        Self::Http(dep.clone())
    }
}

impl From<GitDependency> for Dependency {
    fn from(dep: GitDependency) -> Self {
        Self::Git(dep)
    }
}

impl From<&GitDependency> for Dependency {
    fn from(dep: &GitDependency) -> Self {
        Self::Git(dep.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, Deserialize))]
pub enum ConfigLocation {
    Foundry,
    Soldeer,
}

impl TryFrom<&str> for ConfigLocation {
    type Error = ConfigError;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "foundry" => Ok(Self::Foundry),
            "soldeer" => Ok(Self::Soldeer),
            _ => Err(ConfigError::InvalidPromptOption),
        }
    }
}

/// Read the list of dependencies from the config file.
///
/// If no config file path is provided, then the path is inferred automatically.
pub fn read_config_deps(path: impl AsRef<Path>) -> Result<Vec<Dependency>> {
    let contents = fs::read_to_string(path)?;
    let doc: DocumentMut = contents.parse::<DocumentMut>()?;
    let Some(Some(data)) = doc.get("dependencies").map(|v| v.as_table()) else {
        return Err(ConfigError::MissingDependencies);
    };

    let mut dependencies: Vec<Dependency> = Vec::new();
    for (name, v) in data {
        dependencies.push(parse_dependency(name, v)?);
    }

    Ok(dependencies)
}

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

pub fn delete_from_config(dependency_name: &str, path: impl AsRef<Path>) -> Result<Dependency> {
    let contents = fs::read_to_string(&path)?;
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    let Some(item_removed) = doc["dependencies"].as_table_mut().unwrap().remove(dependency_name)
    else {
        return Err(ConfigError::MissingDependency(dependency_name.to_string()));
    };

    let dependency = parse_dependency(dependency_name, &item_removed)?;

    fs::write(path, doc.to_string())?;
    Ok(dependency)
}

fn parse_dependency(name: impl Into<String>, value: &Item) -> Result<Dependency> {
    let name: String = name.into();
    if let Some(version_req) = value.as_str() {
        if version_req.is_empty() {
            return Err(ConfigError::EmptyVersion(name));
        }
        if version_req.contains('=') {
            return Err(ConfigError::InvalidVersionReq(name));
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
    if version_req.contains('=') {
        return Err(ConfigError::InvalidVersionReq(name));
    }

    // check if it's a git dependency
    match table.get("git").map(|v| v.as_str()) {
        Some(None) => {
            return Err(ConfigError::InvalidField { field: "git".to_string(), dep: name });
        }
        Some(Some(git)) => {
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
            Ok(HttpDependency { name, version_req, url: Some(url.to_string()) }.into())
        }
    }
}

fn create_example_config(
    location: ConfigLocation,
    foundry_path: impl AsRef<Path>,
    soldeer_path: impl AsRef<Path>,
) -> Result<PathBuf> {
    match location {
        ConfigLocation::Foundry => {
            let foundry_path = foundry_path.as_ref();
            if foundry_path.exists() {
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
            let config_contents = format!("[dependencies]\n{}", dep);
            let config_path = write_to_config(&config_contents, "soldeer.toml");
            let res = read_config_deps(config_path);
            assert!(matches!(res, Err(ConfigError::EmptyVersion(_))), "{res:?}");
        }

        for dep in [
            r#""lib1" = "asdf=""#,
            r#""lib1" = { version = "asdf=" }"#,
            r#""lib1" = { version = "asdf=", url = "https://example.com" }"#,
            r#""lib1" = { version = "asdf=", git = "https://example.com/repo.git" }"#,
            r#""lib1" = { version = "asdf=", git = "https://example.com/repo.git", rev = "123456" }"#,
        ] {
            let config_contents = format!("[dependencies]\n{}", dep);
            let config_path = write_to_config(&config_contents, "soldeer.toml");
            let res = read_config_deps(config_path);
            assert!(matches!(res, Err(ConfigError::InvalidVersionReq(_))), "{res:?}");
        }
    }

    #[test]
    fn test_read_soldeer_config_deps_bad_git() {
        for dep in [
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", rev = "123456", branch = "dev" }"#,
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", rev = "123456", tag = "v1.0.0" }"#,
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", branch = "dev", tag = "v1.0.0" }"#,
            r#""lib1" = { version = "1.0.0", git = "https://example.com/repo.git", rev = "123456", branch = "dev", tag = "v1.0.0" }"#,
        ] {
            let config_contents = format!("[dependencies]\n{}", dep);
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
}
