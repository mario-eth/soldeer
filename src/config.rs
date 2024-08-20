use crate::{
    errors::ConfigError,
    remappings::RemappingsLocation,
    utils::{
        get_current_working_dir, get_url_type, read_file_to_string, run_git_command,
        sanitize_filename, UrlType,
    },
    DEPENDENCY_DIR, FOUNDRY_CONFIG_FILE, SOLDEER_CONFIG_FILE,
};
use cliclack::{log::warning, select};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, remove_dir_all, remove_file},
    path::{Path, PathBuf},
};
use toml_edit::{value, DocumentMut, InlineTable, Item, Table};
use yansi::Paint as _;

pub type Result<T> = std::result::Result<T, ConfigError>;

fn default_true() -> bool {
    true
}

/// The Soldeer config options
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
        SoldeerConfig {
            remappings_generate: true,
            remappings_regenerate: false,
            remappings_version: true,
            remappings_prefix: String::new(),
            remappings_location: Default::default(),
            recursive_deps: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GitDependency {
    pub name: String,
    pub version: String,
    pub git: String,
    pub rev: Option<String>,
}

impl GitDependency {
    pub fn install_path(&self) -> PathBuf {
        let sanitized_name = sanitize_filename(&format!("{}-{}", self.name, self.version));
        DEPENDENCY_DIR.join(sanitized_name)
    }
}

impl core::fmt::Display for GitDependency {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HttpDependency {
    pub name: String,
    pub version: String,
    pub url: Option<String>,
}

impl HttpDependency {
    pub fn install_path(&self) -> PathBuf {
        let sanitized_name = sanitize_filename(&format!("{}-{}", self.name, self.version));
        DEPENDENCY_DIR.join(sanitized_name)
    }
}

impl core::fmt::Display for HttpDependency {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version)
    }
}

// Dependency object used to store a dependency data
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Dependency {
    Http(HttpDependency),
    Git(GitDependency),
}

impl Dependency {
    pub fn from_name_version(
        name_version: &str,
        custom_url: Option<impl Into<String>>,
        rev: Option<impl Into<String>>,
    ) -> Result<Self> {
        let (dependency_name, dependency_version) =
            name_version.split_once('~').expect("dependency string should have name and version");
        Ok(match custom_url {
            Some(url) => {
                let url: String = url.into();
                match get_url_type(&url)? {
                    UrlType::Git => Dependency::Git(GitDependency {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        git: url,
                        rev: rev.map(|r| r.into()),
                    }),
                    UrlType::Http => Dependency::Http(HttpDependency {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        url: Some(url),
                    }),
                }
            }
            None => Dependency::Http(HttpDependency {
                name: dependency_name.to_string(),
                version: dependency_version.to_string(),
                url: None,
            }),
        })
    }

    pub fn name(&self) -> &str {
        match self {
            Dependency::Http(dep) => &dep.name,
            Dependency::Git(dep) => &dep.name,
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Dependency::Http(dep) => &dep.version,
            Dependency::Git(dep) => &dep.version,
        }
    }

    pub fn url(&self) -> Option<&String> {
        match self {
            Dependency::Http(dep) => dep.url.as_ref(),
            Dependency::Git(dep) => Some(&dep.git),
        }
    }

    pub fn install_path(&self) -> PathBuf {
        match self {
            Dependency::Http(dep) => dep.install_path(),
            Dependency::Git(dep) => dep.install_path(),
        }
    }

