use std::{ fs::{ File, create_dir, remove_dir_all }, path::PathBuf, io::Read };
use crate::utils::get_current_working_dir;
use std::io::{ Write, Seek };
use std::iter::Iterator;
use walkdir::{ WalkDir, DirEntry };
use zip::write::FileOptions;
use std::path::Path;
use std::process::{ Command, Output };
use std::env;

pub fn zip_version(repository: &String, version: &String) {
    println!("Zipping {}/{}", repository, version);
    let mut zipped: PathBuf = get_current_working_dir().unwrap().join("zipped");
    if !zipped.exists() {
        create_dir(&zipped).unwrap();
    }
    zipped = zipped.join("all_versions");
    if !zipped.exists() {
        create_dir(&zipped).unwrap();
    }
    // we do this in case some repositories are like name/subpath (e.g. @openzeppelin/contracts)
    let source_names: Vec<&str> = repository.split("/").collect();
    let mut source_name: String = repository
        .split("/")
        .collect::<Vec<&str>>()[0]
        .clone()
        .to_string();
    let initial_source_name = source_name.clone();
    if repository.contains("/") {
        for i in 1..source_names.len() {
            source_name.push_str("-");
            source_name.push_str(source_names[i].to_lowercase().as_str());
        }
    }

    let final_zip: PathBuf = zipped.join(format!("{}~{}.zip", &source_name, version));
    let path: &Path = Path::new(&final_zip);
    let file: File = File::create(&path).unwrap();

    let to_zip: String = format!("node_modules/{}", initial_source_name); // TODO this saves as source + version then @openzeppelin...
    let walkdir: WalkDir = WalkDir::new(&to_zip);
    let it: walkdir::IntoIter = walkdir.into_iter();

    zip_dir(&mut it.filter_map(|e| e.ok()), &format!("node_modules/"), file, zip::CompressionMethod::Bzip2).unwrap();

    // removing node modules after zipping
    remove_dir_all(get_current_working_dir().unwrap().join("node_modules/")).unwrap();
}

// simple zip directory that walks through a directory and zips it by adding every file to the zip archive
fn zip_dir<T>(
    it: &mut dyn Iterator<Item = DirEntry>,
    prefix: &str,
    writer: T,
    method: zip::CompressionMethod
) -> zip::result::ZipResult<()>
    where T: Write + Seek
{
    let mut zip: zip::ZipWriter<T> = zip::ZipWriter::new(writer);
    let options: FileOptions = FileOptions::default()
        .compression_method(method)
        .unix_permissions(0o755);

    let mut buffer: Vec<u8> = Vec::new();
    for entry in it {
        let path: &Path = entry.path();
        let name: &Path = path.strip_prefix(Path::new(prefix)).unwrap();

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            // println!("adding file {:?} as {:?} ...", path, name);
            zip.start_file::<&str>(name.to_str().unwrap(), options)?;
            let mut f: File = File::open(path)?;

            f.read_to_end(&mut buffer)?;
            zip.write_all(&*buffer)?;
            buffer.clear();
        } else if name.as_os_str().len() != 0 {
            // Only if not root! Avoids path spec / warning
            // and mapname conversion failed error on unzip
            // println!("adding dir {:?} as {:?} ...", path, name);
            zip.add_directory(name.to_str().unwrap(), options)?;
        }
    }
    zip.finish()?;
    Result::Ok(())
}

pub fn push_to_repository(repository: &String, version: &String) {
    println!("Pushing {}/{} to repository", repository, version);
    let commit_message: String = format!(
        "\"Pushed {} version {} to the repository\"",
        repository,
        version
    );
    let ssh_key = env::var("SOLDEER_SSH_KEY").unwrap();

    let output_add: Output = Command::new("sh")
        .arg("push_to_git.sh")
        .arg(commit_message)
        .arg(ssh_key)
        .output()
        .expect("failed to execute process");
    println!("status: {}", String::from_utf8(output_add.stdout.clone()).unwrap());
    println!("error: {}", String::from_utf8(output_add.stderr.clone()).unwrap());
}

pub fn clean() {
    let zipped: PathBuf = get_current_working_dir().unwrap().join("zipped");
    remove_dir_all(zipped).unwrap();
}
