use crate::errors::ConfigError;
use crate::janitor::cleanup_dependency;
use crate::lock::remove_lock;
use crate::remote::get_dependency_url_remote;
use crate::utils::{
    get_current_working_dir,
    read_file_to_string,
    remove_empty_lines,
};
use crate::DEPENDENCY_DIR;
use serde_derive::Deserialize;
use std::fs::{
    self,
    remove_dir_all,
    File,
};
use std::io::Write;
use std::path::{
    Path,
    PathBuf,
};
use std::process::exit;
use toml::Table;
use yansi::Paint;
extern crate toml_edit;
use std::io;
use toml_edit::{
    value,
    DocumentMut,
    Item,
};

// Top level struct to hold the TOML data.
#[derive(Deserialize, Debug)]
struct Data {
    dependencies: Table,
}

// Dependency object used to store a dependency data
#[derive(Deserialize, Clone, Debug)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub url: String,
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
        Err(err) => {
            println!("{:?}", err);
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
        });
    }

    Ok(dependencies)
}

pub fn define_config_file() -> Result<String, ConfigError> {
    // reading the current directory to look for the config file
    let working_dir = get_current_working_dir()
        .as_ref()
        .unwrap()
        .clone()
        .into_os_string()
        .into_string()
        .unwrap()
        .to_owned();

    let mut filename: String = working_dir.to_owned() + "/soldeer.toml";

    match fs::metadata(&filename) {
        Ok(_) => {}
        Err(_) => {
            filename = working_dir.to_owned() + "/foundry.toml";
            if !Path::new(&filename).exists() {
                println!("{}", Paint::blue("No config file found. If you wish to proceed, please select how you want Soldeer to be configured:\n1. Using foundry.toml\n2. Using soldeer.toml\n(Press 1 or 2)"));
                std::io::stdout().flush().unwrap();
                let mut option = String::new();
                if io::stdin().read_line(&mut option).is_err() {
                    return Err(ConfigError {
                        cause: "Option invalid.".to_string(),
                    });
                }
                match create_example_config(&option) {
                    Ok(_) => {
                        if &option == "1" {
                            filename = working_dir.to_owned() + "/foundry.toml";
                        } else {
                            filename = working_dir.to_owned() + "/soldeer.toml";
                        }
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }
            }
        }
    }

    let foundry_file = working_dir.to_owned() + "/foundry.toml";

    // check if the foundry.toml has the dependencies defined, if so then we setup the foundry.toml as the config file
    if fs::metadata(&foundry_file).is_ok() {
        let contents = read_file_to_string(&foundry_file.clone());
        let doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

        if doc.get("dependencies").is_some() {
            filename = foundry_file;
        }
    }

    let exists: bool = Path::new(&filename).exists();
    if !exists {
        eprintln!(
            "The config file does not exist. Soldeer has exited. If you wish to proceed, below is the minimum requirement for the soldeer.toml file that needs to be created:\n \n [foundry]\n enabled = true\n foundry-config = false\n\n [dependencies]\n"
        );
        exit(404);
    }
    Ok(filename)
}

pub fn add_to_config(
    dependency_name: &str,
    dependency_version: &str,
    dependency_url: &str,
    custom_url: bool,
) -> Result<(), ConfigError> {
    println!(
        "{}",
        Paint::green(&format!(
            "Adding dependency {}-{} to the config file",
            dependency_name, dependency_version
        ))
    );
    let filename: String = match define_config_file() {
        Ok(file) => file,

        Err(err) => {
            let dir = DEPENDENCY_DIR.join(dependency_name);
            remove_dir_all(dir).unwrap();
            match cleanup_dependency(dependency_name, dependency_version) {
                Ok(_) => {}
                Err(_) => {
                    return Err(ConfigError {
                        cause: "Could not delete the dependency artifacts".to_string(),
                    });
                }
            }

            match remove_lock(dependency_name, dependency_version) {
                Ok(_) => {}
                Err(_) => {
                    return Err(ConfigError {
                        cause: "Could not remove the lock".to_string(),
                    })
                }
            }
            return Err(err);
        }
    };

    let contents = read_file_to_string(&filename.clone());
    let mut doc: DocumentMut = contents.parse::<DocumentMut>().expect("invalid doc");

    if doc.contains_table("dependencies") {
        let item = doc["dependencies"].get(dependency_name);
        if doc.get("dependencies").is_some()
            && item.is_some()
            && item.unwrap()["version"].to_string().replace('"', "") == dependency_version
        {
            println!(
                "{}",
                Paint::yellow(&format!(
                    "Dependency {}-{} already exists in the config file",
                    dependency_name, dependency_version
                ))
            );
            return Ok(());
        }
    }

    // in case we don't have dependencies defined in the config file, we add it and re-read the doc
    if !doc.contains_table("dependencies") {
        let mut file: std::fs::File = fs::OpenOptions::new().append(true).open(&filename).unwrap();
        if let Err(e) = write!(file, "{}", String::from("\n[dependencies]\n")) {
            eprintln!("Couldn't write to the config file: {}", e);
        }

        doc = read_file_to_string(&filename.clone())
            .parse::<DocumentMut>()
            .expect("invalid doc");
    }
    let mut new_dependencies: String = String::new();

    new_dependencies.push_str(&format!(
        "  \"{}~{}\" = \"{}\"\n",
        dependency_name, dependency_version, dependency_url
    ));

    let mut new_item: Item = Item::None;
    new_item["version"] = value(dependency_version);
    if custom_url {
        new_item["url"] = value(dependency_url);
    }
    doc["dependencies"]
        .as_table_mut()
        .unwrap()
        .insert(dependency_name.to_string().as_str(), new_item);
    let mut file: std::fs::File = fs::OpenOptions::new()
        .write(true)
        .append(false)
        .open(filename)
        .unwrap();
    if let Err(e) = write!(file, "{}", doc) {
        eprintln!("Couldn't write to the config file: {}", e);
    }
    Ok(())
}

pub async fn remappings() -> Result<(), ConfigError> {
    let remappings_path = get_current_working_dir().unwrap().join("remappings.txt");
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
                Paint::yellow(&"The remappings field not found in the soldeer.toml and no foundry config file found or the foundry.toml does not contain the `[dependencies]` field. \nThe foundry.toml file should contain the `[dependencies]` field if you want to use it as a config file. If you want to use the soldeer.toml file, please add the `[remappings]` field to it with the `enabled` key set to `true` or `false`. \nMore info on https://github.com/mario-eth/soldeer\nThe installation was successful but the remappings feature was skipped.".to_string())
            );
            return Ok(vec![false]);
        }
    };
    if data.remappings.get("enabled").is_none() {
        println!(
            "{}",
            Paint::yellow(&"The remappings field not found in the soldeer.toml and no foundry config file found or the foundry.toml does not contain the `[dependencies]` field. \nThe foundry.toml file should contain the `[dependencies]` field if you want to use it as a config file. If you want to use the soldeer.toml file, please add the `[remappings]` field to it with the `enabled` key set to `true` or `false`. \nMore info on https://github.com/mario-eth/soldeer\nThe installation was successful but the remappings feature was skipped.".to_string())
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

fn create_example_config(option: &str) -> Result<(), ConfigError> {
    let config_file: &str;
    let content: &str;
    let mut path: PathBuf = get_current_working_dir().unwrap();
    if option.trim() == "1" {
        path = path.join("foundry.toml");
        config_file = path.to_str().unwrap();
        content = r#"
# Full reference https://github.com/foundry-rs/foundry/tree/master/crates/config

[profile.default]
auto_detect_solc = false
bytecode_hash = "none"
evm_version = "paris"           # See https://www.evmdiff.com/features?name=PUSH0&kind=opcode
fuzz = { runs = 1_000 }
gas_reports = ["*"]
optimizer = true
optimizer_runs = 10_000
out = "out"
script = "script"
solc = "0.8.23"
src = "src"
test = "test"
libs = ["dependencies"]

[dependencies]
"#;
    } else if option.trim() == "2" {
        path = path.join("soldeer.toml");
        config_file = path.to_str().unwrap();
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
    Ok(())
}
