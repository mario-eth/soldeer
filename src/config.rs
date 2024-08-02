use crate::{
    errors::ConfigError,
    remote::get_dependency_url_remote,
    utils::{get_current_working_dir, read_file_to_string, remove_empty_lines},
    FOUNDRY_CONFIG_FILE, SOLDEER_CONFIG_FILE,
};
use serde_derive::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, remove_dir_all, remove_file, File},
    io::{self, Write},
    path::{Path, PathBuf},
};
use toml_edit::{value, DocumentMut, Item, Table};
use yansi::Paint;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GitDependency {
    pub name: String,
    pub version: String,
    pub git: String,
    pub rev: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HttpDependency {
    pub name: String,
    pub version: String,
    pub url: Option<String>,
    pub checksum: Option<Vec<u8>>,
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
                        let mut table = Table::new();
                        table["version"] = value(&dep.version);
                        table["url"] = value(url);
                        Item::Table(table)
                    }
                    None => value(&dep.version),
                },
            ),
            Dependency::Git(dep) => (
                dep.name.clone(),
                match &dep.rev {
                    Some(rev) => {
                        let mut table = Table::new();
                        table["version"] = value(&dep.version);
                        table["git"] = value(&dep.git);
                        table["rev"] = value(rev);
                        Item::Table(table)
                    }
                    None => {
                        let mut table = Table::new();
                        table["version"] = value(&dep.version);
                        table["git"] = value(&dep.git);
                        Item::Table(table)
                    }
                },
            ),
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

pub async fn read_config(path: Option<PathBuf>) -> Result<Vec<Dependency>, ConfigError> {
    let path: PathBuf = match path {
        Some(p) => p,
        None => get_config_path()?,
    };
    let contents = read_file_to_string(&path);
    let doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");
    if !doc.contains_table("dependencies") {
        return Err(ConfigError {
            cause: format!("`[dependencies]` is missing from the config file {path:?}"),
        });
    }
    let Some(Some(data)) = doc.get("dependencies").map(|v| v.as_table()) else {
        return Err(ConfigError {
            cause: format!("`[dependencies]` is missing from the config file {path:?}"),
        });
    };

    let mut dependencies: Vec<Dependency> = Vec::new();
    for (name, v) in data {
        dependencies.push(parse_dependency(name, v).await?);
    }
    Ok(dependencies)
}

pub fn get_config_path() -> Result<PathBuf, ConfigError> {
    let foundry_path: PathBuf = if cfg!(test) {
        env::var("config_file").map(|s| s.into()).unwrap_or(FOUNDRY_CONFIG_FILE.clone())
    } else {
        FOUNDRY_CONFIG_FILE.clone()
    };

    if let Ok(contents) = fs::read_to_string(&foundry_path) {
        let doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");
        if doc.contains_table("dependencies") {
            return Ok(foundry_path);
        }
    }

    let soldeer_path = SOLDEER_CONFIG_FILE.clone();
    match fs::metadata(&soldeer_path) {
        Ok(_) => {
            return Ok(soldeer_path);
        }
        Err(_) => {
            println!("{}", Paint::blue("No config file found. If you wish to proceed, please select how you want Soldeer to be configured:\n1. Using foundry.toml\n2. Using soldeer.toml\n(Press 1 or 2), default is foundry.toml"));
            std::io::stdout().flush().unwrap();
            let mut option = String::new();
            if io::stdin().read_line(&mut option).is_err() {
                return Err(ConfigError { cause: "Option invalid.".to_string() });
            }

            if option.is_empty() {
                option = "1".to_string();
            }
            return create_example_config(&option);
        }
    }
}

pub fn add_to_config(
    dependency: &Dependency,
    config_path: impl AsRef<Path>,
) -> Result<(), ConfigError> {
    println!(
        "{}",
        Paint::green(&format!(
            "Adding dependency {}-{} to the config file",
            dependency.name(),
            dependency.version()
        ))
    );

    let contents = read_file_to_string(&config_path);
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    // in case we don't have the dependencies section defined in the config file, we add it
    if !doc.contains_table("dependencies") {
        doc.insert("dependencies", Item::Table(Table::default()));
    }

    let (name, value) = dependency.to_toml_value();
    doc["dependencies"]
        .as_table_mut()
        .expect("dependencies should be a table")
        .insert(&name, value);

    fs::write(config_path, doc.to_string())
        .map_err(|e| ConfigError { cause: format!("Couldn't write to the config file: {e:?}") })?;

    Ok(())
}

