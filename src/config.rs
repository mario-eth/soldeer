use crate::errors::ConfigError;
use crate::remote::get_dependency_url_remote;
use crate::utils::{
    get_current_working_dir,
    read_file_to_string,
    remove_empty_lines,
};
use crate::{
    FOUNDRY_CONFIG_FILE,
    SOLDEER_CONFIG_FILE,
};
use serde_derive::Deserialize;
use std::fs::{
    self,
    File,
};
use std::io::Write;
use std::path::Path;
use std::{
    env,
    io,
};
use toml::Table;
use toml_edit::{
    value,
    DocumentMut,
    Item,
};
use yansi::Paint;

// Top level struct to hold the TOML data.
#[derive(Deserialize, Debug)]
struct Data {
    dependencies: Table,
}

// Dependency object used to store a dependency data
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub url: String,
    pub hash: String,
}

#[derive(Deserialize, Debug)]
struct Foundry {
    remappings: Table,
}

pub async fn read_config(filename: String) -> Result<Vec<Dependency>, ConfigError> {
    let mut filename: String = filename;
    if filename.is_empty() {
        filename = match define_config_file() {
            Ok(file) => file,
            Err(err) => return Err(err),
        }
    }
    let contents = read_file_to_string(&filename.clone());

    // reading the contents into a data structure using toml::from_str
    let data: Data = match toml::from_str(&contents) {
        Ok(d) => d,
        Err(_) => {
            return Err(ConfigError {
                cause: format!("Could not read the config file {}", filename),
            });
        }
    };

    let mut dependencies: Vec<Dependency> = Vec::new();
    let iterator = data.dependencies.iter();
    for (name, v) in iterator {
        #[allow(clippy::needless_late_init)]
        let url;
        let version;
        let mut rev = String::new();

        // checks if the format is dependency = {version = "1.1.1" }
        if v.get("version").is_some() {
            // clear any string quotes added by mistake
            version = v["version"].to_string().replace('"', "");
        } else {
            // checks if the format is dependency = "1.1.1"
            version = String::from(v.as_str().unwrap());
            if version.is_empty() {
                return Err(ConfigError {
                    cause: "Could not get the config correctly from the config file".to_string(),
                });
            }
        }

        if v.get("url").is_some() {
            // clear any string quotes added by mistake
            url = v["url"].to_string().replace('\"', "");
        } else if v.get("git").is_some() {
            url = v["git"].to_string().replace('\"', "");
            if v.get("rev").is_some() {
                rev = v["rev"].to_string().replace('\"', "");
            }
        } else {
            // we don't have a specified url, means we will rely on the remote server to give it to us
            url = match get_dependency_url_remote(name, &version).await {
                Ok(u) => u,
                Err(_) => {
                    return Err(ConfigError {
                        cause: "Could not get the url".to_string(),
                    });
                }
            }
        }

        dependencies.push(Dependency {
            name: name.to_string(),
            version,
            url,
            hash: rev,
        });
    }

    Ok(dependencies)
}

pub fn define_config_file() -> Result<String, ConfigError> {
    let mut filename: String;
    if cfg!(test) {
        filename =
            env::var("config_file").unwrap_or(String::from(FOUNDRY_CONFIG_FILE.to_str().unwrap()))
    } else {
        filename = String::from(FOUNDRY_CONFIG_FILE.to_str().unwrap());
    };

    // check if the foundry.toml has the dependencies defined, if so then we setup the foundry.toml as the config file
    if fs::metadata(&filename).is_ok() {
        return Ok(filename);
    }

    filename = String::from(SOLDEER_CONFIG_FILE.to_str().unwrap());
    match fs::metadata(&filename) {
        Ok(_) => {}
        Err(_) => {
            println!("{}", Paint::blue("No config file found. If you wish to proceed, please select how you want Soldeer to be configured:\n1. Using foundry.toml\n2. Using soldeer.toml\n(Press 1 or 2), default is foundry.toml"));
            std::io::stdout().flush().unwrap();
            let mut option = String::new();
            if io::stdin().read_line(&mut option).is_err() {
                return Err(ConfigError {
                    cause: "Option invalid.".to_string(),
                });
            }

            if option.is_empty() {
                option = "1".to_string();
            }
            return create_example_config(&option);
        }
    }

    Ok(filename)
}

