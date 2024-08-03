use crate::{
    auth::get_token,
    errors::{AuthError, PublishError},
    remote::get_project_id,
    utils::{get_base_url, get_current_working_dir, read_file, read_file_to_string},
};
use regex::Regex;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    multipart::{Form, Part},
    Client, StatusCode,
};
use std::{
    fs::{remove_file, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use yansi::Paint;
use yash_fnmatch::{without_escape, Pattern};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

pub type Result<T> = std::result::Result<T, PublishError>;

pub async fn push_version(
    dependency_name: &str,
    dependency_version: &str,
    root_directory_path: PathBuf,
    dry_run: bool,
) -> Result<()> {
    let file_name = root_directory_path.file_name().expect("path should have a last component");
    println!(
        "{}",
        Paint::green(&format!("Pushing a dependency {}-{}:", dependency_name, dependency_version))
    );

    let files_to_copy: Vec<PathBuf> = filter_files_to_copy(&root_directory_path);

    let zip_archive = match zip_file(&root_directory_path, &files_to_copy, file_name) {
        Ok(zip) => zip,
        Err(err) => {
            return Err(err);
        }
    };

    if dry_run {
        return Ok(());
    }

    match push_to_repo(&zip_archive, dependency_name, dependency_version).await {
        Ok(_) => {}
        Err(error) => {
            remove_file(zip_archive.to_str().unwrap()).unwrap();
            return Err(error);
        }
    }
    // deleting zip archive
    let _ = remove_file(zip_archive);

    Ok(())
}

pub fn validate_name(name: &str) -> Result<()> {
    let regex = Regex::new(r"^[@|a-z0-9][a-z0-9-]*[a-z0-9]$").unwrap();
    if !regex.is_match(name) {
        return Err(PublishError::InvalidName);
    }
    Ok(())
}

fn zip_file(
    root_directory_path: &Path,
    files_to_copy: &Vec<PathBuf>,
    file_name: impl Into<PathBuf>,
) -> Result<PathBuf> {
    let mut file_name: PathBuf = file_name.into();
    file_name.set_extension("zip");
    let zip_file_path = root_directory_path.join(file_name);
    let file = File::create(&zip_file_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::DEFLATE);
    if files_to_copy.is_empty() {
        return Err(PublishError::NoFiles);
    }

    for file_path in files_to_copy {
        let file_to_copy = File::open(file_path.clone())
            .map_err(|e| PublishError::IOError { path: file_path.clone(), source: e })?;
        let path = Path::new(&file_path);
        let mut buffer = Vec::new();

        // This is the relative path, we basically get the relative path to the target folder that
        // we want to push and zip that as a name so we won't screw up the file/dir
        // hierarchy in the zip file.
        let relative_file_path = file_path.strip_prefix(root_directory_path)?;

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            zip.start_file(relative_file_path.to_string_lossy(), options)?;
            io::copy(&mut file_to_copy.take(u64::MAX), &mut buffer)
                .map_err(|e| PublishError::IOError { path: file_path.clone(), source: e })?;
            zip.write_all(&buffer)
                .map_err(|e| PublishError::IOError { path: zip_file_path.clone(), source: e })?;
        } else if path.is_dir() {
            let _ = zip.add_directory(file_path.to_string_lossy(), options);
        }
    }
    let _ = zip.finish();
    Ok(zip_file_path)
}

fn filter_files_to_copy(root_directory_path: &Path) -> Vec<PathBuf> {
    let ignore_files: Vec<String> = read_ignore_file();

    let root_directory: &str = &(root_directory_path.to_str().unwrap().to_owned() + "/");
    let mut files_to_copy: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(root_directory).into_iter().filter_map(|e| e.ok()) {
        let is_dir = entry.path().is_dir();
        let file_path: String = entry.path().to_str().unwrap().to_string();
        if file_path.is_empty() || is_dir {
            continue;
        }
        let mut found: bool = false;
        for ignore_file in ignore_files.iter() {
            let p = Pattern::parse(without_escape(ignore_file)).unwrap();
            let exists = p.find(&file_path);
            if exists.is_some() {
                found = true;
                break;
            }
        }

        if found {
            continue;
        }

        files_to_copy.push(entry.path().to_path_buf());
    }
    files_to_copy
}

fn read_ignore_file() -> Vec<String> {
    let mut current_dir = get_current_working_dir();
    if cfg!(test) {
        current_dir = get_current_working_dir().join("test");
    }
    let gitignore = current_dir.join(".gitignore");
    let soldeerignore = current_dir.join(".soldeerignore");

    let mut files: Vec<String> = Vec::new();

    if soldeerignore.exists() {
        let contents = read_file_to_string(&soldeerignore);
        let current_read_file = contents.lines();
        files.append(&mut escape_lines(current_read_file.collect()));
    }

    if gitignore.exists() {
        let contents = read_file_to_string(&gitignore);
        let current_read_file = contents.lines();
        files.append(&mut escape_lines(current_read_file.collect()));
    }

    files
}

fn escape_lines(lines: Vec<&str>) -> Vec<String> {
    let mut escaped_liens: Vec<String> = vec![];
    for line in lines {
        if !line.trim().is_empty() {
            escaped_liens.push(line.trim().to_string());
        }
    }
    escaped_liens
}

async fn push_to_repo(
    zip_file: &Path,
    dependency_name: &str,
    dependency_version: &str,
) -> Result<()> {
    let token = get_token()?;
    let client = Client::new();

    let url = format!("{}/api/v1/revision/upload", get_base_url());

    let mut headers: HeaderMap = HeaderMap::new();

    let header_string = format!("Bearer {}", token);
    let header_value = HeaderValue::from_str(&header_string);

    headers.insert(AUTHORIZATION, header_value.expect("Could not set auth header"));

    let file_fs = read_file(zip_file).unwrap();
    let mut part = Part::bytes(file_fs).file_name(
        zip_file
            .file_name()
            .expect("path should have a last component")
            .to_string_lossy()
            .into_owned(),
    );

    // set the mime as app zip
    part = part.mime_str("application/zip").expect("Could not set mime type");

    let project_id = get_project_id(dependency_name).await?;

    let form = Form::new()
        .text("project_id", project_id)
        .text("revision", dependency_version.to_string())
        .part("zip_name", part);

    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&("multipart/form-data; boundary=".to_owned() + form.boundary()))
            .expect("Could not set content type"),
    );
    let res = client.post(url).headers(headers.clone()).multipart(form).send();

    let response = res.await.unwrap();
    match response.status() {
        StatusCode::OK => {
            println!("{}", Paint::green("Success!"));
            Ok(())
        }
        StatusCode::NO_CONTENT => Err(PublishError::ProjectNotFound),
        StatusCode::ALREADY_REPORTED => Err(PublishError::AlreadyExists),
        StatusCode::UNAUTHORIZED => Err(PublishError::AuthError(AuthError::InvalidCredentials)),
        StatusCode::PAYLOAD_TOO_LARGE => Err(PublishError::PayloadTooLarge),
        s if s.is_server_error() || s.is_client_error() => {
            Err(PublishError::HttpError(response.error_for_status().unwrap_err()))
        }
        _ => Err(PublishError::UnknownError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use io::Cursor;
    use rand::{distributions::Alphanumeric, Rng};
    use serial_test::serial;
    use std::fs::{self, create_dir_all, remove_dir_all, remove_file};

    #[test]
    #[serial]
    fn read_ignore_files_only_soldeerignore() {
        let soldeerignore = define_ignore_file(false);
        let gitignore = define_ignore_file(true);
        let _ = remove_file(gitignore);
        let ignore_contents = r#"
*.toml
*.zip
        "#;
        write_to_ignore(&soldeerignore, ignore_contents);
        let expected_results: Vec<String> = vec!["*.toml".to_string(), "*.zip".to_string()];

        assert_eq!(read_ignore_file(), expected_results);
        let _ = remove_file(soldeerignore);
    }

    #[test]
    #[serial]
    fn read_ignore_files_only_gitignore() {
        let soldeerignore = define_ignore_file(false);
        let gitignore = define_ignore_file(true);
        let _ = remove_file(soldeerignore);

        let ignore_contents = r#"
*.toml
*.zip
        "#;
        write_to_ignore(&gitignore, ignore_contents);
        let expected_results: Vec<String> = vec!["*.toml".to_string(), "*.zip".to_string()];

        assert_eq!(read_ignore_file(), expected_results);
        let _ = remove_file(gitignore);
    }

    #[test]
    #[serial]
    fn read_ignore_files_both_gitignore_soldeerignore() {
        let soldeerignore = define_ignore_file(false);
        let gitignore = define_ignore_file(true);
        let _ = remove_file(&soldeerignore);
        let _ = remove_file(&gitignore);

        let ignore_contents_git = r#"
*.toml
*.zip
        "#;
        write_to_ignore(&gitignore, ignore_contents_git);

        let ignore_contents_soldeer = r#"
        *.sol
        *.txt
                "#;
        write_to_ignore(&soldeerignore, ignore_contents_soldeer);

        let expected_results: Vec<String> = vec![
            "*.sol".to_string(),
            "*.txt".to_string(),
            "*.toml".to_string(),
            "*.zip".to_string(),
        ];

        assert_eq!(read_ignore_file(), expected_results);
        let _ = remove_file(gitignore);
        let _ = remove_file(soldeerignore);
    }

    #[test]
    #[serial]
    fn filter_only_files_success() {
        let target_dir = get_current_working_dir().join("test").join("test_push");
        let _ = remove_dir_all(&target_dir);
        let _ = create_dir_all(&target_dir);

        let soldeerignore = define_ignore_file(false);
        let gitignore = define_ignore_file(true);
        let _ = remove_file(soldeerignore);

        let mut ignored_files = vec![];
        let mut filtered_files = vec![];
        ignored_files.push(create_random_file(&target_dir, "toml".to_string()));
        ignored_files.push(create_random_file(&target_dir, "zip".to_string()));
        ignored_files.push(create_random_file(&target_dir, "toml".to_string()));
        filtered_files.push(create_random_file(&target_dir, "txt".to_string()));

        let ignore_contents_git = r#"
*.toml
*.zip
        "#;
        write_to_ignore(&gitignore, ignore_contents_git);

        let result = filter_files_to_copy(&target_dir);
        assert_eq!(filtered_files.len(), result.len());
        let file = Path::new(&filtered_files[0]);
        assert_eq!(file, result[0]);

        let _ = remove_file(gitignore);
        let _ = remove_dir_all(target_dir);
    }

    #[test]
    #[serial]
    fn filter_files_and_dir_success() {
        let target_dir = get_current_working_dir().join("test").join("test_push");
        let _ = remove_dir_all(&target_dir);
        let _ = create_dir_all(&target_dir);

        let soldeerignore = define_ignore_file(false);
        let gitignore = define_ignore_file(true);
        let _ = remove_file(soldeerignore);

        // divide ignored vs filtered files to check them later
        let mut ignored_files = vec![];
        let mut filtered_files = vec![];

        // initial dir to test the ignore
        let target_dir = get_current_working_dir().join("test").join("test_push");

        // we create various test files structure
        // - test_push/
        // --- random_dir/ <= not ignored
        // --- --- random.toml <= ignored
        // --- --- random.zip <= not ignored
        // --- broadcast/ <= not ignored
        // --- --- random.toml <= ignored
        // --- --- random.zip <= not ignored
        // --- --- 31337/ <= ignored
        // --- --- --- random.toml <= ignored
        // --- --- --- random.zip <= ignored
        // --- --- random_dir_in_broadcast/ <= not ignored
        // --- --- --- random.zip <= not ignored
        // --- --- --- random.toml <= ignored
        // --- --- --- dry_run/ <= ignored
        // --- --- --- --- zip <= ignored
        // --- --- --- --- toml <= ignored

        let random_dir = create_random_directory(&target_dir, "".to_string());
        let broadcast_dir = create_random_directory(&target_dir, "broadcast".to_string());

        let the_31337_dir = create_random_directory(&broadcast_dir, "31337".to_string());
        let random_dir_in_broadcast = create_random_directory(&broadcast_dir, "".to_string());
        let dry_run_dir = create_random_directory(&random_dir_in_broadcast, "dry_run".to_string());

        ignored_files.push(create_random_file(&random_dir, "toml".to_string()));
        filtered_files.push(create_random_file(&random_dir, "zip".to_string()));

        ignored_files.push(create_random_file(&broadcast_dir, "toml".to_string()));
        filtered_files.push(create_random_file(&broadcast_dir, "zip".to_string()));

        ignored_files.push(create_random_file(&the_31337_dir, "toml".to_string()));
        ignored_files.push(create_random_file(&the_31337_dir, "zip".to_string()));

        filtered_files.push(create_random_file(&random_dir_in_broadcast, "zip".to_string()));
        filtered_files.push(create_random_file(&random_dir_in_broadcast, "toml".to_string()));

        ignored_files.push(create_random_file(&dry_run_dir, "zip".to_string()));
        ignored_files.push(create_random_file(&dry_run_dir, "toml".to_string()));

        let ignore_contents_git = r#"
*.toml
!/broadcast
/broadcast/31337/
/broadcast/*/dry_run/
        "#;
        write_to_ignore(&gitignore, ignore_contents_git);

        let result = filter_files_to_copy(&target_dir);

        // for each result we just just to see if a file (not a dir) is in the filtered results
        for res in result {
            if PathBuf::from(&res).is_dir() {
                continue;
            }

            assert!(filtered_files.contains(&res));
        }

        let _ = remove_file(gitignore);
        let _ = remove_dir_all(target_dir);
    }

    #[test]
    #[serial]
    fn zipping_file_structure_check() {
        let target_dir = get_current_working_dir().join("test").join("test_zip");
        let target_dir_unzip = get_current_working_dir().join("test").join("test_unzip");
        let _ = remove_dir_all(&target_dir);
        let _ = remove_dir_all(&target_dir_unzip);
        let _ = create_dir_all(&target_dir);
        let _ = create_dir_all(&target_dir_unzip);

        // File structure that should be preserved
        // - target_dir/
        // --- random_dir_1/
        // --- --- random_dir_2/
        // --- --- --- random_file_3.txt
        // --- --- random_file_2.txt
        // --- random_file_1.txt
        let random_dir_1 = create_random_directory(&target_dir, "".to_string());
        let random_dir_2 = create_random_directory(Path::new(&random_dir_1), "".to_string());
        let random_file_1 = create_random_file(&target_dir, "txt".to_string());
        let random_file_2 = create_random_file(Path::new(&random_dir_1), "txt".to_string());
        let random_file_3 = create_random_file(Path::new(&random_dir_2), "txt".to_string());

        let files_to_copy: Vec<PathBuf> =
            vec![random_file_1.clone(), random_file_3.clone(), random_file_2.clone()];
        let result = match zip_file(&target_dir, &files_to_copy, "test_zip") {
            Ok(r) => r,
            Err(_) => {
                assert_eq!("Invalid State", "");
                return;
            }
        };

        // unzipping for checks
        let archive = read_file(result).unwrap();
        match zip_extract::extract(Cursor::new(archive), &target_dir_unzip, true) {
            Ok(_) => {}
            Err(_) => {
                assert_eq!("Invalid State", "");
            }
        }

        let mut random_file_1_unzipped = target_dir_unzip.clone();
        random_file_1_unzipped.push(random_file_1.strip_prefix(&target_dir).unwrap());
        let mut random_file_2_unzipped = target_dir_unzip.clone();
        random_file_2_unzipped.push(random_file_2.strip_prefix(&target_dir).unwrap());
        let mut random_file_3_unzipped = target_dir_unzip.clone();
        random_file_3_unzipped.push(random_file_3.strip_prefix(&target_dir).unwrap());
        println!("{random_file_3_unzipped:?}");

        assert!(Path::new(&random_file_1_unzipped).exists());
        assert!(Path::new(&random_file_2_unzipped).exists());
        assert!(Path::new(&random_file_3_unzipped).exists());

        //cleaning up
        let _ = remove_dir_all(&target_dir);
        let _ = remove_dir_all(&target_dir_unzip);
    }

    fn define_ignore_file(git: bool) -> PathBuf {
        let mut target = ".soldeerignore";
        if git {
            target = ".gitignore";
        }
        get_current_working_dir().join("test").join(target)
    }

    fn write_to_ignore(target_file: &PathBuf, content: &str) {
        if target_file.exists() {
            let _ = remove_file(target_file);
        }
        let mut file: std::fs::File =
            fs::OpenOptions::new().create_new(true).write(true).open(target_file).unwrap();
        if let Err(e) = write!(file, "{}", content) {
            eprintln!("Couldn't write to the config file: {}", e);
        }
    }

    fn create_random_file(target_dir: &Path, extension: String) -> PathBuf {
        let s: String =
            rand::thread_rng().sample_iter(&Alphanumeric).take(7).map(char::from).collect();
        let target = target_dir.join(format!("random{}.{}", s, extension));
        let mut file: std::fs::File =
            fs::OpenOptions::new().create_new(true).write(true).open(&target).unwrap();
        if let Err(e) = write!(file, "this is a test file") {
            eprintln!("Couldn't write to the config file: {}", e);
        }
        target
    }
    fn create_random_directory(target_dir: &Path, name: String) -> PathBuf {
        let s: String =
            rand::thread_rng().sample_iter(&Alphanumeric).take(7).map(char::from).collect();

        if name.is_empty() {
            let target = target_dir.join(format!("random{}", s));
            let _ = create_dir_all(&target);
            target
        } else {
            let target = target_dir.join(name);
            let _ = create_dir_all(&target);
            target
        }
    }
}
