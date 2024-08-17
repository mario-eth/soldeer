use crate::download::IntegrityChecksum;

pub fn unzip_dependency(dependency: &HttpDependency) -> Result<IntegrityChecksum> {
    let file_name =
        sanitize_dependency_name(&format!("{}-{}", dependency.name, dependency.version));
    let target_name = format!("{}/", file_name);
    let zip_path = DEPENDENCY_DIR.join(format!("{file_name}.zip"));
    let target_dir = DEPENDENCY_DIR.join(target_name);
    let zip_contents = read_file(&zip_path).unwrap();

    zip_extract::extract(Cursor::new(zip_contents), &target_dir, true)?;
    println!("{}", format!("The dependency {dependency} was unzipped!").green());

    hash_folder(&target_dir, Some(zip_path))
        .map_err(|e| DownloadError::IOError { path: target_dir, source: e })
}
