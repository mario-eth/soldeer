use crate::{
    commands::Install,
    config::{
        remappings_foundry, remappings_txt, Dependency, GitDependency, HttpDependency,
        RemappingsAction, RemappingsLocation, SoldeerConfig,
    },
    download::{clone_repo, download_file, unzip_file},
    errors::InstallError,
    lock::LockEntry,
    remote::get_dependency_url_remote,
    utils::{get_url_type, hash_file, hash_folder, run_forge_command, run_git_command},
    DEPENDENCY_DIR,
};
use std::{fs as std_fs, path::Path};
use tokio::{fs, task::JoinSet};
use toml_edit::DocumentMut;

pub type Result<T> = std::result::Result<T, InstallError>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DependencyStatus {
    Missing,
    FailedIntegrity,
    Installed,
}

#[bon::builder]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InstallInfo {
    name: String,
    version: String,
    source: String,
    rev_checksum: Option<String>,
    integrity: Option<String>,
}

impl From<LockEntry> for InstallInfo {
    fn from(lock: LockEntry) -> Self {
        Self {
            name: lock.name,
            version: lock.version,
            source: lock.source,
            rev_checksum: Some(lock.checksum),
            integrity: lock.integrity,
        }
    }
}

pub async fn install_dependencies(
    dependencies: &[Dependency],
    locks: &[LockEntry],
    subdependencies: bool,
) -> Result<Vec<LockEntry>> {
    let mut set = JoinSet::new();
    for dep in dependencies {
        set.spawn({
            let d = dep.clone();
            let lock =
                locks.iter().find(|l| l.name == dep.name() && l.version == dep.version()).cloned();
            async move { install_dependency(&d, lock.as_ref(), subdependencies).await }
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        results.push(res??);
    }
    Ok(results)
}

pub async fn install_dependency(
    dependency: &Dependency,
    lock: Option<&LockEntry>,
    subdependencies: bool,
) -> Result<LockEntry> {
    match lock {
        Some(lock) => {
            match check_dependency_integrity(dependency, lock).await? {
                DependencyStatus::Installed => {
                    // no action needed, dependency is already installed and matches the lockfile
                    // entry
                    return Ok(lock.clone());
                }
                DependencyStatus::FailedIntegrity => match dependency {
                    Dependency::Http(dep) => {
                        // we know the folder exists because otherwise we would have gotten
                        // `Missing`
                        let path = dep.install_path();
                        fs::remove_dir_all(&path)
                            .await
                            .map_err(|e| InstallError::IOError { path, source: e })?;
                    }
                    Dependency::Git(dep) => {
                        reset_git_dependency(dep, lock).await?;
                        // dependency should now be at the correct commit, we can exit
                        return Ok(lock.clone());
                    }
                },
                DependencyStatus::Missing => {
                    // make sure there is no existing directory for the dependency
                    let path = dependency.install_path();
                    if fs::metadata(&path).await.is_ok() {
                        fs::remove_dir_all(&path)
                            .await
                            .map_err(|e| InstallError::IOError { path, source: e })?;
                    }
                }
            }
            install_dependency_inner(
                &lock.clone().into(),
                dependency.install_path(),
                subdependencies,
            )
            .await
        }
        None => {
            // no lockfile entry, install from config object
            // make sure there is no existing directory for the dependency
            let path = dependency.install_path();
            if fs::metadata(&path).await.is_ok() {
                fs::remove_dir_all(&path)
                    .await
                    .map_err(|e| InstallError::IOError { path, source: e })?;
            }
            let url = match dependency.url() {
                Some(url) => url.clone(),
                None => get_dependency_url_remote(dependency).await?,
            };
            let checksum = match &dependency {
                Dependency::Http(_) => None,
                Dependency::Git(dep) => dep.rev.clone(),
            };
            let info = InstallInfo::builder()
                .name(dependency.name())
                .version(dependency.version())
                .source(url)
                .maybe_rev_checksum(checksum)
                .build();
            install_dependency_inner(&info, dependency.install_path(), subdependencies).await
        }
    }
}

pub async fn check_dependency_integrity(
    dependency: &Dependency,
    lock: &LockEntry,
) -> Result<DependencyStatus> {
    match dependency {
        Dependency::Http(http) => check_http_dependency(http, lock).await,
        Dependency::Git(git) => check_git_dependency(git, lock).await,
    }
}

pub fn ensure_dependencies_dir() -> Result<()> {
    let path = DEPENDENCY_DIR.clone();
    if !path.exists() {
        std_fs::create_dir(&path).map_err(|e| InstallError::IOError { path, source: e })?;
    }
    Ok(())
}

pub async fn add_to_remappings(
    dep: Dependency,
    config: &SoldeerConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    if config.remappings_generate {
        if config_path.as_ref().to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(&RemappingsAction::Add(dep), &config_path, config).await?
                }
                RemappingsLocation::Config => {
                    remappings_foundry(&RemappingsAction::Add(dep), &config_path, config).await?
                }
            }
        } else {
            remappings_txt(&RemappingsAction::Add(dep), &config_path, config).await?;
        }
    }
    Ok(())
}

pub async fn update_remappings(
    config: &SoldeerConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    if config.remappings_generate {
        if config_path.as_ref().to_string_lossy().contains("foundry.toml") {
            match config.remappings_location {
                RemappingsLocation::Txt => {
                    remappings_txt(&RemappingsAction::None, &config_path, config).await?
                }
                RemappingsLocation::Config => {
                    remappings_foundry(&RemappingsAction::None, &config_path, config).await?
                }
            }
        } else {
            remappings_txt(&RemappingsAction::None, &config_path, config).await?;
        }
    }
    Ok(())
}

