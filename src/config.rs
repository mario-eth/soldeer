use std::fs;
use std::path::PathBuf;
use std::path::Path;
use std::process::exit;
use serde_derive::Deserialize;
use toml;
use toml::Table;
use std::io::Write;
use std::fs::File;
use std::io::{ BufRead, BufReader };
use crate::utils::get_current_working_dir;

// TODO need to improve this, to propagate the error to main and not exit here.
pub fn read_config(filename: String) -> Vec<Dependency> {
    let mut filename: String = filename;
    if filename == "" {
        filename = define_config_file();
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
    data.dependencies.iter().for_each(|(k, v)| {
        let parts: Vec<&str> = k.split("~").collect::<Vec<&str>>();
        dependencies.push(Dependency {
            name: parts.get(0).unwrap().to_string(),
            version: parts.get(1).unwrap().to_string(),
            url: v.to_string().replace("\"", ""),
        });
    });

    return dependencies;
}

pub fn define_config_file() -> String {
    // reading the current directory to look for the config file
    let working_dir: Result<PathBuf, std::io::Error> = get_current_working_dir();

    let filename: String =
        working_dir.unwrap().into_os_string().into_string().unwrap() + "/soldeer.toml";
    let exists: bool = Path::new(&filename).exists();
    if exists {
        println!("Config file exists.");
    } else {
        eprintln!("Config file does not exist. Program exited.");
        exit(404);
    }
    return filename;
}
pub fn add_to_config(dependency_name: &str, dependency_version: &str, dependency_url: &str) {
    println!("Adding dependency {}-{} to config file", dependency_name, dependency_version);
    let filename: String = define_config_file();
    let dependencies: Vec<Dependency> = read_config(filename.clone());
    let mut dependency_exists: bool = false;
    for dependency in dependencies.iter() {
        if dependency.name == dependency_name && dependency.version == dependency_version {
            dependency_exists = true;
        }
    }
    if dependency_exists {
        println!(
            "Dependency {}-{} already exists in the config file",
            dependency_name,
            dependency_version
        );
        return;
    }
    let mut file: std::fs::File = fs::OpenOptions
        ::new()
        .write(true)
        .append(true)
        .open(filename)
        .unwrap();
    if
        let Err(e) = writeln!(
            file,
            "\n\"{}~{}\" = \"{}\"",
            dependency_name,
            dependency_version,
            dependency_url
        )
    {
        eprintln!("Couldn't write to file: {}", e);
    }
}

pub fn remappings() {
    if !enable_remappings() {
        return;
    }
    update_foundry();
}

fn update_foundry() {
    //TODO need to create the remappings file if it does not exists.
    if !Path::new("remappings.txt").exists() {
        File::create("remappings.txt").unwrap();
    }
    println!("Updating foundry...");
    // Read the contents of the file using a `match` block
    // to return the `data: Ok(c)` as a `String`
    // or handle any `errors: Err(_)`.
    let contents: String = match fs::read_to_string("remappings.txt") {
        // If successful return the files text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(_) => {
            // Write `msg` to `stderr`.
            eprintln!("Could not read file `{}`", "remappings.txt");
            // Exit the program with exit code `1`.
            exit(1);
        }
    };
    let existing_remappings: Vec<String> = contents
        .split("\n")
        .map(|s| s.to_string())
        .collect();
    let mut new_remappings: String = String::new();
    let dependencies: Vec<Dependency> = read_config(String::new());

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
                &format!("{}=dependencies/{}-{}\n", &dependency.name, &dependency.name, &dependency.version)
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

fn enable_remappings() -> bool {
    let filename = define_config_file();
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
    let data: Remmapings = match toml::from_str(&contents) {
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
    return data.remappings.get("enabled").unwrap().as_bool().unwrap();
}
// Top level struct to hold the TOML data.
#[derive(Deserialize)]
#[derive(Debug)]
struct Data {
    dependencies: Table,
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
struct Remmapings {
    remappings: Table,
}
