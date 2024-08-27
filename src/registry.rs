use crate::{
    config::{Dependency, HttpDependency},
    errors::RegistryError,
    utils::api_url,
};
use chrono::{DateTime, Utc};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, RegistryError>;

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RevisionResponse {
    data: Vec<Revision>,
    status: String,
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectResponse {
    data: Vec<Project>,
    status: String,
}

pub async fn get_dependency_url_remote(dependency: &Dependency, version: &str) -> Result<String> {
    let url =
        api_url("revision-cli", &[("project_name", dependency.name()), ("revision", version)]);

    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let revision: RevisionResponse = res.json().await?;
    let Some(r) = revision.data.first() else {
        return Err(RegistryError::URLNotFound(dependency.to_string()));
    };
    Ok(r.url.clone())
}

pub async fn get_project_id(dependency_name: &str) -> Result<String> {
    let url = api_url("project", &[("project_name", dependency_name)]);
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
    let url =
        api_url("revision", &[("project_name", dependency_name), ("offset", "0"), ("limit", "1")]);
    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let revision: RevisionResponse = res.json().await?;
    let Some(data) = revision.data.first() else {
        return Err(RegistryError::URLNotFound(dependency_name.to_string()));
    };
    Ok(HttpDependency {
        name: dependency_name.to_string(),
        version_req: data.clone().version,
        url: None,
    }
    .into())
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Versions {
    Semver(Vec<Version>),
    NonSemver(Vec<String>),
}

/// Get all versions of a dependency sorted in descending order
pub async fn get_all_versions_descending(dependency_name: &str) -> Result<Versions> {
    // TODO: provide a more efficient endpoint which already sorts by descending semver if possible
    // and only returns the version strings
    let url = api_url(
        "revision",
        &[("project_name", dependency_name), ("offset", "0"), ("limit", "10000")],
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

pub async fn get_latest_supported_version(dependency: &Dependency) -> Result<String> {
    match get_all_versions_descending(dependency.name()).await? {
        Versions::Semver(all_versions) => {
            match parse_version_req(dependency.version_req()) {
                Some(req) => {
                    let new_version = all_versions
                        .iter()
                        .find(|version| req.matches(version))
                        .ok_or(RegistryError::NoMatchingVersion {
                            dependency: dependency.name().to_string(),
                            version_req: dependency.version_req().to_string(),
                        })?;
                    Ok(new_version.to_string())
                }
                None => {
                    // we can't check which version is newer, so we just take the latest one
                    Ok(all_versions
                        .into_iter()
                        .next()
                        .map(|v| v.to_string())
                        .expect("there should be at least 1 version"))
                }
            }
        }
        Versions::NonSemver(all_versions) => {
            // we can't check which version is newer, so we just take the latest one
            Ok(all_versions.into_iter().next().expect("there should be at least 1 version"))
        }
    }
}

/// Parse a version requirement string into a `VersionReq`.
///
/// Adds the "equal" operator to the req if it doesn't have an operator.
/// This is necessary because the semver crate considers no operator to be equivalent to the
/// "compatible" operator, but we want to treat it as the "equal" operator.
pub fn parse_version_req(version_req: &str) -> Option<VersionReq> {
    let Ok(mut req) = version_req.parse::<VersionReq>() else {
        return None;
    };
    if req.comparators.is_empty() {
        return None;
    }
    let orig_items: Vec<_> = version_req.split(',').collect();
    // we only perform the operator conversion if we can reference the original string, i.e. if the
    // parsed result has the same number of comparators as the original string
    if orig_items.len() == req.comparators.len() {
        for (comparator, orig) in req.comparators.iter_mut().zip(orig_items.into_iter()) {
            if comparator.op == semver::Op::Caret && !orig.trim_start_matches(' ').starts_with('^')
            {
                comparator.op = semver::Op::Exact;
            }
        }
    }
    Some(req)
}
