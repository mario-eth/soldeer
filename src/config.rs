use std::fs::{ self, File };
use std::path::{ PathBuf, Path };
use std::process::exit;
use serde_derive::Deserialize;
use toml::{ self, Table };
use std::io::{ Write, BufRead, BufReader };
use crate::utils::get_current_working_dir;
extern crate toml_edit;
use toml_edit::{ Document, value };

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
    let contents = read_file_to_string(filename.clone());
    let mut doc: Document = contents.parse::<Document>().expect("invalid doc");

    if !doc["dependencies"].get(format!("{}~{}", dependency_name, dependency_version)).is_none() {
        println!(
            "Dependency {}-{} already exists in the config file",
            dependency_name,
            dependency_version
        );
        return;
    }
    doc["dependencies"][format!("{}~{}", dependency_name, dependency_version)] =
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

pub fn remappings() {
    if !enable_remappings() {
        return;
    }
    update_foundry();
}

fn update_foundry() {
    if !Path::new("remappings.txt").exists() {
        File::create("remappings.txt").unwrap();
    }
    println!("Updating foundry...");
    let mut new_remappings: String = String::new();
    let dependencies: Vec<Dependency> = read_config(String::new());

    dependencies.iter().for_each(|dependency| {
        println!("Adding a new remap {}", &dependency.name);
        new_remappings.push_str(
            &format!(
                "{}=dependencies/{}-{}\n",
                &dependency.name,
                &dependency.name,
                &dependency.version
            )
        );
    });

    if new_remappings.len() == 0 {
        remove_empty_lines("remappings.txt".to_string());
        return;
    }

    let mut file: std::fs::File = fs::OpenOptions
        ::new()
        .write(true)
        .truncate(true)
        .append(false)
        .open(Path::new("remappings.txt"))
        .unwrap();
    println!("New remappings: {}", &new_remappings);
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

    let contents: String = read_file_to_string(filename.clone());

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
