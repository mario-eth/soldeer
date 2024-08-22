use crate::{
    download::{find_install_path, find_install_path_sync},
    errors::ConfigError,
    remappings::RemappingsLocation,
    utils::{get_url_type, run_git_command, UrlType},
    FOUNDRY_CONFIG_FILE, PROJECT_ROOT, SOLDEER_CONFIG_FILE,
};
use cliclack::{log::warning, select};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, remove_dir_all, remove_file},
    path::{Path, PathBuf},
};
use toml_edit::{value, DocumentMut, InlineTable, Item, Table};

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
            remappings_location: RemappingsLocation::default(),
            recursive_deps: false,
        }
    }
}

#[bon::builder]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GitDependency {
    pub name: String,
    #[serde(rename = "version")]
    pub version_req: String,
    pub git: String,
    pub rev: Option<String>,
}

impl GitDependency {
    pub fn install_path_sync(&self) -> Option<PathBuf> {
        find_install_path_sync(&self.into())
    }

    pub async fn install_path(&self) -> Option<PathBuf> {
        find_install_path(&self.into()).await
    }
}

impl core::fmt::Display for GitDependency {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version_req)
    }
}

#[bon::builder]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HttpDependency {
    pub name: String,
    #[serde(rename = "version")]
    pub version_req: String,
    pub url: Option<String>,
}

impl HttpDependency {
    pub fn install_path_sync(&self) -> Option<PathBuf> {
        find_install_path_sync(&self.into())
    }

    pub async fn install_path(&self) -> Option<PathBuf> {
        find_install_path(&self.into()).await
    }
}

