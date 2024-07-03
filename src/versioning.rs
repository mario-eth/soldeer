use crate::auth::get_token;
use crate::errors::PushError;
use crate::remote::get_project_id;
use crate::utils::{
    get_base_url,
    get_current_working_dir,
    read_file,
    read_file_to_string,
};
use reqwest::StatusCode;
use reqwest::{
    header::{
        HeaderMap,
        HeaderValue,
        AUTHORIZATION,
        CONTENT_TYPE,
    },
    multipart::{
        Form,
        Part,
    },
    Client,
};
use std::fs::remove_file;
use std::{
    fs::File,
    io::{
        self,
        Read,
        Write,
    },
    path::{
        Path,
        PathBuf,
    },
};
use walkdir::WalkDir;
use yansi::Paint;
use yash_fnmatch::{
    without_escape,
    Pattern,
};
use zip::{
    write::SimpleFileOptions,
    CompressionMethod,
    ZipWriter,
};

#[derive(Clone, Debug)]
struct FilePair {
    name: String,
    path: String,
}

pub async fn push_version(
    dependency_name: &String,
    dependency_version: &String,
    root_directory_path: PathBuf,
    dry_run: bool,
) -> Result<(), PushError> {
    let file_name = root_directory_path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    println!(
        "{}",
        Paint::green(&format!(
            "Pushing a dependency {}-{}:",
            dependency_name, dependency_version
        ))
    );

    let files_to_copy: Vec<FilePair> = filter_files_to_copy(&root_directory_path);

    let zip_archive = match zip_file(
        dependency_name,
        dependency_version,
        &root_directory_path,
        &files_to_copy,
        &file_name,
    ) {
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

fn zip_file(
    dependency_name: &String,
    dependency_version: &String,
    root_directory_path: &Path,
    files_to_copy: &Vec<FilePair>,
    file_name: &String,
) -> Result<PathBuf, PushError> {
    let root_dir_as_string = root_directory_path.to_str().unwrap();
    let zip_file_path = root_directory_path.join(file_name.to_owned() + ".zip");
    let file = File::create(zip_file_path.to_str().unwrap()).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::DEFLATE);
    if files_to_copy.is_empty() {
        return Err(PushError {
            name: dependency_name.to_string(),
            version: dependency_version.to_string(),
            cause: "No files to push".to_string(),
        });
    }

    for file_path in files_to_copy {
        let file_to_copy = File::open(file_path.path.clone()).unwrap();
        let file_to_copy_name = file_path.name.clone();
        let path = Path::new(&file_path.path);
        let mut buffer = Vec::new();

        // This is the relative path, we basically get the relative path to the target folder that we want to push
        // and zip that as a name so we won't screw up the file/dir hierarchy in the zip file.
        let relative_file_path = file_path.path.to_string().replace(root_dir_as_string, "");

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            match zip.start_file(relative_file_path, options) {
                Ok(_) => {}
                Err(err) => {
                    return Err(PushError {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        cause: format!("Zipping failed. Could not start to zip: {}", err),
                    });
                }
            }
            match io::copy(&mut file_to_copy.take(u64::MAX), &mut buffer) {
                Ok(_) => {}
                Err(err) => {
                    return Err(PushError {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        cause: format!(
                            "Zipping failed, could not read file {} because of the error {}",
                            file_to_copy_name, err
                        ),
                    });
                }
            }
            match zip.write_all(&buffer) {
                Ok(_) => {}
                Err(err) => {
                    return Err(PushError {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                        cause: format!("Zipping failed. Could not write to zip: {}", err),
                    });
                }
            }
        } else if !path.as_os_str().is_empty() {
            let _ = zip.add_directory(&file_path.path, options);
        }
    }
    let _ = zip.finish();
    Ok(zip_file_path)
}