pub fn add_to_config(
    dependency: &Dependency,
    custom_url: bool,
    config_file: &str,
    via_git: bool,
) -> Result<(), ConfigError> {
    println!(
        "{}",
        Paint::green(&format!(
            "Adding dependency {}-{} to the config file",
            dependency.name, dependency.version
        ))
    );

    let contents = read_file_to_string(&String::from(config_file));
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    // in case we don't have dependencies defined in the config file, we add it and re-read the doc
    if !doc.contains_table("dependencies") {
        let mut file: std::fs::File = fs::OpenOptions::new()
            .append(true)
            .open(config_file)
            .unwrap();
        if let Err(e) = write!(file, "{}", String::from("\n[dependencies]\n")) {
            eprintln!("Couldn't write to the config file: {}", e);
        }

        doc = read_file_to_string(&String::from(config_file))
            .parse::<DocumentMut>()
            .expect("invalid doc");
    }
    let mut new_dependencies: String = String::new();

    new_dependencies.push_str(&format!(
        "  \"{}~{}\" = \"{}\"\n",
        dependency.name, dependency.version, dependency.url
    ));

    let mut new_item: Item = Item::None;
    if custom_url && !via_git {
        new_item["version"] = value(dependency.version.clone());
        new_item["url"] = value(dependency.url.clone());
    } else if via_git {
        new_item["version"] = value(dependency.version.clone());
        new_item["git"] = value(dependency.url.clone());
        new_item["rev"] = value(dependency.hash.clone());
    } else {
        new_item = value(dependency.version.clone())
    }

    doc["dependencies"]
        .as_table_mut()
        .unwrap()
        .insert(dependency.name.to_string().as_str(), new_item);
    let mut file: std::fs::File = fs::OpenOptions::new()
        .write(true)
        .append(false)
        .truncate(true)
        .open(config_file)
        .unwrap();
    if let Err(e) = write!(file, "{}", doc) {
        eprintln!("Couldn't write to the config file: {}", e);
    }
    Ok(())
}

pub async fn remappings() -> Result<(), ConfigError> {
    let remappings_path = get_current_working_dir().join("remappings.txt");
    if !remappings_path.exists() {
        File::create(remappings_path.clone()).unwrap();
    }
    let contents = read_file_to_string(&remappings_path.to_str().unwrap().to_string());

    let existing_remappings: Vec<String> = contents.split('\n').map(|s| s.to_string()).collect();
    let mut new_remappings: String = String::new();

    let dependencies: Vec<Dependency> = match read_config(String::new()).await {
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
        let mut dependency_name_formatted = format!("{}-{}", &dependency.name, &dependency.version);
        if !dependency_name_formatted.contains('@') {
            dependency_name_formatted = format!("@{}", dependency_name_formatted);
        }
        let index = existing_remap
            .iter()
            .position(|r| r == &dependency_name_formatted);
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
                &dependency_name_formatted, &dependency.name, &dependency.version
            ));
        }
    });

    if new_remappings.is_empty() {
        remove_empty_lines("remappings.txt");
        return Ok(());
    }

    let mut file: std::fs::File = fs::OpenOptions::new()
        .append(true)
        .open(Path::new("remappings.txt"))
        .unwrap();

    match write!(file, "{}", &new_remappings) {
        Ok(_) => {}
        Err(_) => {
            println!(
                "{}",
                Paint::yellow(&"Could not write to the remappings file".to_string())
            );
        }
    }
    remove_empty_lines("remappings.txt");
    Ok(())
}

pub fn get_foundry_setup() -> Result<Vec<bool>, ConfigError> {
    let filename = match define_config_file() {
        Ok(file) => file,
        Err(err) => {
            return Err(err);
        }
    };
    if filename.contains("foundry.toml") {
        return Ok(vec![true]);
    }
    let contents: String = read_file_to_string(&filename.clone());

    // reading the contents into a data structure using toml::from_str
    let data: Foundry = match toml::from_str(&contents) {
        Ok(d) => d,
        Err(_) => {
            println!(
                "{}",
                Paint::yellow(&"The remappings field not found in the soldeer.toml and no foundry config file found or the foundry.toml does not contain the `[dependencies]` field.\nThe foundry.toml file should contain the `[dependencies]` field if you want to use it as a config file. If you want to use the soldeer.toml file, please add the `[remappings]` field to it with the `enabled` key set to `true` or `false`.\nMore info on https://github.com/mario-eth/soldeer\nThe installation was successful but the remappings feature was skipped.".to_string())
            );
            return Ok(vec![false]);
        }
    };
    if data.remappings.get("enabled").is_none() {
        println!(
            "{}",
            Paint::yellow(&"The remappings field not found in the soldeer.toml and no foundry config file found or the foundry.toml does not contain the `[dependencies]` field.\nThe foundry.toml file should contain the `[dependencies]` field if you want to use it as a config file. If you want to use the soldeer.toml file, please add the `[remappings]` field to it with the `enabled` key set to `true` or `false`.\nMore info on https://github.com/mario-eth/soldeer\nThe installation was successful but the remappings feature was skipped.".to_string())
        );
        return Ok(vec![false]);
    }
    Ok(vec![data
        .remappings
        .get("enabled")
        .unwrap()
        .as_bool()
        .unwrap()])
}