pub async fn remappings() -> Result<(), ConfigError> {
    let remappings_path = get_current_working_dir().join("remappings.txt");
    if !remappings_path.exists() {
        File::create(remappings_path.clone()).unwrap();
    }
    let contents = read_file_to_string(&remappings_path);

    let existing_remappings: Vec<String> = contents.split('\n').map(|s| s.to_string()).collect();
    let mut new_remappings: String = String::new();

    let dependencies: Vec<Dependency> = match read_config(None).await {
        Ok(dep) => dep,
        Err(err) => {
            return Err(err);
        }
    };

    let mut existing_remap: Vec<String> = Vec::new();
    existing_remappings.iter().for_each(|remapping| {
        let split: Vec<&str> = remapping.split('=').collect::<Vec<&str>>();
        if split.len() == 1 {
            // skip empty lines
            return;
        }
        existing_remap.push(String::from(split[0]));
    });

    dependencies.iter().for_each(|dependency| {
        let mut dependency_name_formatted =
            format!("{}-{}", &dependency.name(), &dependency.version());
        if !dependency_name_formatted.contains('@') {
            dependency_name_formatted = format!("@{}", dependency_name_formatted);
        }
        let index = existing_remap.iter().position(|r| r == &dependency_name_formatted);
        if index.is_none() {
            println!(
                "{}",
                Paint::green(&format!(
                    "Added a new dependency to remappings {}",
                    &dependency_name_formatted
                ))
            );
            new_remappings.push_str(&format!(
                "\n{}=dependencies/{}-{}",
                &dependency_name_formatted,
                &dependency.name(),
                &dependency.version()
            ));
        }
    });

    if new_remappings.is_empty() {
        remove_empty_lines("remappings.txt");
        return Ok(());
    }

    let mut file: std::fs::File =
        fs::OpenOptions::new().append(true).open(Path::new("remappings.txt")).unwrap();

    match write!(file, "{}", &new_remappings) {
        Ok(_) => {}
        Err(_) => {
            println!("{}", Paint::yellow(&"Could not write to the remappings file".to_string()));
        }
    }
    remove_empty_lines("remappings.txt");
    Ok(())
}

pub fn delete_config(
    dependency_name: &str,
    path: impl AsRef<Path>,
) -> Result<Dependency, ConfigError> {
    println!(
        "{}",
        Paint::green(&format!("Removing the dependency {dependency_name} from the config file"))
    );

    let contents = read_file_to_string(&path);
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    if !doc.contains_table("dependencies") {
        return Err(ConfigError {
            cause: format!("`[dependencies]` is missing from the config file {:?}", path.as_ref()),
        });
    }

    let Some(item_removed) = doc["dependencies"].as_table_mut().unwrap().remove(dependency_name)
    else {
        return Err(ConfigError {
            cause: format!("The dependency {dependency_name} does not exists in the config file"),
        });
    };

    let dependency = parse_dependency_sync(dependency_name, &item_removed)?;

    fs::write(path, doc.to_string())
        .map_err(|e| ConfigError { cause: format!("Couldn't write to the config file: {e:?}") })?;

    Ok(dependency)
}

pub fn remove_forge_lib() -> Result<(), ConfigError> {
    let lib_dir = get_current_working_dir().join("lib/");
    let gitmodules_file = get_current_working_dir().join(".gitmodules");

    let _ = remove_file(gitmodules_file);
    let _ = remove_dir_all(lib_dir);
    Ok(())
}

