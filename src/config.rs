use std::fs::{ self, File };
use std::path::{ PathBuf, Path };
use std::process::exit;
use serde_derive::Deserialize;
use toml::{ self, Table };
use std::io::{ Write, BufRead, BufReader };
use crate::utils::get_current_working_dir;
extern crate toml_edit;
use toml_edit::{ Document, value };
use crate::FOUNDRY;

// TODO need to improve this, to propagate the error to main and not exit here.
pub fn read_config(filename: String, foundry_setup: &FOUNDRY) -> Vec<Dependency> {
    let mut filename: String = filename;
    if filename == "" {
        filename = define_config_file(foundry_setup);
    }
    // Read the contents of the file using a `match` block
    // to return the `data: Ok(c)` as a `String`
    // or handle any `errors: Err(_)`.
    let contents: String = match fs::read_to_string(&filename) {
        // If successful return the files text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(_) => {
            // Write `msg` to `stderr`.
            eprintln!("Could not read file `{}`", &filename);
            // Exit the program with exit code `1`.
            exit(1);
        }
    };

    // Use a `match` block to return the
    // file `contents` as a `Data struct: Ok(d)`
    // or handle any `errors: Err(_)`.
    let data: Data = match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(err) => {
            eprintln!("Error: {}", err);
            // Write `msg` to `stderr`.
            eprintln!("Unable to load data from `{}`", filename);
            // Exit the program with exit code `1`.
            exit(1);
        }
    };

    let mut dependencies: Vec<Dependency> = Vec::new();
    data.sdependencies.iter().for_each(|(k, v)| {
        let parts: Vec<&str> = k.split("~").collect::<Vec<&str>>();
        dependencies.push(Dependency {
            name: parts.get(0).unwrap().to_string(),
            version: parts.get(1).unwrap().to_string(),
            url: v.to_string().replace("\"", ""),
        });
    });

    return dependencies;
}

pub fn define_config_file(foundry_setup: &FOUNDRY) -> String {
    // reading the current directory to look for the config file
    let working_dir: Result<PathBuf, std::io::Error> = get_current_working_dir();

    let mut filename: String =
        working_dir.as_ref().unwrap().clone().into_os_string().into_string().unwrap().to_owned() +
        "/soldeer.toml";

    if foundry_setup.config {
        filename =
            working_dir.as_ref().unwrap().clone().into_os_string().into_string().unwrap().clone() +
            "/foundry.toml";
    }
    let exists: bool = Path::new(&filename).exists();
    if exists {
        // println!("Config file exists.");
    } else {
        eprintln!("Config file does not exist. Program exited.");
        exit(404);
    }
    return filename;
}
pub fn add_to_config(
    dependency_name: &str,
    dependency_version: &str,
    dependency_url: &str,
    foundry_setup: &FOUNDRY
) {
    println!("Adding dependency {}-{} to config file", dependency_name, dependency_version);
    let filename: String = define_config_file(foundry_setup);
    let contents = read_file_to_string(filename.clone());
    let mut doc: Document = contents.parse::<Document>().expect("invalid doc");

    if
        !doc.get("sdependencies").is_none() &&
        !doc["sdependencies"].get(format!("{}~{}", dependency_name, dependency_version)).is_none()
    {
        println!(
            "Dependency {}-{} already exists in the config file",
            dependency_name,
            dependency_version
        );
        return;
    }
    doc["sdependencies"][format!("{}~{}", dependency_name, dependency_version)] =
        value(dependency_url);
    let mut file: std::fs::File = fs::OpenOptions
        ::new()
        .write(true)
        .append(false)
        .open(filename)
        .unwrap();
    if let Err(e) = write!(file, "{}", doc.to_string()) {
        eprintln!("Couldn't write to file: {}", e);
    }
}

pub fn remappings(foundry_setup: &FOUNDRY) {
    if !Path::new("remappings.txt").exists() {
        File::create("remappings.txt").unwrap();
    }
    println!("Updating foundry...");
    let contents = read_file_to_string(String::from("remappings.txt"));

    let existing_remappings: Vec<String> = contents
        .split("\n")
        .map(|s| s.to_string())
        .collect();
    let mut new_remappings: String = String::new();
    let dependencies: Vec<Dependency> = read_config(String::new(), foundry_setup);

    let mut existing_remap: Vec<String> = Vec::new();
    existing_remappings.iter().for_each(|remapping| {
        let split: Vec<&str> = remapping.split("=").collect::<Vec<&str>>();
        existing_remap.push(String::from(split[0]));
    });

    dependencies.iter().for_each(|dependency| {
        let index = existing_remap.iter().position(|r| r == &dependency.name);
        if index.is_none() {
            println!("Adding a new remap {}", &dependency.name);
            new_remappings.push_str(
                &format!(
                    "{}=dependencies/{}-{}\n",
                    &dependency.name,
                    &dependency.name,
                    &dependency.version
                )
            );
        }
    });

    if new_remappings.len() == 0 {
        remove_empty_lines("remappings.txt".to_string());
        return;
    }

    let mut file: std::fs::File = fs::OpenOptions
        ::new()
        .write(true)
        .append(true)
        .open(Path::new("remappings.txt"))
        .unwrap();

    match write!(file, "{}", &new_remappings) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Couldn't write to file: {}", e);
        }
    }
    remove_empty_lines("remappings.txt".to_string());
}

fn remove_empty_lines(filename: String) {
    let file: File = File::open(&filename).unwrap();

    let reader: BufReader<File> = BufReader::new(file);
    let mut new_content: String = String::new();
    let lines: Vec<_> = reader.lines().collect();
    let total: usize = lines.len();
    for (index, line) in lines.into_iter().enumerate() {
        let line: &String = line.as_ref().unwrap();
        // Making sure the line contains something
        if line.len() > 2 {
            if index == total - 1 {
                new_content.push_str(&format!("{}", line));
            } else {
                new_content.push_str(&format!("{}\n", line));
            }
        }
    }

    // Removing the annoying new lines at the end and beginning of the file
    new_content = String::from(new_content.trim_end_matches('\n'));
    new_content = String::from(new_content.trim_start_matches('\n'));
    let mut file: std::fs::File = fs::OpenOptions
        ::new()
        .write(true)
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

pub fn get_foundry_setup() -> Vec<bool> {
    let filename = define_config_file(&(FOUNDRY { remappings: false, config: false }));

    let contents: String = read_file_to_string(filename.clone());

    // Use a `match` block to return the
    // file `contents` as a `Data struct: Ok(d)`
    // or handle any `errors: Err(_)`.
    let data: Foundry = match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(err) => {
            eprintln!("Error: {}", err);
            // Write `msg` to `stderr`.
            eprintln!("Unable to load data from `{}`", filename);
            // Exit the program with exit code `1`.
            exit(1);
        }
    };

    return vec![
        data.foundry.get("enabled").unwrap().as_bool().unwrap(),
        data.foundry.get("foundry-config").unwrap().as_bool().unwrap()
    ];
}

fn read_file_to_string(filename: String) -> String {
    let contents: String = match fs::read_to_string(&filename) {
        // If successful return the files text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(_) => {
            // Write `msg` to `stderr`.
            eprintln!("Could not read file `{}`", &filename);
            // Exit the program with exit code `1`.
            exit(1);
        }
    };
    return contents;
}
// Top level struct to hold the TOML data.
#[derive(Deserialize)]
#[derive(Debug)]
struct Data {
    sdependencies: Table,
}

// Dependency object used to store a dependency data
#[derive(Debug)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
#[derive(Debug)]
struct Foundry {
    foundry: Table,
}
