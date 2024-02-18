use std::fs;
use std::io::Cursor;
use std::path::{
    Path,
    PathBuf,
};
use tokio_dl_stream_to_disk::AsyncDownload;
use yansi::Paint;

use crate::config::Dependency;
use crate::errors::DownloadError;
use crate::errors::UnzippingError;
use crate::remote::get_dependency_url_remote;
use crate::utils::{
    get_current_working_dir,
    read_file,
};

pub async fn download_dependencies(
    dependencies: &[Dependency],
    clean: bool,
) -> Result<(), DownloadError> {
    // clean dependencies folder if flag is true
    if clean {
        clean_dependency_directory();
    }
    // downloading dependencies to dependencies folder
    for dependency in dependencies.iter() {
        let file_name: String = format!("{}-{}.zip", dependency.name, dependency.version);
        match download_dependency(&file_name, &dependency.url).await {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }
    }
    Ok(())
}

// un-zip-ing dependencies to dependencies folder
pub fn unzip_dependencies(dependencies: &[Dependency]) -> Result<(), UnzippingError> {
    for dependency in dependencies.iter() {
        match unzip_dependency(&dependency.name, &dependency.version) {
            Ok(_) => {}
            Err(err) => {
                return Err(err);
            }
        }
    }
    Ok(())
}

#[allow(unused_assignments)]
pub async fn download_dependency_remote(
    dependency_name: &String,
    dependency_version: &String,
) -> Result<String, DownloadError> {
    let dependency_url = match get_dependency_url_remote(dependency_name, dependency_version).await
    {
        Ok(url) => url,
        Err(err) => return Err(err),
    };

    match download_dependency(
        &format!("{}-{}.zip", &dependency_name, &dependency_version),
        &dependency_url,
    )
    .await
    {
        Ok(_) => Ok(dependency_url),
        Err(err) => {
            eprintln!("Error downloading dependency: {:?}", err);
            Err(err)
        }
    }
    // Ok(dependency_url)
}

pub async fn download_dependency(
    dependency_name: &str,
    dependency_url: &str,
) -> Result<(), DownloadError> {
    let dependency_directory: PathBuf = get_current_working_dir().unwrap().join("dependencies");
    if !dependency_directory.is_dir() {
        fs::create_dir(&dependency_directory).unwrap();
    }

    let download_result: Result<(), tokio_dl_stream_to_disk::error::Error> =
        AsyncDownload::new(dependency_url, &dependency_directory, dependency_name)
            .download(&None)
            .await;
    if download_result.is_ok() {
        println!(
            "{}",
            Paint::green(format!("Dependency {} downloaded! ", dependency_name))
        );
        Ok(())
    } else if download_result
        .err()
        .unwrap()
        .to_string()
        .contains("already exists")
    {
        println!(
            "{}",
            Paint::yellow(format!("Dependency {} already downloaded", dependency_name))
        );
        return Ok(());
    } else {
        return Err(DownloadError {
            name: "Unknown".to_string(),
            version: "Unknown".to_string(),
        });
    }
}

pub fn unzip_dependency(
    dependency_name: &String,
    dependency_version: &String,
) -> Result<(), UnzippingError> {
    let file_name: String = format!("{}-{}.zip", dependency_name, dependency_version);
    let target_name: String = format!("{}-{}/", dependency_name, dependency_version);
    let current_dir: PathBuf = get_current_working_dir()
        .unwrap()
        .join(Path::new(&("dependencies/".to_owned() + &file_name)));

    let target = get_current_working_dir()
        .unwrap()
        .join("dependencies/")
        .join(target_name);
    let archive: Vec<u8> = read_file(current_dir.as_path().to_str().unwrap().to_string()).unwrap();
    match zip_extract::extract(Cursor::new(archive), &target, true) {
        Ok(_) => {}
        Err(_) => {
            return Err(UnzippingError {
                name: dependency_name.to_string(),
                version: dependency_version.to_string(),
            })
        }
    }
    println!(
        "{}",
        Paint::green(format!(
            "The dependency {}-{} was unzipped!",
            dependency_name, dependency_version
        ))
    );
    Ok(())
}

pub fn clean_dependency_directory() {
    let dep_path = get_current_working_dir().unwrap().join("dependencies");
    if dep_path.is_dir() {
        fs::remove_dir_all(&dep_path).unwrap();
        fs::create_dir(&dep_path).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::MissingDependencies;
    use crate::janitor::healthcheck_dependency;
    use serial_test::serial;

    // Helper macro to run async tests
    macro_rules! aw {
        ($e:expr) => {
            tokio_test::block_on($e)
        };
    }

    #[test]
    #[serial]
    fn unzip_dependency_success() {
        let mut dependencies: Vec<Dependency> = Vec::new();
        dependencies.push(Dependency {
            name: "@openzeppelin-contracts".to_string(),
            version: "2.3.0".to_string(),
            url: "https://github.com/mario-eth/soldeer-versions/raw/main/all_versions/@openzeppelin-contracts~2.3.0.zip".to_string(),
        });
        let _ = aw!(download_dependencies(&dependencies, false));
        let result: Result<(), UnzippingError> =
            unzip_dependency(&dependencies[0].name, &dependencies[0].version);
        assert!(result.is_ok());
        let result: Result<(), MissingDependencies> =
            healthcheck_dependency("@openzeppelin-contracts", "2.3.0");
        assert!(result.is_ok());
    }
}
