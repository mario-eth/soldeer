use crate::{
    errors::ConfigError,
    utils::{get_current_working_dir, read_file_to_string},
    FOUNDRY_CONFIG_FILE, SOLDEER_CONFIG_FILE,
};
use serde_derive::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, remove_dir_all, remove_file, File},
    io::{self, Write},
    path::{Path, PathBuf},
};
use toml_edit::{value, DocumentMut, InlineTable, Item, Table};
use yansi::Paint as _;

pub type Result<T> = std::result::Result<T, ConfigError>;

/// Location where to store the remappings, either in `remappings.txt` or the config file
/// (foundry/soldeer)
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RemappingsLocation {
    #[default]
    Txt,
    Config,
}

/// The Soldeer config options
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SoldeerConfig {
    pub remappings_generate: bool,
    pub remappings_regenerate: bool,
    pub remappings_version: bool,
    pub remappings_prefix: String,
    pub remappings_location: RemappingsLocation,
}

impl Default for SoldeerConfig {
    fn default() -> Self {
        SoldeerConfig {
            remappings_generate: true,
            remappings_regenerate: false,
            remappings_version: false,
            remappings_prefix: String::new(),
            remappings_location: Default::default(),
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
    pub checksum: Option<String>,
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

    pub fn as_http(&self) -> Option<&HttpDependency> {
        if let Self::Http(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_git(&self) -> Option<&GitDependency> {
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

pub fn get_config_path() -> Result<PathBuf> {
    let foundry_path: PathBuf = if cfg!(test) {
        env::var("config_file").map(|s| s.into()).unwrap_or(FOUNDRY_CONFIG_FILE.clone())
    } else {
        FOUNDRY_CONFIG_FILE.clone()
    };

    if let Ok(contents) = fs::read_to_string(&foundry_path) {
        let doc: DocumentMut = contents.parse::<DocumentMut>()?;
        if doc.contains_table("dependencies") {
            return Ok(foundry_path);
        }
    }

    let soldeer_path = SOLDEER_CONFIG_FILE.clone();
    match fs::metadata(&soldeer_path) {
        Ok(_) => Ok(soldeer_path),
        Err(_) => {
            println!("{}", "No config file found. If you wish to proceed, please select how you want Soldeer to be configured:\n1. Using foundry.toml\n2. Using soldeer.toml\n(Press 1 or 2), default is foundry.toml".blue());
            std::io::stdout().flush().unwrap();
            let mut option = String::new();
            io::stdin()
                .read_line(&mut option)
                .map_err(|e| ConfigError::PromptError { source: e })?;

            if option.is_empty() {
                option = "1".to_string();
            }
            create_example_config(&option)
        }
    }
}

pub fn read_config_deps(path: Option<PathBuf>) -> Result<Vec<Dependency>> {
    let path: PathBuf = match path {
        Some(p) => p,
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
    Ok(dependencies)
}

pub fn read_soldeer_config(path: Option<PathBuf>) -> Result<SoldeerConfig> {
    let path: PathBuf = match path {
        Some(p) => p,
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
    println!(
        "{}",
        format!(
            "Adding dependency {}-{} to the config file",
            dependency.name(),
            dependency.version()
        )
        .green()
    );

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

fn generate_remappings(
    add_dependency: Option<&Dependency>,
    soldeer_config: &SoldeerConfig,
    existing_remappings: Vec<(&str, &str)>,
) -> Result<Vec<String>> {
    let mut new_remappings = Vec::new();
    if soldeer_config.remappings_regenerate {
        let dependencies = read_config_deps(None)?;

        dependencies.iter().for_each(|dependency| {
            let dependency_name_formatted = format_remap_name(soldeer_config, dependency);

            println!("{}", format!("Adding {dependency} to remappings").green());
            new_remappings.push(format!(
                "{dependency_name_formatted}=dependencies/{}-{}/",
                dependency.name(),
                dependency.version()
            ));
        });
    } else if let Some(add_dep) = add_dependency {
        // we only add the remapping if it's not already existing, otherwise we keep the old
        // remapping
        let new_dep_remapped = format_remap_name(soldeer_config, add_dep);
        let new_dep_orig = format!("dependencies/{}-{}/", add_dep.name(), add_dep.version());
        for (remapped, orig) in existing_remappings {
            if orig == new_dep_orig {
                new_remappings.push(format!("{}={}", remapped, orig));
            } else {
                new_remappings.push(format!("{}={}", new_dep_remapped, new_dep_orig));
                println!("{}", format!("Added {add_dep} to remappings").green());
            }
        }
    } else {
        for (remapped, orig) in existing_remappings {
            new_remappings.push(format!("{}={}", remapped, orig));
        }
    }
    // sort the remappings
    new_remappings.sort_unstable();
    Ok(new_remappings)
}

pub async fn remappings_txt(
    add_dependency: Option<&Dependency>,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    let remappings_path = get_current_working_dir().join("remappings.txt");
    if soldeer_config.remappings_regenerate {
        remove_file(&remappings_path).map_err(ConfigError::RemappingsError)?;
    }
    if !remappings_path.exists() {
        File::create(remappings_path.clone()).unwrap();
    }

    let new_remappings = match add_dependency {
        Some(_) => {
            let contents = read_file_to_string(&remappings_path);
            let existing_remappings = contents.lines().filter_map(|r| r.split_once('=')).collect();
            generate_remappings(add_dependency, soldeer_config, existing_remappings)?
        }
        None => generate_remappings(add_dependency, soldeer_config, vec![])?,
    };

    let mut file = File::create(remappings_path)?;
    for remapping in new_remappings {
        writeln!(file, "{}", remapping)?;
    }
    Ok(())
}

pub async fn remappings_foundry(
    add_dependency: Option<&Dependency>,
    config_path: impl AsRef<Path>,
    soldeer_config: &SoldeerConfig,
) -> Result<()> {
    let contents = read_file_to_string(&config_path);
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    let Some(profiles) = doc["profile"].as_table_mut() else {
        // we don't add remappings if there are no profiles
        return Ok(());
    };

    for (_, profile) in profiles.iter_mut() {
        let Some(Some(remappings)) = profile.get_mut("remappings").map(|v| v.as_array_mut()) else {
            continue;
        };
        let existing_remappings: Vec<_> = remappings
            .iter()
            .filter_map(|r| r.as_str())
            .filter_map(|r| r.split_once('='))
            .collect();
        let new_remappings =
            generate_remappings(add_dependency, soldeer_config, existing_remappings)?;
        remappings.clear();
        for remapping in new_remappings {
            remappings.push(remapping);
        }
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

pub fn remove_forge_lib() -> Result<()> {
    let lib_dir = get_current_working_dir().join("lib/");
    let gitmodules_file = get_current_working_dir().join(".gitmodules");

    let _ = remove_file(gitmodules_file);
    let _ = remove_dir_all(lib_dir);
    Ok(())
}

fn parse_dependency(name: impl Into<String>, value: &Item) -> Result<Dependency> {
    let name: String = name.into();
    if let Some(version) = value.as_str() {
        if version.is_empty() {
            return Err(ConfigError::EmptyVersion(name));
        }
        // this function does not retrieve the url
        return Ok(
            HttpDependency { name, version: version.to_string(), url: None, checksum: None }.into()
        );
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
        None => Err(ConfigError::MissingField { field: "url".to_string(), dep: name }),
        Some(Some(url)) => Ok(Dependency::Http(HttpDependency {
            name: name.to_string(),
            version,
            url: Some(url.to_string()),
            checksum: None,
        })),
    }
}

fn format_remap_name(soldeer_config: &SoldeerConfig, dependency: &Dependency) -> String {
    let version_suffix =
        if soldeer_config.remappings_version { &format!("-{}", dependency.version()) } else { "" };
    format!("{}{}{}/", soldeer_config.remappings_prefix, dependency.name(), version_suffix)
}

fn create_example_config(option: &str) -> Result<PathBuf> {
    let (config_path, contents) = match option.trim() {
        "1" => (
            FOUNDRY_CONFIG_FILE.clone(),
            r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
"#,
        ),
        "2" => (
            SOLDEER_CONFIG_FILE.clone(),
            r#"
[remappings]
enabled = true

[dependencies]
"#,
        ),
        _ => {
            return Err(ConfigError::InvalidPromptOption);
        }
    };

    fs::write(&config_path, contents)?;
    Ok(config_path)
}

////////////// TESTS //////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Dependency, errors::ConfigError, utils::get_current_working_dir};
    use rand::{distributions::Alphanumeric, Rng};
    use serial_test::serial;
    use std::{
        fs::{
            remove_file, {self},
        },
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
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
                checksum: None
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
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
                checksum: None
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
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
                checksum: None
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
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: None,
                checksum: None
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

    #[test]
    fn create_new_file_if_not_defined_foundry() -> Result<()> {
        let content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
script = "script"
solc = "0.8.26"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
"#;

        let result = create_example_config("1").unwrap();

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

        let result = create_example_config("2").unwrap();

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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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
            checksum: None,
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

    #[allow(unused)]
    fn get_return_data() -> String {
        r#"
        {
            "data": [
                {
                    "created_at": "2024-03-14T06:11:59.838552Z",
                    "deleted": false,
                    "downloads": 100,
                    "id": "c10d3ec8-7968-468f-bc12-8188bcafce2b",
                    "internal_name": "example_url.zip",
                    "project_id": "bbf2a8e4-2572-4787-bff9-216db013691b",
                    "url": "https://example_url.com/example_url.zip",
                    "version": "5.0.2"
                }
            ],
            "status": "success"
        }
        "#
        .to_string()
    }
}
