use crate::auth::get_token;
use crate::remote::get_project_id;
use crate::utils::{
    get_current_working_dir,
    read_file,
    read_file_to_string,
};
use std::{
    fmt,
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
    process::exit,
};

use walkdir::WalkDir;

use zip::{
    write::FileOptions,
    CompressionMethod,
    ZipWriter,
};

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
use serde_derive::Deserialize;
use std::fs::remove_file;
#[derive(Clone, Debug)]
struct FilePair {
    name: String,
    path: String,
}

pub async fn push_version(
    dependency_name: String,
    dependency_version: String,
    root_directory_path: PathBuf,
) -> Result<(), PushError> {
    let file_name = root_directory_path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    println!(
        "Pushing dependency {}-{}",
        dependency_name, dependency_version
    );

    let files_to_copy: Vec<FilePair> = filter_filles_to_copy(&root_directory_path);
    let zip_archive = zip_file(&root_directory_path, &files_to_copy, &file_name).unwrap();
    match push_to_repo(&zip_archive, dependency_name, dependency_version).await {
        Ok(_) => {}
        Err(error) => {
            remove_file(zip_archive.to_str().unwrap()).unwrap();
            println!("{}", error.message);
            exit(500);
        }
    }

    Ok(())
}

fn zip_file(
    root_directory_path: &Path,
    files_to_copy: &Vec<FilePair>,
    file_name: &String,
) -> Result<PathBuf, PushError> {
    let zip_file_path = root_directory_path.join(file_name.to_owned() + ".zip");
    let file = File::create(zip_file_path.to_str().unwrap()).unwrap();

    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(CompressionMethod::DEFLATE);
    if files_to_copy.is_empty() {
        return Err(PushError {
            message: "No files to push".to_string(),
        });
    }
    for file_path in files_to_copy {
        let file = File::open(&file_path.path.clone()).unwrap();
        let file_name = file_path.name.clone();
        let path = Path::new(&file_path.path);
        let mut buffer = Vec::new();

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            let _ = zip.start_file(&file_name, options);
            let _ = io::copy(&mut file.take(u64::MAX), &mut buffer);
            let _ = zip.write_all(&buffer);
        } else if !path.as_os_str().is_empty() {
            let _ = zip.add_directory(&file_name, options);
        }
    }
    let _ = zip.finish();
    Ok(zip_file_path)
}

fn filter_filles_to_copy(root_directory_path: &Path) -> Vec<FilePair> {
    let ignore_files: Vec<String> = read_ignore_file();

    let root_directory: &str = &(root_directory_path.to_str().unwrap().to_owned() + "/");
    let mut files_to_copy: Vec<FilePair> = Vec::new();
    for entry in WalkDir::new(root_directory)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let file_name = entry
            .path()
            .to_str()
            .unwrap()
            .to_string()
            .replace(root_directory, "");
        if file_name.is_empty() {
            continue;
        }
        let mut found: bool = false;
        for ignore_file in ignore_files.iter() {
            if file_name.contains(ignore_file) {
                found = true;
                break;
            }
        }
        if found {
            continue;
        }
        files_to_copy.push(FilePair {
            name: file_name,
            path: entry.path().to_str().unwrap().to_string(),
        });
    }

    files_to_copy
}

fn read_ignore_file() -> Vec<String> {
    let ignore_file = get_current_working_dir().unwrap().join(".soldeerignore");
    if !ignore_file.exists() {
        return Vec::new();
    }

    let file_contents = read_file_to_string(&ignore_file.to_str().unwrap().to_string());

    let mut ignore_list: Vec<String> = Vec::new();
    for line in file_contents.lines() {
        ignore_list.push(line.to_string());
    }
    ignore_list
}

async fn push_to_repo(
    zip_file: &Path,
    dependency_name: String,
    dependency_version: String,
) -> Result<(), PushError> {
    let token = get_token();
    let client = Client::new();

    let url = format!("{}/api/v1/revision/upload", crate::BASE_URL);

    let mut headers: HeaderMap = HeaderMap::new();

    let header_string = format!("Bearer {}", token);
    let header_value = HeaderValue::from_str(&header_string);

    headers.insert(
        AUTHORIZATION,
        header_value.expect("Could not set auth header"),
    );

    let file_fs = read_file(zip_file.to_str().unwrap().to_string()).unwrap();
    let mut part =
        Part::bytes(file_fs).file_name(zip_file.file_name().unwrap().to_str().unwrap().to_string());

    // set the mime as app zip
    part = part
        .mime_str("application/zip")
        .expect("Could not set mime type");
    let project_id = get_project_id(&dependency_name).await;
    let form = Form::new()
        .text("project_id", project_id)
        .text("revision", dependency_version)
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
    if response.status() != 200 {
        let push_error: PushResponseError =
            serde_json::from_str(response.text().await.unwrap().as_str()).unwrap();
        return Err(PushError {
            message: format!("Could not push dependency: {}", push_error.message),
        });
    } else {
        println!("Successfully pushed dependency");
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PushError {
    message: String,
}

impl fmt::Display for PushError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "push failed")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushResponseError {
    pub message: String,
    pub status: String,
}

impl fmt::Display for PushResponseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "push failed with status {} and message {}",
            self.status, self.message
        )
    }
}
