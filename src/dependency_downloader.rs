use std::path::Path;
use std::io::{ Cursor, Read, BufReader };
use reqwest::{ get, Response };
use std::fs::{ self, remove_file, File };
use std::fmt;
use tokio_dl_stream_to_disk::AsyncDownload;
use zip_extract::ZipExtractError;

use crate::config::{ Dependency, read_config };
use crate::utils::get_current_working_dir;

// TODOs:
// - needs to be downloaded in parallel
pub async fn download_dependencies(
    dependencies: &Vec<Dependency>,
    clean: bool
) -> Result<(), DownloadError> {
    if clean {
        let dep_path = get_current_working_dir().unwrap().join("dependencies");
        fs::remove_dir_all(&dep_path).unwrap();
        fs::create_dir(&dep_path).unwrap();
    }
    for dependency in dependencies.iter() {
        let file_name: String = format!("{}-{}.zip", dependency.name, dependency.version);
        match download_dependency(&file_name, &dependency.url).await {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Error downloading dependency: {:?}", err);
                return Err(err);
            }
        }
    }
    return Ok(());
}

// un-zip-ing dependencies to dependencies folder
pub fn unzip_dependencies(dependencies: &Vec<Dependency>) -> Result<(), ZipExtractError> {
    println!("Unzipping dependencies...");
    for dependency in dependencies.iter() {
        match unzip_dependency(&dependency.name, &dependency.version) {
            Ok(_) => {}
            Err(err) => {
                eprintln!("Error unzipping dependency: {:?}", err);
                return Err(err);
            }
        }
    }
    return Ok(());
}

pub async fn download_dependency_remote(
    dependency_name: &String,
    dependency_version: &String,
    remote_url: &String
) -> Result<String, DownloadError> {
    let res: Response = get(format!("{}", remote_url)).await.unwrap();
    let body: String = res.text().await.unwrap();
    let tmp_path: std::path::PathBuf = get_current_working_dir()
        .unwrap()
        .join(".dependency_reading.toml");
    fs::write(&tmp_path, body).expect("Unable to write file");
    let dependencies: Vec<Dependency> = read_config((&tmp_path).to_str().unwrap().to_string());
    for dependency in dependencies.iter() {
        if dependency.name == *dependency_name && dependency.version == *dependency_version {
            println!("dependency url: {}", dependency.url);
            match
                download_dependency(
                    &format!("{}-{}.zip", &dependency_name, &dependency_version),
                    &dependency.url
                ).await
            {
                Ok(_) => {}
                Err(err) => {
                    eprintln!("Error downloading dependency: {:?}", err);
                    remove_file(tmp_path).unwrap();
                    return Err(err);
                }
            }
            remove_file(tmp_path).unwrap();
            return Ok(dependency.url.to_string());
        }
    }
    remove_file(tmp_path).unwrap();
    return Ok(String::new());
}

pub async fn download_dependency(
    dependency_name: &String,
    dependency_url: &String
) -> Result<(), DownloadError> {
    let new_path: std::path::PathBuf = get_current_working_dir().unwrap().join("dependencies");
    if !new_path.is_dir() {
        fs::create_dir(&new_path).unwrap();
    }
    let download_result: Result<(), tokio_dl_stream_to_disk::error::Error> = AsyncDownload::new(
        dependency_url,
        &new_path,
        dependency_name
    ).download(&None).await;
    if download_result.is_ok() {
        println!("{} downloaded successfully!", dependency_name);
        return Ok(());
    } else if download_result.err().unwrap().to_string().contains("already exists") {
        eprintln!("Dependency {} already downloaded", dependency_name);
        return Ok(());
    } else {
        eprintln!("Dependency {} not downloaded", dependency_name);
        return Err(DownloadError);
    }
}

pub fn unzip_dependency(
    dependency_name: &String,
    dependency_version: &String
) -> Result<(), ZipExtractError> {
    println!("Unzipping dependency {}-{}", dependency_name, dependency_version);
    let file_name: String = format!("{}-{}.zip", dependency_name, dependency_version);
    let target_name: String = format!("{}-{}/", dependency_name, dependency_version);
    let current_dir: std::path::PathBuf = get_current_working_dir()
        .unwrap()
        .join(Path::new(&("dependencies/".to_owned() + &file_name)));

    let target = get_current_working_dir().unwrap().join("dependencies/").join(target_name);
    let archive: Vec<u8> = read_file(current_dir.as_path().to_str().unwrap().to_string()).unwrap();
    zip_extract::extract(Cursor::new(archive), &target, true)?;
    return Ok(());
}

// read a file contents into a vector of bytes so we can unzip it
fn read_file(path: String) -> Result<Vec<u8>, std::io::Error> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let mut buffer = Vec::new();

    // Read file into vector.
    reader.read_to_end(&mut buffer)?;

    return Ok(buffer);
}

#[derive(Debug, Clone)]
pub struct DownloadError;

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "download failed")
    }
}