    pub fn to_toml_value(&self) -> (String, Item) {
        match self {
            Dependency::Http(dep) => (
                dep.name.clone(),
                match &dep.url {
                    Some(url) => {
                        let mut table = InlineTable::new();
                        table.insert(
                            "version",
                            value(&dep.version)
                                .into_value()
                                .expect("version should be a valid toml value"),
                        );
                        table.insert(
                            "url",
                            value(url).into_value().expect("url should be a valid toml value"),
                        );
                        value(table)
                    }
                    None => value(&dep.version),
                },
            ),
            Dependency::Git(dep) => (
                dep.name.clone(),
                match &dep.rev {
                    Some(rev) => {
                        let mut table = InlineTable::new();
                        table.insert(
                            "version",
                            value(&dep.version)
                                .into_value()
                                .expect("version should be a valid toml value"),
                        );
                        table.insert(
                            "git",
                            value(&dep.git)
                                .into_value()
                                .expect("git URL should be a valid toml value"),
                        );
                        table.insert(
                            "rev",
                            value(rev).into_value().expect("rev should be a valid toml value"),
                        );
                        value(table)
                    }
                    None => {
                        let mut table = InlineTable::new();
                        table.insert(
                            "version",
                            value(&dep.version)
                                .into_value()
                                .expect("version should be a valid toml value"),
                        );
                        table.insert(
                            "git",
                            value(&dep.git)
                                .into_value()
                                .expect("git URL should be a valid toml value"),
                        );

                        value(table)
                    }
                },
            ),
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

impl core::fmt::Display for Dependency {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Dependency::Http(dep) => write!(f, "{}", dep),
            Dependency::Git(dep) => write!(f, "{}", dep),
        }
    }
}

impl From<HttpDependency> for Dependency {
    fn from(dep: HttpDependency) -> Self {
        Dependency::Http(dep)
    }
}

impl From<GitDependency> for Dependency {
    fn from(dep: GitDependency) -> Self {
        Dependency::Git(dep)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfigLocation {
    Foundry,
    Soldeer,
}

impl TryFrom<&str> for ConfigLocation {
    type Error = ConfigError;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "foundry" => Ok(ConfigLocation::Foundry),
            "soldeer" => Ok(ConfigLocation::Soldeer),
            _ => Err(ConfigError::InvalidPromptOption),
        }
    }
}

pub fn get_config_path() -> Result<PathBuf> {
    let foundry_path: PathBuf = if cfg!(test) {
        env::var("config_file").map(|s| s.into()).unwrap_or(FOUNDRY_CONFIG_FILE.clone())
    } else {
        FOUNDRY_CONFIG_FILE.to_path_buf()
    };

    if let Ok(contents) = fs::read_to_string(&foundry_path) {
        let doc: DocumentMut = contents.parse::<DocumentMut>()?;
        if doc.contains_table("dependencies") {
            return Ok(foundry_path);
        }
    }

    let soldeer_path = SOLDEER_CONFIG_FILE.clone();
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

    create_example_config(config_option)
}

/// Read the list of dependencies from the config file
///
/// If no config file path is provided, then the path is inferred automatically
/// The returned list is sorted by name and version
pub fn read_config_deps(path: Option<impl AsRef<Path>>) -> Result<Vec<Dependency>> {
    let path: PathBuf = match path {
        Some(p) => p.as_ref().to_path_buf(),
        None => get_config_path()?,
    };
    let contents = read_file_to_string(&path);
    let doc: DocumentMut = contents.parse::<DocumentMut>()?;
    let Some(Some(data)) = doc.get("dependencies").map(|v| v.as_table()) else {
        return Err(ConfigError::MissingDependencies);
    };

    let mut dependencies: Vec<Dependency> = Vec::new();
    for (name, v) in data {
        dependencies.push(parse_dependency(name, v)?);
    }
    dependencies
        .sort_unstable_by(|a, b| a.name().cmp(b.name()).then_with(|| a.version().cmp(b.version())));

    Ok(dependencies)
}

pub fn read_soldeer_config(path: Option<impl AsRef<Path>>) -> Result<SoldeerConfig> {
    let path: PathBuf = match path {
        Some(p) => p.as_ref().to_path_buf(),
        None => get_config_path()?,
    };
    let contents = read_file_to_string(&path);

    #[derive(Deserialize)]
    struct SoldeerConfigParsed {
        #[serde(default)]
        soldeer: SoldeerConfig,
    }
    let config: SoldeerConfigParsed = toml_edit::de::from_str(&contents)?;

    Ok(config.soldeer)
}

pub fn add_to_config(dependency: &Dependency, config_path: impl AsRef<Path>) -> Result<()> {
    let contents = read_file_to_string(&config_path);
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

pub fn update_deps(dependencies: &[Dependency], config_path: impl AsRef<Path>) -> Result<()> {
    let contents = read_file_to_string(&config_path);
    let mut doc: DocumentMut = contents.parse::<DocumentMut>()?;
    // in case we don't have the dependencies section defined in the config file, we add it
    if !doc.contains_table("dependencies") {
        doc.insert("dependencies", Item::Table(Table::default()));
    }
    let deps = doc["dependencies"].as_table_mut().expect("dependencies should be a table");
    for dep in dependencies {
        let (name, value) = dep.to_toml_value();
        deps.insert(&name, value);
    }
    fs::write(config_path, doc.to_string())?;
    Ok(())
}

pub fn delete_config(dependency_name: &str, path: impl AsRef<Path>) -> Result<Dependency> {
    println!(
        "{}",
        format!("Removing the dependency {dependency_name} from the config file").green()
    );

    let contents = read_file_to_string(&path);
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    let Some(item_removed) = doc["dependencies"].as_table_mut().unwrap().remove(dependency_name)
    else {
        return Err(ConfigError::MissingDependency(dependency_name.to_string()));
    };

    let dependency = parse_dependency(dependency_name, &item_removed)?;

    fs::write(path, doc.to_string())?;

    Ok(dependency)
}

pub async fn remove_forge_lib() -> Result<()> {
    let root_dir = get_current_working_dir();
    let gitmodules_path = root_dir.join(".gitmodules");
    let lib_dir = root_dir.join("lib");
    let forge_std_dir = lib_dir.join("forge-std");
    run_git_command(&["rm", &forge_std_dir.to_string_lossy()], None).await?;
    if lib_dir.exists() {
        remove_dir_all(&lib_dir)?;
    }
    if gitmodules_path.exists() {
        remove_file(&gitmodules_path)?;
    }
    Ok(())
}

fn parse_dependency(name: impl Into<String>, value: &Item) -> Result<Dependency> {
    let name: String = name.into();
    if let Some(version) = value.as_str() {
        if version.is_empty() {
            return Err(ConfigError::EmptyVersion(name));
        }
        // this function does not retrieve the url
        return Ok(HttpDependency { name, version: version.to_string(), url: None }.into());
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
    let version = match table.get("version").map(|v| v.as_str()) {
        Some(None) => {
            return Err(ConfigError::InvalidField { field: "version".to_string(), dep: name });
        }
        None => {
            return Err(ConfigError::MissingField { field: "version".to_string(), dep: name });
        }
        Some(Some(version)) => version.to_string(),
    };

    // check if it's a git dependency
    match table.get("git").map(|v| v.as_str()) {
        Some(None) => {
            return Err(ConfigError::InvalidField { field: "git".to_string(), dep: name });
        }
        Some(Some(git)) => {
            // rev field is optional but needs to be a string if present
            let rev = match table.get("rev").map(|v| v.as_str()) {
                Some(Some(rev)) => Some(rev.to_string()),
                Some(None) => {
                    return Err(ConfigError::InvalidField { field: "rev".to_string(), dep: name });
                }
                None => None,
            };
            return Ok(Dependency::Git(GitDependency {
                name: name.to_string(),
                git: git.to_string(),
                version,
                rev,
            }));
        }
        None => {}
    }

    // we should have a HTTP dependency
    match table.get("url").map(|v| v.as_str()) {
        Some(None) => Err(ConfigError::InvalidField { field: "url".to_string(), dep: name }),
        None => Ok(Dependency::Http(HttpDependency { name: name.to_string(), version, url: None })),
        Some(Some(url)) => Ok(Dependency::Http(HttpDependency {
            name: name.to_string(),
            version,
            url: Some(url.to_string()),
        })),
    }
}

fn create_example_config(location: ConfigLocation) -> Result<PathBuf> {
    match location {
        ConfigLocation::Foundry => {
            if FOUNDRY_CONFIG_FILE.exists() {
                return Ok(FOUNDRY_CONFIG_FILE.to_path_buf());
            }
            let contents = r#"[profile.default]
src = "src"
out = "out"
libs = ["lib"]

[dependencies]

# See more config options https://github.com/foundry-rs/foundry/blob/master/crates/config/README.md#all-options
"#;

            fs::write(FOUNDRY_CONFIG_FILE.as_path(), contents)?;
            Ok(FOUNDRY_CONFIG_FILE.to_path_buf())
        }
        ConfigLocation::Soldeer => {
            if SOLDEER_CONFIG_FILE.exists() {
                return Ok(SOLDEER_CONFIG_FILE.to_path_buf());
            }

            fs::write(SOLDEER_CONFIG_FILE.as_path(), "[dependencies]\n")?;
            Ok(SOLDEER_CONFIG_FILE.to_path_buf())
        }
    }
}

////////////// TESTS //////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Dependency, errors::ConfigError, utils::get_current_working_dir};
    use rand::{distributions::Alphanumeric, Rng};
    use serial_test::serial;
    use std::{
        fs::{self, remove_file},
        io::Write,
        path::PathBuf,
    };

    #[tokio::test] // check dependencies as {version = "1.1.1"}
    #[serial]
    async fn read_foundry_config_version_v1_ok() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"@gearbox-protocol-periphery-v3" = "1.6.1"
"@openzeppelin-contracts" = "5.0.2"
"#;
        let target_config = define_config(true);

        write_to_config(&target_config, config_contents);

        let result = read_config_deps(Some(target_config.clone()))?;

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: None,
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test] // check dependencies as "1.1.1"
    #[serial]
    async fn read_foundry_config_version_v2_ok() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"@gearbox-protocol-periphery-v3" = "1.6.1"
"@openzeppelin-contracts" = "5.0.2"
"#;
        let target_config = define_config(true);

        write_to_config(&target_config, config_contents);

        let result = read_config_deps(Some(target_config.clone()))?;

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: None,
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test] // check dependencies as "1.1.1"
    #[serial]
    async fn read_soldeer_config_version_v1_ok() -> Result<()> {
        let config_contents = r#"
[remappings]
enabled = true

[dependencies]
"@gearbox-protocol-periphery-v3" = "1.6.1"
"@openzeppelin-contracts" = "5.0.2"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let result = read_config_deps(Some(target_config.clone()))?;

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: None,
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test] // check dependencies as "1.1.1"
    #[serial]
    async fn read_soldeer_config_version_v2_ok() -> Result<()> {
        let config_contents = r#"
[remappings]
enabled = true

[dependencies]
"@gearbox-protocol-periphery-v3" = "1.6.1"
"@openzeppelin-contracts" = "5.0.2"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let result = read_config_deps(Some(target_config.clone()))?;

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: None,
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn read_malformed_config_incorrect_version_string_fails() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"@gearbox-protocol-periphery-v3" = 1.6.1"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        assert!(matches!(
            read_config_deps(Some(target_config.clone())),
            Err(ConfigError::Parsing(_))
        ));
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn read_malformed_config_empty_version_string_fails() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"@gearbox-protocol-periphery-v3" = ""
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        assert!(matches!(
            read_config_deps(Some(target_config.clone())),
            Err(ConfigError::EmptyVersion(_))
        ));
        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn define_config_file_choses_foundry() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"#;
        let target_config = define_config(true);

        write_to_config(&target_config, config_contents);

        assert!(target_config.file_name().unwrap().to_string_lossy().contains("foundry"));
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn define_config_file_choses_soldeer() -> Result<()> {
        let config_contents = r#"
[dependencies]
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        assert!(target_config.file_name().unwrap().to_string_lossy().contains("soldeer"));
        let _ = remove_file(target_config);
        Ok(())
    }

    // #[test] // TODO check how to do this properly
    #[allow(dead_code)]
    fn create_new_file_if_not_defined_but_foundry_exists() -> Result<()> {
        let content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies", "libs"]

[dependencies]
forge-std = "1.9.1"
"#;

        let result = create_example_config(ConfigLocation::Foundry).unwrap();

        assert!(PathBuf::from(&result).file_name().unwrap().to_string_lossy().contains("foundry"));
        assert_eq!(read_file_to_string(&result), content);
        Ok(())
    }

    // #[test]// TODO check how to do this properly
    #[allow(dead_code)]
    fn create_new_file_if_not_defined_but_foundry_does_not_exists() -> Result<()> {
        let content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies", "libs"]

[dependencies]
forge-std = "1.9.1"
"#;

        let result = create_example_config(ConfigLocation::Foundry).unwrap();

        assert!(PathBuf::from(&result).file_name().unwrap().to_string_lossy().contains("foundry"));
        assert_eq!(read_file_to_string(&result), content);
        Ok(())
    }

    #[test]
    fn create_new_file_if_not_defined_soldeer() -> Result<()> {
        let content = r#"
[remappings]
enabled = true

[dependencies]
"#;

        let result = create_example_config(ConfigLocation::Soldeer).unwrap();

        assert!(PathBuf::from(&result).file_name().unwrap().to_string_lossy().contains("soldeer"));
        assert_eq!(read_file_to_string(&result), content);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_no_custom_url_first_dependency() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);
        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
        });
        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