async fn install_dependency_inner(
    dep: &InstallInfo,
    path: impl AsRef<Path>,
    subdependencies: bool,
) -> Result<LockEntry> {
    match get_url_type(&dep.source) {
        crate::utils::UrlType::Git => {
            // if the dependency was specified without a commit hash and we didn't have a lockfile,
            // clone the default branch
            let commit = clone_repo(&dep.source, dep.rev_checksum.as_ref(), &path).await?;
            if subdependencies {
                install_subdependencies(&path).await?;
            }
            Ok(LockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .source(&dep.source)
                .checksum(commit)
                .build())
        }
        crate::utils::UrlType::Http => {
            let zip_path = download_file(&dep.source, path.as_ref().with_extension("zip")).await?;
            let zip_integrity = tokio::task::spawn_blocking({
                let zip_path = zip_path.clone();
                move || hash_file(zip_path)
            })
            .await?
            .map_err(|e| InstallError::IOError { path: zip_path.clone(), source: e })?;
            if let Some(checksum) = &dep.rev_checksum {
                if checksum != &zip_integrity.to_string() {
                    return Err(InstallError::ZipIntegrityError(zip_path.clone()));
                }
            }
            unzip_file(&zip_path).await?;
            if subdependencies {
                install_subdependencies(&path).await?;
            }
            let integrity = hash_folder(&path, None).map_err(|e| InstallError::IOError {
                path: path.as_ref().to_path_buf(),
                source: e,
            })?;
            Ok(LockEntry::builder()
                .name(&dep.name)
                .version(&dep.version)
                .source(&dep.source)
                .checksum(zip_integrity.to_string())
                .integrity(integrity.to_string())
                .build())
        }
    }
}

async fn install_subdependencies(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_path_buf();
    let gitmodules_path = path.join(".gitmodules");
    if fs::metadata(&gitmodules_path).await.is_ok() {
        // clone submodules
        run_git_command(&["submodule", "update", "--init", "--recursive"], Some(&path)).await?;
    }
    // if there is a soldeer.toml file, install the soldeer deps
    let soldeer_config_path = path.join("soldeer.toml");
    if fs::metadata(&soldeer_config_path).await.is_ok() {
        // install subdependencies
        run_forge_command(&["soldeer", "install"], Some(&path)).await?;
        return Ok(());
    }
    // if soldeer deps are defined in the foundry.toml file, install them
    let foundry_path = path.join("foundry.toml");
    if let Ok(contents) = fs::read_to_string(&foundry_path).await {
        if let Ok(doc) = contents.parse::<DocumentMut>() {
            if doc.contains_table("dependencies") {
                run_forge_command(&["soldeer", "install"], Some(&path)).await?;
            }
        }
    }
    Ok(())
}

async fn check_http_dependency(
    dependency: &HttpDependency,
    lock: &LockEntry,
) -> Result<DependencyStatus> {
    let path = dependency.install_path();
    if fs::metadata(&path).await.is_err() {
        return Ok(DependencyStatus::Missing);
    }
    let Some(integrity) = &lock.integrity else {
        return Ok(DependencyStatus::FailedIntegrity);
    };
    let current_hash = tokio::task::spawn_blocking({
        let path = path.clone();
        move || hash_folder(path, None)
    })
    .await?
    .map_err(|e| InstallError::IOError { path, source: e })?;
    if &current_hash.to_string() != integrity {
        return Ok(DependencyStatus::FailedIntegrity);
    }
    Ok(DependencyStatus::Installed)
}

async fn check_git_dependency(
    dependency: &GitDependency,
    lock: &LockEntry,
) -> Result<DependencyStatus> {
    let path = dependency.install_path();
    if fs::metadata(&path).await.is_err() {
        return Ok(DependencyStatus::Missing);
    }
    // check that the location is a git repository
    let top_level = match run_git_command(
        &["rev-parse", "--show-toplevel", path.to_string_lossy().as_ref()],
        Some(&path),
    )
    .await
    {
        Ok(top_level) => top_level.trim().to_string(),
        Err(_) => {
            // error getting the top level directory, assume the directory is not a git repository
            return Ok(DependencyStatus::Missing);
        }
    };
    // compare the top level directory to the install path
    let absolute_path = fs::canonicalize(&path)
        .await
        .map_err(|e| InstallError::IOError { path: path.clone(), source: e })?;
    if top_level.trim() != absolute_path.to_string_lossy() {
        // the top level directory is not the install path, assume the directory is not a git
        // repository
        return Ok(DependencyStatus::Missing);
    }
    // for git dependencies, the `checksum` field holds the commit hash
    match run_git_command(&["diff", "--exit-code", &lock.checksum], Some(&path)).await {
        Ok(_) => Ok(DependencyStatus::Installed),
        Err(_) => Ok(DependencyStatus::FailedIntegrity),
    }
}

/// Reset a git dependency to the commit specified in the lockfile entry
///
/// This function runs `git reset --hard <commit>` and `git clean -fd` in the git dependency's
/// directory
async fn reset_git_dependency(dependency: &GitDependency, lock: &LockEntry) -> Result<()> {
    let path = dependency.install_path();
    run_git_command(&["reset", "--hard", &lock.checksum], Some(&path)).await?;
    run_git_command(&["clean", "-fd"], Some(&path)).await?;
    Ok(())
}
