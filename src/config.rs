use crate::errors::ConfigError;
use crate::janitor::cleanup_dependency;
use crate::lock::remove_lock;
use crate::utils::{
    get_current_working_dir,
    read_file_to_string,
};
use serde_derive::Deserialize;
use std::fs::{
    self,
    remove_dir_all,
    File,
};
use std::io::{
    BufRead,
    BufReader,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};
use std::process::exit;
use toml::{
    self,
    Table,
};
use yansi::Paint;
extern crate toml_edit;
use std::io;
use toml_edit::{
    value,
    Document,
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

pub fn read_config(filename: String) -> Result<Vec<Dependency>, ConfigError> {
    let mut filename: String = filename;
    if filename.is_empty() {
        filename = match define_config_file() {
            Ok(file) => file,
            Err(err) => return Err(err),
        }
    }
    let contents = read_file_to_string(&filename.clone());
    // Use a `match` block to return the
    // file `contents` as a `Data struct: Ok(d)`
    // or handle any `errors: Err(_)`.
    let data: Data = match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(err) => {
            println!("{:?}", err);
            return Err(ConfigError {
                cause: format!("Could not read the config file {}", filename),
            });
        }
    };

    let mut dependencies: Vec<Dependency> = Vec::new();
    data.dependencies.iter().for_each(|(k, v)| {
        dependencies.push(Dependency {
            name: k.to_string(),
            version: v["version"].to_string().replace('"', ""),
            url: v["url"].to_string().replace('\"', ""),
        });
    });

    Ok(dependencies)
}

