use crate::{
    auth::get_token,
    errors::{AuthError, PublishError},
    registry::{api_url, get_project_id},
    utils::read_file,
};
use cliclack::log::{info, remark, success};
use ignore::{WalkBuilder, WalkState};
use path_slash::{PathBufExt, PathExt};
use regex::Regex;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    multipart::{Form, Part},
    Client, StatusCode,
};
use std::{
    fs::{remove_file, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

pub type Result<T> = std::result::Result<T, PublishError>;

pub async fn push_version(
    dependency_name: &str,
    dependency_version: &str,
    root_directory_path: impl AsRef<Path>,
    files_to_copy: &[PathBuf],
    dry_run: bool,
) -> Result<()> {
    let file_name =
        root_directory_path.as_ref().file_name().expect("path should have a last component");

    let zip_archive = match zip_file(&root_directory_path, files_to_copy, file_name) {
        Ok(zip) => zip,
        Err(err) => {
            return Err(err);
        }
    };

    if dry_run {
        info(format!(
            "Zip file created at {}",
            PathBuf::from_slash_lossy(&zip_archive).to_string_lossy()
        ))
        .ok();
        return Ok(());
    }

    if let Err(error) = push_to_repo(&zip_archive, dependency_name, dependency_version).await {
        remove_file(zip_archive.to_str().unwrap()).unwrap();
        return Err(error);
    }

    // deleting zip archive
    let _ = remove_file(zip_archive);

    Ok(())
}

pub fn validate_name(name: &str) -> Result<()> {
    let regex = Regex::new(r"^[@|a-z0-9][a-z0-9-]*[a-z0-9]$").expect("regex should compile");
    if !regex.is_match(name) {
        return Err(PublishError::InvalidName);
    }
    if !(3..=100).contains(&name.len()) {
        return Err(PublishError::InvalidName);
    }
    Ok(())
}

pub fn zip_file(
    root_directory_path: impl AsRef<Path>,
    files_to_copy: &[PathBuf],
    file_name: impl Into<PathBuf>,
) -> Result<PathBuf> {
    let mut file_name: PathBuf = file_name.into();
    file_name.set_extension("zip");
    let zip_file_path = root_directory_path.as_ref().join(file_name);
    let file = File::create(&zip_file_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    if files_to_copy.is_empty() {
        return Err(PublishError::NoFiles);
    }
    let mut added_dirs = Vec::new();

    for file_path in files_to_copy {
        let path = file_path.as_path();
        if !path.is_file() {
            continue;
        }

        // This is the relative path, we basically get the relative path to the target folder
        // that we want to push and zip that as a name so we won't screw up the
        // file/dir hierarchy in the zip file.
        let relative_file_path = file_path.strip_prefix(root_directory_path.as_ref())?;

        // we add folders explicitly to the zip file, some tools might not handle this properly
        // otherwise
        if let Some(parent) = relative_file_path.parent() {
            if !parent.as_os_str().is_empty() && !added_dirs.contains(&parent) {
                zip.add_directory(parent.to_slash_lossy(), options)?;
                added_dirs.push(parent);
            }
        }

        let mut f = File::open(file_path.clone())
            .map_err(|e| PublishError::IOError { path: file_path.clone(), source: e })?;
        let mut buffer = Vec::new();
        zip.start_file(relative_file_path.to_slash_lossy(), options)?;
        f.read_to_end(&mut buffer)
            .map_err(|e| PublishError::IOError { path: file_path.clone(), source: e })?;
        zip.write_all(&buffer)
            .map_err(|e| PublishError::IOError { path: zip_file_path.clone(), source: e })?;
    }
    zip.finish()?;
    Ok(zip_file_path)
}

pub fn filter_files_to_copy(root_directory_path: impl AsRef<Path>) -> Vec<PathBuf> {
    let files_to_copy = Arc::new(Mutex::new(Vec::with_capacity(100)));
    let walker = WalkBuilder::new(root_directory_path)
        .add_custom_ignore_filename(".soldeerignore")
        .hidden(false)
        .filter_entry(|entry| {
            !(entry.path().is_dir() && entry.path().file_name().unwrap_or_default() == ".git")
        })
        .build_parallel();
    walker.run(|| {
        let files_to_copy = Arc::clone(&files_to_copy);
        // function executed for each DirEntry
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            let path = entry.path();
            if path.is_dir() {
                return WalkState::Continue;
            }
            let mut files_to_copy = files_to_copy.lock().expect("mutex should not be poisoned");
            files_to_copy.push(path.to_path_buf());
            WalkState::Continue
        })
    });

    Arc::into_inner(files_to_copy)
        .expect("Arc should have no other strong references")
        .into_inner()
        .expect("mutex should not be poisoned")
}

async fn push_to_repo(
    zip_file: &Path,
    dependency_name: &str,
    dependency_version: &str,
) -> Result<()> {
    let token = get_token()?;
    let client = Client::new();

    let url = api_url("revision/upload", &[]);

    let mut headers: HeaderMap = HeaderMap::new();

    let header_string = format!("Bearer {token}");
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
    let response = client.post(url).headers(headers.clone()).multipart(form).send().await?;
    match response.status() {
        StatusCode::OK => {
            success("Pushed to repository!").ok();
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

// Function to prompt the user for confirmation
pub fn prompt_user_for_confirmation() -> Result<bool> {
    remark("You are about to include some sensitive files in this version").ok();
    info("If you are not sure which files will be included, you can run the command with `--dry-run`and inspect the generated zip file.").ok();

    cliclack::confirm("Do you want to continue?")
        .interact()
        .map_err(|e| PublishError::IOError { path: PathBuf::new(), source: e })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::unzip_file;
    use std::fs;
    use testdir::testdir;

    #[test]
    fn test_validate_name() {
        assert!(validate_name("foo").is_ok());
        assert!(validate_name("test").is_ok());
        assert!(validate_name("test-123").is_ok());
        assert!(validate_name("@test-123").is_ok());

        assert!(validate_name("t").is_err());
        assert!(validate_name("te").is_err());
        assert!(validate_name("@t").is_err());
        assert!(validate_name("test@123").is_err());
        assert!(validate_name("test-123-").is_err());
        assert!(validate_name("foo.bar").is_err());
        assert!(validate_name("mypäckage").is_err());
        assert!(validate_name(&"a".repeat(101)).is_err());
    }

    #[test]
    fn test_filter_files_to_copy() {
        let dir = testdir!();
        // ignore file
        // *.toml
        // !/broadcast
        // /broadcast/31337/
        // /broadcast/*/dry_run/
        fs::write(
            dir.join(".soldeerignore"),
            "*.toml\n!/broadcast\n/broadcast/31337/\n/broadcast/*/dry_run/\n",
        )
        .unwrap();

        let mut ignored = Vec::new();
        let mut included = vec![dir.join(".soldeerignore")];

        // test structure
        // - testdir/
        // --- .soldeerignore <= not ignored
        // --- random_dir/
        // --- --- random.toml <= ignored
        // --- --- random.zip <= not ignored
        // --- broadcast/
        // --- --- random.toml <= ignored
        // --- --- random.zip <= not ignored
        // --- --- 31337/
        // --- --- --- random.toml <= ignored
        // --- --- --- random.zip <= ignored
        // --- --- random_dir_in_broadcast/
        // --- --- --- random.zip <= not ignored
        // --- --- --- random.toml <= ignored
        // --- --- --- dry_run/
        // --- --- --- --- zip <= ignored
        // --- --- --- --- toml <= ignored
        fs::create_dir(dir.join("random_dir")).unwrap();
        fs::create_dir(dir.join("broadcast")).unwrap();
        fs::create_dir(dir.join("broadcast/31337")).unwrap();
        fs::create_dir(dir.join("broadcast/random_dir_in_broadcast")).unwrap();
        fs::create_dir(dir.join("broadcast/random_dir_in_broadcast/dry_run")).unwrap();

        ignored.push(dir.join("random_dir/random.toml"));
        fs::write(ignored.last().unwrap(), "ignored").unwrap();
        included.push(dir.join("random_dir/random.zip"));
        fs::write(included.last().unwrap(), "included").unwrap();
        ignored.push(dir.join("broadcast/random.toml"));
        fs::write(ignored.last().unwrap(), "ignored").unwrap();
        included.push(dir.join("broadcast/random.zip"));
        fs::write(included.last().unwrap(), "included").unwrap();
        ignored.push(dir.join("broadcast/31337/random.toml"));
        fs::write(ignored.last().unwrap(), "ignored").unwrap();
        ignored.push(dir.join("broadcast/31337/random.zip"));
        fs::write(ignored.last().unwrap(), "ignored").unwrap();
        included.push(dir.join("broadcast/random_dir_in_broadcast/random.zip"));
        fs::write(included.last().unwrap(), "included").unwrap();
        ignored.push(dir.join("broadcast/random_dir_in_broadcast/random.toml"));
        fs::write(ignored.last().unwrap(), "ignored").unwrap();
        ignored.push(dir.join("broadcast/random_dir_in_broadcast/dry_run/zip"));
        fs::write(ignored.last().unwrap(), "ignored").unwrap();
        ignored.push(dir.join("broadcast/random_dir_in_broadcast/dry_run/toml"));
        fs::write(ignored.last().unwrap(), "ignored").unwrap();

        let res = filter_files_to_copy(&dir);
        assert_eq!(res.len(), included.len());
        for r in res {
            assert!(included.contains(&r));
        }
    }

    #[tokio::test]
    async fn test_zip_file() {
        let dir = testdir!().join("test_zip");
        fs::create_dir(&dir).unwrap();
        let mut files = Vec::new();
        files.push(dir.join("a.txt"));
        fs::write(files.last().unwrap(), "test").unwrap();
        files.push(dir.join("b.txt"));
        fs::write(files.last().unwrap(), "test").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        files.push(dir.join("sub/c.txt"));
        fs::write(files.last().unwrap(), "test").unwrap();
        fs::create_dir(dir.join("sub/sub")).unwrap();
        files.push(dir.join("sub/sub/d.txt"));
        fs::write(files.last().unwrap(), "test").unwrap();
        fs::create_dir(dir.join("empty")).unwrap();

        let res = zip_file(&dir, &files, "test");
        assert!(res.is_ok(), "{res:?}");

        fs::copy(dir.join("test.zip"), testdir!().join("test.zip")).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        fs::create_dir(&dir).unwrap();
        unzip_file(testdir!().join("test.zip"), &dir).await.unwrap();
        for f in files {
            assert!(f.exists());
        }
    }
}