fn create_example_config(option: &str) -> Result<String, ConfigError> {
    let config_file: &str;
    let content: &str;
    if option.trim() == "1" {
        config_file = FOUNDRY_CONFIG_FILE.to_str().unwrap();
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
    } else if option.trim() == "2" {
        config_file = SOLDEER_CONFIG_FILE.to_str().unwrap();
        content = r#"
[remappings]
enabled = true

[dependencies]
"#;
    } else {
        return Err(ConfigError {
            cause: "Option invalid".to_string(),
        });
    }

    std::fs::File::create(config_file).unwrap();
    let mut file: std::fs::File = fs::OpenOptions::new()
        .write(true)
        .open(config_file)
        .unwrap();
    if write!(file, "{}", content).is_err() {
        return Err(ConfigError {
            cause: "Could not create a new config file".to_string(),
        });
    }
    let mut filename = String::from(FOUNDRY_CONFIG_FILE.to_str().unwrap());
    if option.trim() == "2" {
        filename = String::from(SOLDEER_CONFIG_FILE.to_str().unwrap());
    }
    Ok(filename)
}

////////////// TESTS //////////////

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs::remove_file;
    use std::io::Write;
    use std::{
        fs::{
            self,
        },
        path::PathBuf,
    };

    use crate::config::Dependency;
    use crate::errors::ConfigError;
    use crate::utils::get_current_working_dir;
    use rand::{
        distributions::Alphanumeric,
        Rng,
    };
    use serial_test::serial; // 0.8

    use super::*;

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
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()),
            )
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(String::from(target_config.to_str().unwrap())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
        );

        assert_eq!(
            result[1],
            Dependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
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
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()),
            )
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(String::from(target_config.to_str().unwrap())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
        );

        assert_eq!(
            result[1],
            Dependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
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
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()),
            )
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(String::from(target_config.to_str().unwrap())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
        );

        assert_eq!(
            result[1],
            Dependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
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
        // Request a new server from the pool, TODO i tried to move this into a fn but the mock is dropped at the end of the function...
        let mut server = mockito::Server::new_async().await;
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        let _ = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/api/v1/revision-cli.*".to_string()),
            )
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(get_return_data())
            .create();

        ////////////// END-MOCK //////////////

        let result = match read_config(String::from(target_config.to_str().unwrap())).await {
            Ok(dep) => dep,
            Err(err) => {
                return Err(err);
            }
        };

        assert_eq!(
            result[0],
            Dependency {
                name: "@gearbox-protocol-periphery-v3".to_string(),
                version: "1.6.1".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
        );

        assert_eq!(
            result[1],
            Dependency {
                name: "@openzeppelin-contracts".to_string(),
                version: "5.0.2".to_string(),
                url: "https://example_url.com/example_url.zip".to_string(),
                hash: String::new()
            }
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

        match read_config(String::from(target_config.clone().to_str().unwrap())).await {
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

        match read_config(String::from(target_config.clone().to_str().unwrap())).await {
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

        match read_config(String::from(target_config.clone().to_str().unwrap())).await {
            Ok(_) => {
                assert_eq!("False state", "");
            }
            Err(err) => {
                assert_eq!(
                    err,
                    ConfigError {
                        cause: "Could not get the url".to_string(),
                    }
                )
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

        assert!(target_config
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("foundry"));
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

        assert!(target_config
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("soldeer"));
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

        assert!(PathBuf::from(&result)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("foundry"));
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

        assert!(PathBuf::from(&result)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("soldeer"));
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
        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };
        add_to_config(&dependency, false, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, true, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, false, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, true, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "old_dep".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, true, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "old_dep".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, false, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, false, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, false, target_config.to_str().unwrap(), false).unwrap();
        content = r#"
[remappings]
enabled = true

[dependencies]
dep1 = "1.0.0"
"#;

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, true, target_config.to_str().unwrap(), false).unwrap();
        content = r#"
[remappings]
enabled = true

[dependencies]
dep1 = { version = "1.0.0", url = "http://custom_url.com/custom.zip" }
"#;

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "git@github.com:foundry-rs/forge-std.git".to_string(),
            hash: "07263d193d621c4b2b0ce8b4d54af58f6957d97d".to_string(),
        };

        add_to_config(&dependency, true, target_config.to_str().unwrap(), true).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "git@github.com:foundry-rs/forge-std.git".to_string(),
            hash: "07263d193d621c4b2b0ce8b4d54af58f6957d97d".to_string(),
        };

        add_to_config(&dependency, true, target_config.to_str().unwrap(), true).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

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

        let dependency = Dependency {
            name: "dep1".to_string(),
            version: "1.0.0".to_string(),
            url: "http://custom_url.com/custom.zip".to_string(),
            hash: String::new(),
        };

        add_to_config(&dependency, true, target_config.to_str().unwrap(), false).unwrap();
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

        assert_eq!(
            read_file_to_string(&String::from(target_config.to_str().unwrap())),
            content
        );

        let _ = remove_file(target_config);
        Ok(())
    }

    ////////////// UTILS //////////////

    fn write_to_config(target_file: &PathBuf, content: &str) {
        if target_file.exists() {
            let _ = remove_file(target_file);
        }
        let mut file: std::fs::File = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target_file)
            .unwrap();
        if let Err(e) = write!(file, "{}", content) {
            eprintln!("Couldn't write to the config file: {}", e);
        }
    }

    fn define_config(foundry: bool) -> PathBuf {
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
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