dep1 = "1.0.0"
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_with_custom_url_first_dependency() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: Some("http://custom_url.com/custom.zip".to_string()),
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_no_custom_url_second_dependency() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = "5.1.0-my-version-is-cool"
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = "5.1.0-my-version-is-cool"
dep1 = "1.0.0"
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_with_custom_url_second_dependency() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = { version = "5.1.0-my-version-is-cool", url = "http://custom_url.com/cool-cool-cool.zip" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: Some("http://custom_url.com/custom.zip".to_string()),
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = { version = "5.1.0-my-version-is-cool", url = "http://custom_url.com/cool-cool-cool.zip" }
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_update_dependency_version() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = { version = "5.1.0-my-version-is-cool", url = "http://custom_url.com/cool-cool-cool.zip" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "old_dep".to_string(),
            version: "1.0.0".to_string(),
            url: Some("http://custom_url.com/custom.zip".to_string()),
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_update_dependency_version_no_custom_url() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = { version = "5.1.0-my-version-is-cool", url = "http://custom_url.com/cool-cool-cool.zip" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "old_dep".to_string(),
            version: "1.0.0".to_string(),
            url: None,
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
old_dep = "1.0.0"
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_not_altering_the_existing_contents() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

# we don't have [dependencies] declared
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

[dependencies]
dep1 = "1.0.0"

# we don't have [dependencies] declared
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_soldeer_no_custom_url_first_dependency() -> Result<()> {
        let mut content = r#"
[remappings]
enabled = true

[dependencies]
"#;

        let target_config = define_config(false);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: None,
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
[remappings]
enabled = true

[dependencies]
dep1 = "1.0.0"
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_soldeer_with_custom_url_first_dependency() -> Result<()> {
        let mut content = r#"
[remappings]
enabled = true

[dependencies]
"#;

        let target_config = define_config(false);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: Some("http://custom_url.com/custom.zip".to_string()),
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
[remappings]
enabled = true

[dependencies]
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_github_with_commit() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

# we don't have [dependencies] declared
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Git(GitDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            git: "git@github.com:foundry-rs/forge-std.git".to_string(),
            rev: Some("07263d193d621c4b2b0ce8b4d54af58f6957d97d".to_string()),
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

[dependencies]
dep1 = { version = "1.0.0", git = "git@github.com:foundry-rs/forge-std.git", rev = "07263d193d621c4b2b0ce8b4d54af58f6957d97d" }

# we don't have [dependencies] declared
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_github_previous_no_commit_then_with_commit() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

# we don't have [dependencies] declared

[dependencies]
dep1 = { version = "1.0.0", git = "git@github.com:foundry-rs/forge-std.git" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Git(GitDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            git: "git@github.com:foundry-rs/forge-std.git".to_string(),
            rev: Some("07263d193d621c4b2b0ce8b4d54af58f6957d97d".to_string()),
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

# we don't have [dependencies] declared

[dependencies]
dep1 = { version = "1.0.0", git = "git@github.com:foundry-rs/forge-std.git", rev = "07263d193d621c4b2b0ce8b4d54af58f6957d97d" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_github_previous_commit_then_no_commit() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

# we don't have [dependencies] declared

[dependencies]
dep1 = { version = "1.0.0", git = "git@github.com:foundry-rs/forge-std.git", rev = "07263d193d621c4b2b0ce8b4d54af58f6957d97d" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        let dependency = Dependency::Http(HttpDependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: Some("http://custom_url.com/custom.zip".to_string()),
        });

        add_to_config(&dependency, &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]
gas_reports = ['*']

# we don't have [dependencies] declared

[dependencies]
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn remove_from_the_config_single() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        delete_config("dep1", &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn remove_from_the_config_multiple() -> Result<()> {
        let mut content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
dep3 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
dep2 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        delete_config("dep1", &target_config).unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
dep3 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
dep2 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn remove_config_nonexistent_fails() -> Result<()> {
        let content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        let target_config = define_config(true);

        write_to_config(&target_config, content);

        assert!(matches!(
            delete_config("dep2", &target_config),
            Err(ConfigError::MissingDependency(_))
        ));

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    async fn read_soldeer_configs_all_set() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config
[profile.default]
libs = ["dependencies"]
[dependencies]
"@gearbox-protocol-periphery-v3" = "1.1.1"
[soldeer]
remappings_generate = true
remappings_prefix = "@"
remappings_regenerate = true
remappings_version = true
remappings_location = "config"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let sc = match read_soldeer_config(Some(target_config.clone())) {
            Ok(sc) => sc,
            Err(_) => {
                assert_eq!("False state", "");
                SoldeerConfig::default()
            }
        };
        let _ = remove_file(target_config);
        assert!(sc.remappings_prefix == *"@");
        assert!(sc.remappings_generate);
        assert!(sc.remappings_regenerate);
        assert!(sc.remappings_version);
        assert_eq!(sc.remappings_location, RemappingsLocation::Config);
        Ok(())
    }

    #[tokio::test]
    async fn read_soldeer_configs_generate_remappings() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config
[profile.default]
libs = ["dependencies"]
[dependencies]
"@gearbox-protocol-periphery-v3" = "1.1.1"
[soldeer]
remappings_generate = true
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let sc = match read_soldeer_config(Some(target_config.clone())) {
            Ok(sc) => sc,
            Err(_) => {
                assert_eq!("False state", "");
                SoldeerConfig::default()
            }
        };
        let _ = remove_file(target_config);
        assert!(sc.remappings_generate);
        assert!(sc.remappings_prefix.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn read_soldeer_configs_append_at_in_remappings() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config
[profile.default]
libs = ["dependencies"]
[dependencies]
"@gearbox-protocol-periphery-v3" = "1.1.1"
[soldeer]
remappings_prefix = "@"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let sc = match read_soldeer_config(Some(target_config.clone())) {
            Ok(sc) => sc,
            Err(_) => {
                assert_eq!("False state", "");
                SoldeerConfig::default()
            }
        };
        let _ = remove_file(target_config);
        assert!(sc.remappings_prefix == *"@");
        assert!(sc.remappings_generate);
        Ok(())
    }

    #[tokio::test]
    async fn read_soldeer_configs_reg_remappings() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config
[profile.default]
libs = ["dependencies"]
[dependencies]
"@gearbox-protocol-periphery-v3" = "1.1.1"
[soldeer]
remappings_regenerate = true
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let sc = match read_soldeer_config(Some(target_config.clone())) {
            Ok(sc) => sc,
            Err(_) => {
                assert_eq!("False state", "");
                SoldeerConfig::default()
            }
        };
        let _ = remove_file(target_config);
        assert!(sc.remappings_regenerate);
        assert!(sc.remappings_generate);
        Ok(())
    }

    #[tokio::test]
    async fn read_soldeer_configs_remappings_version() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config
[profile.default]
libs = ["dependencies"]
[dependencies]
"@gearbox-protocol-periphery-v3" = "1.1.1"
[soldeer]
remappings_version = true
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let sc = match read_soldeer_config(Some(target_config.clone())) {
            Ok(sc) => sc,
            Err(_) => {
                assert_eq!("False state", "");
                SoldeerConfig::default()
            }
        };
        let _ = remove_file(target_config);
        assert!(sc.remappings_version);
        assert!(sc.remappings_generate);
        Ok(())
    }

    #[tokio::test]
    async fn read_soldeer_configs_remappings_location() -> Result<()> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config
[profile.default]
libs = ["dependencies"]
[dependencies]
"@gearbox-protocol-periphery-v3" = "1.1.1"
[soldeer]
remappings_location = "config"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        let sc = match read_soldeer_config(Some(target_config.clone())) {
            Ok(sc) => sc,
            Err(_) => {
                assert_eq!("False state", "");
                SoldeerConfig::default()
            }
        };
        let _ = remove_file(target_config);
        assert_eq!(sc.remappings_location, RemappingsLocation::Config);
        assert!(sc.remappings_generate);
        Ok(())
    }

    ////////////// UTILS //////////////

    fn write_to_config(target_file: &PathBuf, content: &str) {
        if target_file.exists() {
            let _ = remove_file(target_file);
        }
        let mut file: std::fs::File =
            fs::OpenOptions::new().create_new(true).write(true).open(target_file).unwrap();
        if let Err(e) = write!(file, "{}", content) {
            eprintln!("Couldn't write to the config file: {}", e);
        }
    }

    fn define_config(foundry: bool) -> PathBuf {
        let s: String =
            rand::thread_rng().sample_iter(&Alphanumeric).take(7).map(char::from).collect();
        let mut target = format!("foundry{}.toml", s);
        if !foundry {
            target = format!("soldeer{}.toml", s);
        }

        get_current_working_dir().join("test").join(target)
    }
}