/// This function parses the TOML config item into a Dependency object but doesn't retrieve the URL
/// in case of an HTTP dependency with only a version string.
fn parse_dependency_sync(name: impl Into<String>, value: &Item) -> Result<Dependency, ConfigError> {
    let name: String = name.into();
    if let Some(version) = value.as_str() {
        // this function does not retrieve the url
        return Ok(Dependency::Http(HttpDependency {
            name,
            version: version.to_string(),
            url: None,
            checksum: None,
        }));
    }

    // we should have a table
    let Some(table) = value.as_table() else {
        return Err(ConfigError { cause: format!("Config for {name} is invalid") });
    };

    // version is needed in both cases
    let version = match table.get("version").map(|v| v.as_str()) {
        Some(None) => {
            return Err(ConfigError {
                cause: format!("Field `version` for dependency {name} is invalid"),
            });
        }
        None => {
            return Err(ConfigError {
                cause: format!("Field `version` for dependency {name} is missing"),
            });
        }
        Some(Some(version)) => version.to_string(),
    };

    // check if it's a git dependency
    match table.get("git").map(|v| v.as_str()) {
        Some(None) => {
            return Err(ConfigError {
                cause: format!("Field `git` for dependency {name} is invalid"),
            });
        }
        Some(Some(git)) => {
            // rev field is optional but needs to be a string if present
            let rev = match table.get("rev").map(|v| v.as_str()) {
                Some(Some(rev)) => Some(rev.to_string()),
                Some(None) => {
                    return Err(ConfigError {
                        cause: format!("Field `rev` for dependency {name} is invalid"),
                    });
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
        Some(None) => {
            return Err(ConfigError {
                cause: format!("Field `url` for dependency {name} is invalid"),
            });
        }
        None => {
            return Err(ConfigError {
                cause: format!("Field `url` for dependency {name} is missing"),
            });
        }
        Some(Some(url)) => Ok(Dependency::Http(HttpDependency {
            name: name.to_string(),
            version,
            url: Some(url.to_string()),
            checksum: None,
        })),
    }
}

async fn parse_dependency(
    name: impl Into<String>,
    value: &Item,
) -> Result<Dependency, ConfigError> {
    match parse_dependency_sync(name, value)? {
        Dependency::Http(mut dep) => {
            let url = get_dependency_url_remote(&dep.name, &dep.version).await.map_err(|_| {
                ConfigError { cause: format!("Could not retrieve URL for dependency {}", dep.name) }
            })?;
            dep.url = Some(url);
            Ok(Dependency::Http(dep))
        }
        dep => Ok(dep),
    }
}

fn create_example_config(option: &str) -> Result<PathBuf, ConfigError> {
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
            return Err(ConfigError { cause: "Invalid option".to_string() });
        }
    };

    fs::write(&config_path, contents)
        .map_err(|e| ConfigError { cause: format!("Could not create a new config file: {e:?}") })?;
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
        env,
        fs::{
            remove_file, {self},
        },
        io::Write,
        path::PathBuf,
    };

    #[tokio::test] // check dependencies as {version = "1.1.1"}
    #[serial]
    async fn read_foundry_config_version_v1_ok() -> Result<(), ConfigError> {
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

        ////////////// MOCK //////////////
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is
        // dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock("GET", mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(Some(target_config.clone())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test] // check dependencies as "1.1.1"
    #[serial]
    async fn read_foundry_config_version_v2_ok() -> Result<(), ConfigError> {
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

        ////////////// MOCK //////////////
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is
        // dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock("GET", mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(Some(target_config.clone())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test] // check dependencies as "1.1.1"
    #[serial]
    async fn read_soldeer_config_version_v1_ok() -> Result<(), ConfigError> {
        let config_contents = r#"
[remappings]
enabled = true

[dependencies]
"@gearbox-protocol-periphery-v3" = "1.6.1"
"@openzeppelin-contracts" = "5.0.2"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        ////////////// MOCK //////////////
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is
        // dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock("GET", mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(Some(target_config.clone())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test] // check dependencies as "1.1.1"
    #[serial]
    async fn read_soldeer_config_version_v2_ok() -> Result<(), ConfigError> {
        let config_contents = r#"
[remappings]
enabled = true

[dependencies]
"@gearbox-protocol-periphery-v3" = "1.6.1"
"@openzeppelin-contracts" = "5.0.2"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        ////////////// MOCK //////////////
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is
        // dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock("GET", mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(Some(target_config.clone())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency::Http(HttpDependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );

        assert_eq!(
            result[1],
            Dependency::Http(HttpDependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: Some("https://example_url.com/example_url.zip".to_string()),
                checksum: None
            })
        );
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn read_malformed_config_incorrect_version_string_fails() -> Result<(), ConfigError> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"@gearbox-protocol-periphery-v3" = 1.6.1"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        match read_config(Some(target_config.clone())).await {
            Ok(_) => {
                assert_eq!("False state", "");
            }
            Err(err) => {
                assert_eq!(
                    err,
                    ConfigError {
                        cause: format!(
                            "Could not read the config file {}",
                            target_config.to_str().unwrap()
                        ),
                    }
                )
            }
        };
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn read_malformed_config_empty_version_string_fails() -> Result<(), ConfigError> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"@gearbox-protocol-periphery-v3" = ""
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        match read_config(Some(target_config.clone())).await {
            Ok(_) => {
                assert_eq!("False state", "");
            }
            Err(err) => {
                assert_eq!(
                    err,
                    ConfigError {
                        cause: "Could not get the config correctly from the config file"
                            .to_string(),
                    }
                )
            }
        };
        let _ = remove_file(target_config);
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn read_dependency_url_call_fails() -> Result<(), ConfigError> {
        let config_contents = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
libs = ["dependencies"]

[dependencies]
"@gearbox-protocol-periphery-v3" = "1.1.1"
"#;
        let target_config = define_config(false);

        write_to_config(&target_config, config_contents);

        match read_config(Some(target_config.clone())).await {
            Ok(_) => {
                assert_eq!("False state", "");
            }
            Err(err) => {
                assert_eq!(err, ConfigError { cause: "Could not get the url".to_string() })
            }
        };
        let _ = remove_file(target_config);

        Ok(())
    }

    #[test]
    fn define_config_file_choses_foundry() -> Result<(), ConfigError> {
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
    async fn define_config_file_choses_soldeer() -> Result<(), ConfigError> {
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
    fn create_new_file_if_not_defined_foundry() -> Result<(), ConfigError> {
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
    fn create_new_file_if_not_defined_soldeer() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_no_custom_url_first_dependency() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_with_custom_url_first_dependency() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_no_custom_url_second_dependency() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_with_custom_url_second_dependency() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_update_dependency_version() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_update_dependency_version_no_custom_url() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_not_altering_the_existing_contents() -> Result<(), ConfigError> {
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

# we don't have [dependencies] declared

[dependencies]
dep1 = "1.0.0"
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_soldeer_no_custom_url_first_dependency() -> Result<(), ConfigError> {
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
    fn add_to_config_soldeer_with_custom_url_first_dependency() -> Result<(), ConfigError> {
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
    fn add_to_config_foundry_github_with_commit() -> Result<(), ConfigError> {
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

# we don't have [dependencies] declared

[dependencies]
dep1 = { version = "1.0.0", git = "git@github.com:foundry-rs/forge-std.git", rev = "07263d193d621c4b2b0ce8b4d54af58f6957d97d" }
"#;

        assert_eq!(read_file_to_string(&target_config), content);

        let _ = remove_file(target_config);
        Ok(())
    }

    #[test]
    fn add_to_config_foundry_github_previous_no_commit_then_with_commit() -> Result<(), ConfigError>
    {
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
    fn add_to_config_foundry_github_previous_commit_then_no_commit() -> Result<(), ConfigError> {
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
    fn remove_from_the_config_single() -> Result<(), ConfigError> {
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
    fn remove_from_the_config_multiple() -> Result<(), ConfigError> {
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
    fn remove_config_nonexistent_fails() -> Result<(), ConfigError> {
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

        match delete_config(&"dep2".to_string(), target_config.to_str().unwrap()) {
            Ok(_) => {
                assert_eq!("Invalid State", "");
            }
            Err(err) => {
                assert_eq!(
                    err,
                    ConfigError {
                        cause: "The dependency dep2 does not exists in the config file".to_string()
                    }
                )
            }
        }

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