impl core::fmt::Display for HttpDependency {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}~{}", self.name, self.version_req)
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
        let (dependency_name, dependency_version_req) = name_version
            .split_once('~')
            .expect("dependency string should have name and version requirement");
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
                        rev: rev.map(Into::into),
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
            Dependency::Http(dep) => &dep.name,
            Dependency::Git(dep) => &dep.name,
        }
    }

    pub fn version_req(&self) -> &str {
        match self {
            Dependency::Http(dep) => &dep.version_req,
            Dependency::Git(dep) => &dep.version_req,
        }
    }

    pub fn url(&self) -> Option<&String> {
        match self {
            Dependency::Http(dep) => dep.url.as_ref(),
            Dependency::Git(dep) => Some(&dep.git),
        }
    }

    pub fn install_path_sync(&self) -> Option<PathBuf> {
        match self {
            Dependency::Http(dep) => dep.install_path_sync(),
            Dependency::Git(dep) => dep.install_path_sync(),
        }
    }

    pub async fn install_path(&self) -> Option<PathBuf> {
        match self {
            Dependency::Http(dep) => dep.install_path().await,
            Dependency::Git(dep) => dep.install_path().await,
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
            Dependency::Git(dep) => (
                dep.name.clone(),
                match &dep.rev {
                    Some(rev) => {
                        let mut table = InlineTable::new();
                        table.insert(
                            "version",
                            value(&dep.version_req)
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
                            value(&dep.version_req)
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
            Dependency::Http(dep) => write!(f, "{dep}"),
            Dependency::Git(dep) => write!(f, "{dep}"),
        }
    }
}

impl From<HttpDependency> for Dependency {
    fn from(dep: HttpDependency) -> Self {
        Dependency::Http(dep)
    }
}

impl From<&HttpDependency> for Dependency {
    fn from(dep: &HttpDependency) -> Self {
        Dependency::Http(dep.clone())
    }
}

impl From<GitDependency> for Dependency {
    fn from(dep: GitDependency) -> Self {
        Dependency::Git(dep)
    }
}

impl From<&GitDependency> for Dependency {
    fn from(dep: &GitDependency) -> Self {
        Dependency::Git(dep.clone())
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
    if let Ok(file_path) = env::var("SOLDEER_CONFIG_FILE") {
        if !file_path.is_empty() {
            return Ok(file_path.into());
        }
    }

    let foundry_path: PathBuf = FOUNDRY_CONFIG_FILE.to_path_buf();
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

/// Read the list of dependencies from the config file.
///
/// If no config file path is provided, then the path is inferred automatically.
pub fn read_config_deps(path: Option<impl AsRef<Path>>) -> Result<Vec<Dependency>> {
    let path: PathBuf = match path {
        Some(p) => p.as_ref().to_path_buf(),
        None => get_config_path()?,
    };
    let contents = fs::read_to_string(&path)?;
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

pub fn read_soldeer_config(path: Option<impl AsRef<Path>>) -> Result<SoldeerConfig> {
    #[derive(Deserialize)]
    struct SoldeerConfigParsed {
        #[serde(default)]
        soldeer: SoldeerConfig,
    }

    let path: PathBuf = match path {
        Some(p) => p.as_ref().to_path_buf(),
        None => get_config_path()?,
    };
    let contents = fs::read_to_string(&path)?;

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

pub fn delete_config(dependency_name: &str, path: impl AsRef<Path>) -> Result<Dependency> {
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

pub async fn remove_forge_lib() -> Result<()> {
    let gitmodules_path = PROJECT_ROOT.join(".gitmodules");
    let lib_dir = PROJECT_ROOT.join("lib");
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
            // rev field is optional but needs to be a string if present
            let rev = match table.get("rev").map(|v| v.as_str()) {
                Some(Some(rev)) => Some(rev.to_string()),
                Some(None) => {
                    return Err(ConfigError::InvalidField { field: "rev".to_string(), dep: name });
                }
                None => None,
            };
            return Ok(GitDependency {
                name: name.to_string(),
                git: git.to_string(),
                version_req,
                rev,
            }
            .into());
        }
        None => {}
    }

    // we should have a HTTP dependency
    match table.get("url").map(|v| v.as_str()) {
        Some(None) => Err(ConfigError::InvalidField { field: "url".to_string(), dep: name }),
        None => Ok(HttpDependency { name: name.to_string(), version_req, url: None }.into()),
        Some(Some(url)) => {
            Ok(HttpDependency { name: name.to_string(), version_req, url: Some(url.to_string()) }
                .into())
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ConfigError;
    use std::{fs, path::PathBuf};
    use temp_env::with_var;
    use testdir::testdir;

    #[tokio::test]
    async fn read_foundry_config_deps() {
        let config_contents = r#"[profile.default]
libs = ["dependencies"]

[dependencies]
"lib1" = "1.0.0"
"lib2" = { version = "2.0.0" }
"lib3" = { version = "3.0.0", url = "https://example.com" }
"lib4" = { version = "4.0.0", git = "https://example.com/repo.git" }
"lib5" = { version = "5.0.0", git = "https://example.com/repo.git", rev = "123456" }
"#;
        let config_path = write_to_config(config_contents, "foundry.toml");
        let res = read_config_deps(Some(config_path));
        assert!(res.is_ok(), "{res:?}");
        let result = res.unwrap();

        assert_eq!(
            result[0],
            HttpDependency {
                name: "lib1".to_string(),
                version_req: "1.0.0".to_string(),
                url: None,
            }
            .into()
        );
        assert_eq!(
            result[1],
            HttpDependency {
                name: "lib2".to_string(),
                version_req: "2.0.0".to_string(),
                url: None,
            }
            .into()
        );
        assert_eq!(
            result[2],
            HttpDependency {
                name: "lib3".to_string(),
                version_req: "3.0.0".to_string(),
                url: Some("https://example.com".to_string()),
            }
            .into()
        );
        assert_eq!(
            result[4],
            GitDependency {
                name: "lib4".to_string(),
                version_req: "4.0.0".to_string(),
                git: "https://example.com/repo.git".to_string(),
                rev: None
            }
            .into()
        );
        assert_eq!(
            result[4],
            GitDependency {
                name: "lib4".to_string(),
                version_req: "5.0.0".to_string(),
                git: "https://example.com/repo.git".to_string(),
                rev: Some("123456".to_string())
            }
            .into()
        );
    }

    #[tokio::test]
    async fn read_soldeer_config_deps() {
        let config_contents = r#"[dependencies]
"lib1" = "1.0.0"
"lib2" = { version = "2.0.0" }
"lib3" = { version = "3.0.0", url = "https://example.com" }
"lib4" = { version = "4.0.0", git = "https://example.com/repo.git" }
"lib5" = { version = "5.0.0", git = "https://example.com/repo.git", rev = "123456" }
"#;
        let config_path = write_to_config(config_contents, "soldeer.toml");
        let res = read_config_deps(Some(config_path));
        assert!(res.is_ok(), "{res:?}");
        let result = res.unwrap();

        assert_eq!(
            result[0],
            HttpDependency {
                name: "lib1".to_string(),
                version_req: "1.0.0".to_string(),
                url: None,
            }
            .into()
        );
        assert_eq!(
            result[1],
            HttpDependency {
                name: "lib2".to_string(),
                version_req: "2.0.0".to_string(),
                url: None,
            }
            .into()
        );
        assert_eq!(
            result[2],
            HttpDependency {
                name: "lib3".to_string(),
                version_req: "3.0.0".to_string(),
                url: Some("https://example.com".to_string()),
            }
            .into()
        );
        assert_eq!(
            result[4],
            GitDependency {
                name: "lib4".to_string(),
                version_req: "4.0.0".to_string(),
                git: "https://example.com/repo.git".to_string(),
                rev: None
            }
            .into()
        );
        assert_eq!(
            result[4],
            GitDependency {
                name: "lib4".to_string(),
                version_req: "5.0.0".to_string(),
                git: "https://example.com/repo.git".to_string(),
                rev: Some("123456".to_string())
            }
            .into()
        );
    }

    #[tokio::test]
    async fn read_soldeer_config_deps_bad_version() {
        for dep in [
            r#""lib1" = """#,
            r#""lib1" = { version = "" }"#,
            r#""lib1" = { version = "", url = "https://example.com" }"#,
            r#""lib1" = { version = "", git = "https://example.com/repo.git" }"#,
            r#""lib1" = { version = "", git = "https://example.com/repo.git", rev = "123456" }"#,
        ] {
            let config_contents = format!("[dependencies]\n{}", dep);
            let config_path = write_to_config(&config_contents, "soldeer.toml");
            let res = read_config_deps(Some(config_path));
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
            let res = read_config_deps(Some(config_path));
            assert!(matches!(res, Err(ConfigError::InvalidVersionReq(_))), "{res:?}");
        }
    }

    #[test]
    fn config_path_soldeer() {
        let config_contents = "[dependencies]\n";
        let config_path = write_to_config(config_contents, "soldeer.toml");
        with_var(
            "SOLDEER_PROJECT_ROOT",
            Some(config_path.parent().unwrap().to_string_lossy().to_string()),
            || {
                let res = get_config_path();
                assert!(res.is_ok(), "{res:?}");
                assert_eq!(res.unwrap(), config_path);
            },
        );
    }

    /* // #[test] // TODO check how to do this properly
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
            assert_eq!(fs::read_to_string(&result).unwrap(), content);
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
            assert_eq!(fs::read_to_string(&result).unwrap(), content);
            Ok(())
        }

        #[test]
        fn create_new_file_if_not_defined_soldeer() -> Result<()> {
            let content = "
    [remappings]
    enabled = true

    [dependencies]
    ";

            let result = create_example_config(ConfigLocation::Soldeer).unwrap();

            assert!(PathBuf::from(&result).file_name().unwrap().to_string_lossy().contains("soldeer"));
            assert_eq!(fs::read_to_string(&result).unwrap(), content);
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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
                url: None,
            });

            add_to_config(&dependency, &target_config).unwrap();
            content = r#"
    [remappings]
    enabled = true

    [dependencies]
    dep1 = "1.0.0"
    "#;

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
                url: Some("http://custom_url.com/custom.zip".to_string()),
            });

            add_to_config(&dependency, &target_config).unwrap();
            content = r#"
    [remappings]
    enabled = true

    [dependencies]
    dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
    "#;

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
                version_req: "1.0.0".to_string(),
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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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

            assert_eq!(fs::read_to_string(&target_config).unwrap(), content);

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
        } */

    fn write_to_config(content: &str, filename: &str) -> PathBuf {
        let path = testdir!().join(filename);
        fs::write(&path, content).unwrap();
        path
    }
}