pub fn define_config_file() -> Result<String, ConfigError> {
    // reading the current directory to look for the config file
    let working_dir: Result<PathBuf, std::io::Error> = get_current_working_dir();

    let mut filename: String = working_dir
        .as_ref()
        .unwrap()
        .clone()
        .into_os_string()
        .into_string()
        .unwrap()
        .to_owned()
        + "/soldeer.toml";

    match fs::metadata(&filename) {
        Ok(_) => {}
        Err(_) => {
            filename = working_dir
                .as_ref()
                .unwrap()
                .clone()
                .into_os_string()
                .into_string()
                .unwrap()
                .to_owned()
                + "/foundry.toml";
            if !Path::new(&filename).exists() {
                println!("{}", Paint::blue("No config file found. If you wish to proceed, please select how you want Soldeer to be configured:\n1. Using foundry.toml\n2. Using soldeer.toml\n(Press 1 or 2)"));
                std::io::stdout().flush().unwrap();
                let mut option = String::new();
                if io::stdin().read_line(&mut option).is_err() {
                    return Err(ConfigError {
                        cause: "Option invalid.".to_string(),
                    });
                }
                match create_example_config(option) {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(err);
                    }
                }
            }
        }
    }

    let foundry_file = working_dir
        .as_ref()
        .unwrap()
        .clone()
        .into_os_string()
        .into_string()
        .unwrap()
        .to_owned()
        + "/foundry.toml";

    // check if the foundry.toml has the dependencies defined, if so then we setup the foundry.toml as the config file
    if fs::metadata(&foundry_file).is_ok() {
        let contents = read_file_to_string(&foundry_file.clone());
        let doc: Document = contents.parse::<Document>().expect("invalid doc");

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
) -> Result<(), ConfigError> {
    println!(
        "{}",
        Paint::green(format!(
            "Adding dependency {}-{} to the config file",
            dependency_name, dependency_version
        ))
    );
    let filename: String = match define_config_file() {
        Ok(file) => file,

        Err(err) => {
            let dir = get_current_working_dir()
                .unwrap()
                .join("dependencies")
                .join(dependency_name);
            remove_dir_all(dir).unwrap();
            match cleanup_dependency(dependency_name, dependency_version) {
                Ok(_) => {}
                Err(_) => {
                    return Err(ConfigError {
                        cause: "Could not delete the artifacts".to_string(),
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
    let mut doc: Document = contents.parse::<Document>().expect("invalid doc");
    let item = doc["dependencies"].get(dependency_name);
    if doc.get("dependencies").is_some()
        && item.is_some()
        && item.unwrap()["version"].to_string().replace('"', "") == dependency_version
    {
        println!(
            "{}",
            Paint::yellow(format!(
                "Dependency {}-{} already exists in the config file",
                dependency_name, dependency_version
            ))
        );
        return Ok(());
    }

    // in case we don't have dependencies defined in the config file, we add it and re-read the doc
    if doc.get("dependencies").is_none() {
        let mut file: std::fs::File = fs::OpenOptions::new().append(true).open(&filename).unwrap();
        if let Err(e) = write!(file, "{}", String::from("\n[dependencies]\n")) {
            eprintln!("Couldn't write to file: {}", e);
        }

        doc = read_file_to_string(&filename.clone())
            .parse::<Document>()
            .expect("invalid doc");
    }
    let mut new_dependencies: String = String::new(); //todo delete this

    // in case we don't have dependencies defined in the config file, we add it and re-read the doc
    if doc.get("dependencies").is_none() {
        let mut file: std::fs::File = fs::OpenOptions::new().append(true).open(&filename).unwrap();
        if let Err(e) = write!(file, "{}", String::from("\n[dependencies]\n")) {
            eprintln!("Couldn't write to file: {}", e);
        }

        doc = read_file_to_string(&filename.clone())
            .parse::<Document>()
            .expect("invalid doc");
    }

    new_dependencies.push_str(&format!(
        "  \"{}~{}\" = \"{}\"\n",
        dependency_name, dependency_version, dependency_url
    ));

    let mut new_item: Item = Item::None;
    new_item["version"] = value(dependency_version);
    new_item["url"] = value(dependency_url);
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
        eprintln!("Couldn't write to file: {}", e);
    }
    Ok(())
}

pub fn remappings() -> Result<(), ConfigError> {
    let remappings_path = get_current_working_dir().unwrap().join("remappings.txt");
    if !remappings_path.exists() {
        File::create(remappings_path.clone()).unwrap();
    }
    let contents = read_file_to_string(&remappings_path.to_str().unwrap().to_string());

    let existing_remappings: Vec<String> = contents.split('\n').map(|s| s.to_string()).collect();
    let mut new_remappings: String = String::new();

    let dependencies: Vec<Dependency> = match read_config(String::new()) {
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
                Paint::green(format!(
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
        remove_empty_lines("remappings.txt".to_string());
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
                Paint::yellow("Could not write to the remappings file".to_string())
            );
        }
    }
    remove_empty_lines("remappings.txt".to_string());
    Ok(())
}

fn remove_empty_lines(filename: String) {
    let file: File = File::open(filename).unwrap();

    let reader: BufReader<File> = BufReader::new(file);
    let mut new_content: String = String::new();
    let lines: Vec<_> = reader.lines().collect();
    let total: usize = lines.len();
    for (index, line) in lines.into_iter().enumerate() {
        let line: &String = line.as_ref().unwrap();
        // Making sure the line contains something
        if line.len() > 2 {
            if index == total - 1 {
                new_content.push_str(&line.to_string());
            } else {
                new_content.push_str(&format!("{}\n", line));
            }
        }
    }

    // Removing the annoying new lines at the end and beginning of the file
    new_content = String::from(new_content.trim_end_matches('\n'));
    new_content = String::from(new_content.trim_start_matches('\n'));
    let mut file: std::fs::File = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .append(false)
        .open(Path::new("remappings.txt"))
        .unwrap();

    match write!(file, "{}", &new_content) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Couldn't write to file: {}", e);
        }
    }
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

    // Use a `match` block to return the
    // file `contents` as a `Data struct: Ok(d)`
    // or handle any `errors: Err(_)`.
    let data: Foundry = match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(_) => {
            println!(
                "{}",
                Paint::yellow("The remappings field not found in the soldeer.toml and no foundry config file found or the foundry.toml does not contain the `[dependencies]` field. \nThe foundry.toml file should contain the `[dependencies]` field if you want to use it as a config file. If you want to use the soldeer.toml file, please add the `[remappings]` field to it with the `enabled` key set to `true` or `false`. \nMore info on https://github.com/mario-eth/soldeer\nThe installation was successful but the remappings feature was skipped.".to_string())
            );
            return Ok(vec![false]);
        }
    };
    if data.remappings.get("enabled").is_none() {
        println!(
            "{}",
            Paint::yellow("The remappings field not found in the soldeer.toml and no foundry config file found or the foundry.toml does not contain the `[dependencies]` field. \nThe foundry.toml file should contain the `[dependencies]` field if you want to use it as a config file. If you want to use the soldeer.toml file, please add the `[remappings]` field to it with the `enabled` key set to `true` or `false`. \nMore info on https://github.com/mario-eth/soldeer\nThe installation was successful but the remappings feature was skipped.".to_string())
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

fn create_example_config(option: String) -> Result<(), ConfigError> {
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