fn filter_files_to_copy(root_directory_path: &Path) -> Vec<FilePair> {
    let ignore_files: Vec<String> = read_ignore_file();

    let root_directory: &str = &(root_directory_path.to_str().unwrap().to_owned() + "/");
    let mut files_to_copy: Vec<FilePair> = Vec::new();
    for entry in WalkDir::new(root_directory)
        .into_iter()
        .filter_map(|e| e.ok())
    {
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

        files_to_copy.push(FilePair {
            name: String::from(entry.path().file_name().unwrap().to_str().unwrap()),
            path: entry.path().to_str().unwrap().to_string(),
        });
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
        let contents = read_file_to_string(&soldeerignore.to_str().unwrap().to_string());
        let current_read_file = contents.lines();
        files.append(&mut escape_lines(current_read_file.collect()));
    }

    if gitignore.exists() {
        let contents = read_file_to_string(&gitignore.to_str().unwrap().to_string());
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
    dependency_name: &String,
    dependency_version: &String,
) -> Result<(), PushError> {
    let token = match get_token() {
        Ok(result) => result,
        Err(err) => {
            return Err(PushError {
                name: (&dependency_name).to_string(),
                version: (&dependency_version).to_string(),
                cause: err.cause,
            });
        }
    };
    let client = Client::new();

    let url = format!("{}/api/v1/revision/upload", get_base_url());

    let mut headers: HeaderMap = HeaderMap::new();

    let header_string = format!("Bearer {}", token);
    let header_value = HeaderValue::from_str(&header_string);

    headers.insert(
        AUTHORIZATION,
        header_value.expect("Could not set auth header"),
    );

    let file_fs = read_file(zip_file).unwrap();
    let mut part =
        Part::bytes(file_fs).file_name(zip_file.file_name().unwrap().to_str().unwrap().to_string());

    // set the mime as app zip
    part = part
        .mime_str("application/zip")
        .expect("Could not set mime type");

    let project_id = match get_project_id(dependency_name).await {
        Ok(id) => id,
        Err(err) => {
            return Err(PushError {
                name: (&dependency_name).to_string(),
                version: (&dependency_version).to_string(),
                cause: err.cause,
            });
        }
    };

    let form = Form::new()
        .text("project_id", project_id)
        .text("revision", dependency_version.clone())
        .part("zip_name", part);

    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&("multipart/form-data; boundary=".to_owned() + form.boundary()))
            .expect("Could not set content type"),
    );
    let res = client
        .post(url)
        .headers(headers.clone())
        .multipart(form)
        .send();

    let response = res.await.unwrap();
    match response.status() {
        StatusCode::OK => println!("{}", Paint::green("Success!")),
        StatusCode::NO_CONTENT => {
            return Err(PushError {
                name: (&dependency_name).to_string(),
                version: (&dependency_version).to_string(),
                cause: "Project not found. Make sure you send the right dependency name.\nThe dependency name is the project name you created on https://soldeer.xyz".to_string(),
            });
        }
        StatusCode::ALREADY_REPORTED => {
            return Err(PushError {
                name: (&dependency_name).to_string(),
                version: (&dependency_version).to_string(),
                cause: "Dependency already exists".to_string(),
            });
        }
        StatusCode::UNAUTHORIZED => {
            return Err(PushError {
                name: (&dependency_name).to_string(),
                version: (&dependency_version).to_string(),
                cause: "Unauthorized. Please login".to_string(),
            });
        }
        StatusCode::PAYLOAD_TOO_LARGE => {
            return Err(PushError {
                name: (&dependency_name).to_string(),
                version: (&dependency_version).to_string(),
                cause: "The package is too big, it has over 50 MB".to_string(),
            });
        }
        _ => {
            return Err(PushError {
                name: (&dependency_name).to_string(),
                version: (&dependency_version).to_string(),
                cause: format!(
                    "The server returned an unexpected error {:?}",
                    response.status()
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{
        self,
        create_dir_all,
        remove_dir_all,
        remove_file,
    };

    use io::Cursor;
    use serial_test::serial;

    use super::*;
    use rand::{
        distributions::Alphanumeric,
        Rng,
    };

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
        assert_eq!(
            String::from(file.file_name().unwrap().to_str().unwrap()),
            result[0].name
        );

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

        let random_dir = PathBuf::from(create_random_directory(&target_dir, "".to_string()));
        let broadcast_dir = PathBuf::from(create_random_directory(
            &target_dir,
            "broadcast".to_string(),
        ));

        let the_31337_dir =
            PathBuf::from(create_random_directory(&broadcast_dir, "31337".to_string()));
        let random_dir_in_broadcast =
            PathBuf::from(create_random_directory(&broadcast_dir, "".to_string()));
        let dry_run_dir = PathBuf::from(create_random_directory(
            &random_dir_in_broadcast,
            "dry_run".to_string(),
        ));

        ignored_files.push(create_random_file(&random_dir, "toml".to_string()));
        filtered_files.push(create_random_file(&random_dir, "zip".to_string()));

        ignored_files.push(create_random_file(&broadcast_dir, "toml".to_string()));
        filtered_files.push(create_random_file(&broadcast_dir, "zip".to_string()));

        ignored_files.push(create_random_file(&the_31337_dir, "toml".to_string()));
        ignored_files.push(create_random_file(&the_31337_dir, "zip".to_string()));

        filtered_files.push(create_random_file(
            &random_dir_in_broadcast,
            "zip".to_string(),
        ));
        filtered_files.push(create_random_file(
            &random_dir_in_broadcast,
            "toml".to_string(),
        ));

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
            if PathBuf::from(&res.path).is_dir() {
                continue;
            }

            assert!(filtered_files.contains(&res.path));
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
        let random_file_1 = create_random_file(&target_dir, ".txt".to_string());
        let random_file_2 = create_random_file(Path::new(&random_dir_1), ".txt".to_string());
        let random_file_3 = create_random_file(Path::new(&random_dir_2), ".txt".to_string());

        let dep_name = "test_dep".to_string();
        let dep_version = "1.1".to_string();
        let files_to_copy: Vec<FilePair> = vec![
            FilePair {
                name: "random_file_1".to_string(),
                path: random_file_1.clone(),
            },
            FilePair {
                name: "random_file_1".to_string(),
                path: random_file_3.clone(),
            },
            FilePair {
                name: "random_file_1".to_string(),
                path: random_file_2.clone(),
            },
        ];
        let result = match zip_file(
            &dep_name,
            &dep_version,
            &target_dir,
            &files_to_copy,
            &"test_zip".to_string(),
        ) {
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

        let random_file_1_unzipped = random_file_1.replace("test_zip", "test_unzip");
        let random_file_2_unzipped = random_file_2.replace("test_zip", "test_unzip");
        let random_file_3_unzipped = random_file_3.replace("test_zip", "test_unzip");

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
        let mut file: std::fs::File = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target_file)
            .unwrap();
        if let Err(e) = write!(file, "{}", content) {
            eprintln!("Couldn't write to the config file: {}", e);
        }
    }

    fn create_random_file(target_dir: &Path, extension: String) -> String {
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        let target = target_dir.join(format!("random{}.{}", s, extension));
        let mut file: std::fs::File = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)
            .unwrap();
        if let Err(e) = write!(file, "this is a test file") {
            eprintln!("Couldn't write to the config file: {}", e);
        }
        String::from(target.to_str().unwrap())
    }
    fn create_random_directory(target_dir: &Path, name: String) -> String {
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();

        if name.is_empty() {
            let target = target_dir.join(format!("random{}", s));
            let _ = create_dir_all(&target);
            return String::from(target.to_str().unwrap());
        } else {
            let target = target_dir.join(name);
            let _ = create_dir_all(&target);
            return String::from(target.to_str().unwrap());
        }
    }
}
