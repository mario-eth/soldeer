use crate::{
    config::{Dependency, HttpDependency},
    errors::RegistryError,
    utils::get_base_url,
};
use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, RegistryError>;

#[allow(non_snake_case)]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Revision {
    pub id: uuid::Uuid,
    pub version: String,
    pub internal_name: String,
    pub url: String,
    pub project_id: uuid::Uuid,
    pub deleted: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct Project {
    pub id: uuid::Uuid,
    pub name: String,
    pub description: String,
    pub github_url: String,
    pub user_id: uuid::Uuid,
    pub deleted: Option<bool>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RevisionResponse {
    data: Vec<Revision>,
    status: String,
}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProjectResponse {
    data: Vec<Project>,
    status: String,
}

pub async fn get_dependency_url_remote(dependency: &Dependency) -> Result<String> {
    let url = format!(
        "{}/api/v1/revision-cli?project_name={}&revision={}",
        get_base_url(),
        dependency.name(),
        dependency.version()
    );
    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let revision: RevisionResponse = res.json().await?;
    let Some(r) = revision.data.first() else {
        return Err(RegistryError::URLNotFound(dependency.to_string()));
    };
    Ok(r.url.clone())
}

pub async fn get_project_id(dependency_name: &str) -> Result<String> {
    let url = format!("{}/api/v1/project?project_name={}", get_base_url(), dependency_name);
    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let project: ProjectResponse = res.json().await?;
    let Some(p) = project.data.first() else {
        return Err(RegistryError::ProjectNotFound(dependency_name.to_string()));
    };
    Ok(p.id.to_string())
}

pub async fn get_latest_forge_std() -> Result<Dependency> {
    let dependency_name = "forge-std";
    let url = format!(
        "{}/api/v1/revision?project_name={}&offset=0&limit=1",
        get_base_url(),
        dependency_name
    );
    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let revision: RevisionResponse = res.json().await?;
    let Some(data) = revision.data.first() else {
        return Err(RegistryError::URLNotFound(dependency_name.to_string()));
    };
    Ok(Dependency::Http(HttpDependency {
        name: dependency_name.to_string(),
        version: data.clone().version,
        url: Some(data.clone().url),
        checksum: None,
    }))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Versions {
    Semver(Vec<Version>),
    NonSemver(Vec<String>),
}

/// Get all versions of a dependency sorted in descending order
pub async fn get_all_versions_descending(dependency_name: &str) -> Result<Versions> {
    // TODO: provide a more efficient endpoint which already sorts by descending semver if possible
    // and only returns the version strings
    let url = format!(
        "{}/api/v1/revision?project_name={}&offset=0&limit=10000",
        get_base_url(),
        dependency_name
    );
    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let revision: RevisionResponse = res.json().await?;
    if revision.data.is_empty() {
        return Err(RegistryError::NoVersion(dependency_name.to_string()));
    }

    match revision
        .data
        .iter()
        .map(|r| Version::parse(&r.version))
        .collect::<std::result::Result<Vec<Version>, _>>()
    {
        Ok(mut versions) => {
            // all versions are semver compliant
            versions.sort_unstable_by(|a, b| b.cmp(a)); // sort in descending order
            Ok(Versions::Semver(versions))
        }
        Err(_) => {
            // not all versions are semver compliant, do not sort (use API sort order)
            Ok(Versions::NonSemver(revision.data.iter().map(|r| r.version.to_string()).collect()))
        }
    }
}
