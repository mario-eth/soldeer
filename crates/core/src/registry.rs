//! Soldeer registry client.
//!
//! The registry client is responsible for fetching information about packages from the Soldeer
//! registry at <https://soldeer.xyz>.
use crate::{
    config::{Dependency, HttpDependency},
    errors::RegistryError,
};
use chrono::{DateTime, Utc};
use log::{debug, warn};
use reqwest::Url;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::env;

pub type Result<T> = std::result::Result<T, RegistryError>;

/// A revision (version) for a project (package).
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Revision {
    /// The unique ID for the revision.
    pub id: uuid::Uuid,

    /// The version of the revision.
    pub version: String,

    /// The internal name (path of zip file) for the revision.
    pub internal_name: String,

    /// The zip file download URL.
    pub url: String,

    /// The project unique ID.
    pub project_id: uuid::Uuid,

    /// Whether this revision has been deleted.
    pub deleted: bool,

    /// Creation date for the revision.
    pub created_at: Option<DateTime<Utc>>,
}

/// A project (package) in the registry.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Project {
    /// The unique ID for the project.
    pub id: uuid::Uuid,

    /// The name of the project.
    pub name: String,

    /// The description of the project.
    pub description: String,

    /// The URL of the repository on GitHub.
    pub github_url: String,

    /// The unique ID for the owner of the project.
    pub user_id: uuid::Uuid,

    /// Whether this project has been deleted.
    pub deleted: Option<bool>,

    /// The project's creation datetime.
    pub created_at: Option<DateTime<Utc>>,

    /// The project's last update datetime.
    pub updated_at: Option<DateTime<Utc>>,
}

/// The response from the revision endpoint.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RevisionResponse {
    /// The revisions.
    data: Vec<Revision>,

    /// The status of the response.
    status: String,
}

/// The response from the project endpoint.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ProjectResponse {
    /// The projects.
    data: Vec<Project>,

    /// The status of the response.
    status: String,
}

/// Construct a URL for the Soldeer API.
///
/// The URL is constructed from the `SOLDEER_API_URL` environment variable, or defaults to
/// <https://api.soldeer.xyz>. The API version prefix and path are appended to the base URL,
/// and any query parameters are URL-encoded and appended to the URL.
///
/// # Examples
///
/// ```
/// # use soldeer_core::registry::api_url;
/// let url =
///     api_url("revision", &[("project_name", "forge-std"), ("offset", "0"), ("limit", "1")]);
/// assert_eq!(
///     url.as_str(),
///     "https://api.soldeer.xyz/api/v1/revision?project_name=forge-std&offset=0&limit=1"
/// );
/// ```
pub fn api_url(path: &str, params: &[(&str, &str)]) -> Url {
    let url = env::var("SOLDEER_API_URL").unwrap_or("https://api.soldeer.xyz".to_string());
    let mut url = Url::parse(&url).expect("SOLDEER_API_URL is invalid");
    url.set_path(&format!("api/v1/{path}"));
    if params.is_empty() {
        return url;
    }
    url.query_pairs_mut().extend_pairs(params.iter());
    url
}

/// Get the download URL for a dependency at a specific version.
pub async fn get_dependency_url_remote(dependency: &Dependency, version: &str) -> Result<String> {
    debug!(dep:% = dependency; "retrieving URL for dependency");
    let url =
        api_url("revision-cli", &[("project_name", dependency.name()), ("revision", version)]);

    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let revision: RevisionResponse = res.json().await?;
    let Some(r) = revision.data.first() else {
        return Err(RegistryError::URLNotFound(dependency.to_string()));
    };
    debug!(dep:% = dependency, url = r.url; "URL for dependency was found");
    Ok(r.url.clone())
}

/// Get the unique ID for a project by name.
pub async fn get_project_id(dependency_name: &str) -> Result<String> {
    debug!(name = dependency_name; "retrieving project ID");
    let url = api_url("project", &[("project_name", dependency_name)]);
    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let project: ProjectResponse = res.json().await?;
    let Some(p) = project.data.first() else {
        return Err(RegistryError::ProjectNotFound(dependency_name.to_string()));
    };
    debug!(name = dependency_name, id:% = p.id; "project ID was found");
    Ok(p.id.to_string())
}

/// Get the latest version of a dependency.
pub async fn get_latest_version(dependency_name: &str) -> Result<Dependency> {
    debug!(dep = dependency_name; "retrieving latest version for dependency");
    let url =
        api_url("revision", &[("project_name", dependency_name), ("offset", "0"), ("limit", "1")]);
    let res = reqwest::get(url).await?;
    let res = res.error_for_status()?;
    let revision: RevisionResponse = res.json().await?;
    let Some(data) = revision.data.first() else {
        return Err(RegistryError::URLNotFound(dependency_name.to_string()));
    };
    debug!(dep = dependency_name, version = data.version; "latest version found");
    Ok(HttpDependency {
        name: dependency_name.to_string(),
        version_req: data.clone().version,
        url: None,
    }
    .into())
}

/// The versions of a dependency.
///
/// If all versions can be parsed as semver, then the versions are sorted in descending order
/// according to semver. If not all versions can be parsed as semver, then the versions are returned
/// in the order they were received from the API (descending creation date).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Versions {
    /// All versions are semver compliant.
    Semver(Vec<Version>),

    /// Not all versions are semver compliant.
    NonSemver(Vec<String>),
}

/// Get all versions of a dependency sorted in descending order
///
/// If all versions can be parsed as semver, then the versions are sorted in descending order
/// according to semver. If not all versions can be parsed as semver, then the versions are returned
/// in the order they were received from the API (descending creation date).
pub async fn get_all_versions_descending(dependency_name: &str) -> Result<Versions> {
    // TODO: provide a more efficient endpoint which already sorts by descending semver if possible
    // and only returns the version strings
    debug!(dep = dependency_name; "retrieving all dependency versions");
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
            debug!(dep = dependency_name; "all versions are semver compliant, sorting by descending version");
            versions.sort_unstable_by(|a, b| b.cmp(a)); // sort in descending order
            Ok(Versions::Semver(versions))
        }
        Err(_) => {
            debug!(dep = dependency_name; "not all versions are semver compliant, using API ordering");
            Ok(Versions::NonSemver(revision.data.iter().map(|r| r.version.to_string()).collect()))
        }
    }
}

/// Get the latest version of a dependency that satisfies the version requirement.
///
/// If the API response contains non-semver-compliant versions, then we attempt to find an exact
/// match for the requirement, or error out.
pub async fn get_latest_supported_version(dependency: &Dependency) -> Result<String> {
    debug!(dep:% = dependency, version_req = dependency.version_req(); "retrieving latest version according to version requirement");
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
                    debug!(dep:% = dependency, version:% = new_version; "acceptable version found");
                    Ok(new_version.to_string())
                }
                None => {
                    warn!(dep:% = dependency, version_req = dependency.version_req(); "could not parse version req according to semver, using latest version");
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
            // try to find the exact version specifier in the list of all versions, otherwise error
            // out
            debug!(dep:% = dependency; "versions are not all semver compliant, trying to find exact match");
            all_versions.into_iter().find(|v| v == dependency.version_req()).ok_or_else(|| {
                RegistryError::NoMatchingVersion {
                    dependency: dependency.name().to_string(),
                    version_req: dependency.version_req().to_string(),
                }
            })
        }
    }
}

/// Parse a version requirement string into a `VersionReq`.
///
/// Adds the "equal" operator to the req if it doesn't have an operator.
/// This is necessary because the [`semver`] crate considers no operator to be equivalent to the
/// "compatible" operator, but we want to treat it as the "equal" operator.
pub fn parse_version_req(version_req: &str) -> Option<VersionReq> {
    let Ok(mut req) = version_req.parse::<VersionReq>() else {
        debug!(version_req; "version requirement cannot be parsed by semver");
        return None;
    };
    if req.comparators.is_empty() {
        debug!(version_req; "comparators list is empty (wildcard req), no further action needed");
        return Some(req); // wildcard/any version
    }
    let orig_items: Vec<_> = version_req.split(',').collect();
    // we only perform the operator conversion if we can reference the original string, i.e. if the
    // parsed result has the same number of comparators as the original string

    if orig_items.len() == req.comparators.len() {
        for (comparator, orig) in req.comparators.iter_mut().zip(orig_items.into_iter()) {
            if comparator.op == semver::Op::Caret && !orig.trim_start_matches(' ').starts_with('^')
            {
                debug!(comparator:% = comparator; "adding exact operator for comparator");
                comparator.op = semver::Op::Exact;
            }
        }
    }
    Some(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};
    use temp_env::async_with_vars;

    #[tokio::test]
    async fn test_get_dependency_url() {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[{"created_at":"2024-08-06T17:31:25.751079Z","deleted":false,"downloads":3391,"id":"660132e6-4902-4804-8c4b-7cae0a648054","internal_name":"forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","version":"1.9.2"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision-cli")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;

        let dependency =
            HttpDependency::builder().name("forge-std").version_req("^1.9.0").build().into();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            get_dependency_url_remote(&dependency, "1.9.2"),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip");
    }

    #[tokio::test]
    async fn test_get_dependency_url_nomatch() {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision-cli")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;

        let dependency =
            HttpDependency::builder().name("forge-std").version_req("^1.9.0").build().into();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            get_dependency_url_remote(&dependency, "1.9.2"),
        )
        .await;
        assert!(matches!(res, Err(RegistryError::URLNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_project_id() {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[{"created_at":"2024-02-27T19:19:23.938837Z","deleted":false,"description":"Forge Standard Library is a collection of helpful contracts and libraries for use with Forge and Foundry.","downloads":67634,"github_url":"https://github.com/foundry-rs/forge-std","id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","image":"https://soldeer-resources.s3.amazonaws.com/default_icon.png","long_description":"Forge Standard Library is a collection of helpful contracts and libraries for use with Forge and Foundry. It leverages Forge's cheatcodes to make writing tests easier and faster, while improving the UX of cheatcodes.","name":"forge-std","updated_at":"2024-02-27T19:19:23.938837Z","user_id":"96228bb5-f777-4c19-ba72-363d14b8beed"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/project")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;
        let res =
            async_with_vars([("SOLDEER_API_URL", Some(server.url()))], get_project_id("forge-std"))
                .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "37adefe5-9bc6-4777-aaf2-e56277d1f30b");
    }

    #[tokio::test]
    async fn test_get_project_id_nomatch() {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/project")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;

        let res =
            async_with_vars([("SOLDEER_API_URL", Some(server.url()))], get_project_id("forge-std"))
                .await;
        assert!(matches!(res, Err(RegistryError::ProjectNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_latest_forge_std() {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[{"created_at":"2024-08-06T17:31:25.751079Z","deleted":false,"downloads":3391,"id":"660132e6-4902-4804-8c4b-7cae0a648054","internal_name":"forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","version":"1.9.2"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;

        let dependency =
            HttpDependency::builder().name("forge-std").version_req("1.9.2").build().into();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            get_latest_version("forge-std"),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), dependency);
    }

    #[tokio::test]
    async fn test_get_all_versions_descending() {
        let mut server = Server::new_async().await;
        // data is not sorted in reverse semver order
        let data = r#"{"data":[{"created_at":"2024-07-03T14:44:58.148723Z","deleted":false,"downloads":21,"id":"b463683a-c4b4-40bf-b707-1c4eb343c4d2","internal_name":"forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","version":"1.9.0"},{"created_at":"2024-08-06T17:31:25.751079Z","deleted":false,"downloads":3389,"id":"660132e6-4902-4804-8c4b-7cae0a648054","internal_name":"forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","version":"1.9.2"},{"created_at":"2024-07-03T14:44:59.729623Z","deleted":false,"downloads":5290,"id":"fa5160fc-ba7b-40fd-8e99-8becd6dadbe4","internal_name":"forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","version":"1.9.1"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;

        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            get_all_versions_descending("forge-std"),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(
            res.unwrap(),
            Versions::Semver(vec![
                "1.9.2".parse().unwrap(),
                "1.9.1".parse().unwrap(),
                "1.9.0".parse().unwrap()
            ])
        );
    }

    #[tokio::test]
    async fn test_get_latest_supported_version_semver() {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[{"created_at":"2024-08-06T17:31:25.751079Z","deleted":false,"downloads":3389,"id":"660132e6-4902-4804-8c4b-7cae0a648054","internal_name":"forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","version":"1.9.2"},{"created_at":"2024-07-03T14:44:59.729623Z","deleted":false,"downloads":5290,"id":"fa5160fc-ba7b-40fd-8e99-8becd6dadbe4","internal_name":"forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","version":"1.9.1"},{"created_at":"2024-07-03T14:44:58.148723Z","deleted":false,"downloads":21,"id":"b463683a-c4b4-40bf-b707-1c4eb343c4d2","internal_name":"forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","version":"1.9.0"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;

        let dependency: Dependency =
            HttpDependency::builder().name("forge-std").version_req("^1.9.0").build().into();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            get_latest_supported_version(&dependency),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "1.9.2");
    }

    #[tokio::test]
    async fn test_get_latest_supported_version_no_semver() {
        let mut server = Server::new_async().await;
        let data = r#"{"data":[{"created_at":"2024-08-06T17:31:25.751079Z","deleted":false,"downloads":3389,"id":"660132e6-4902-4804-8c4b-7cae0a648054","internal_name":"forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/1_9_2_06-08-2024_17:31:25_forge-std-1.9.2.zip","version":"2024-08"},{"created_at":"2024-07-03T14:44:59.729623Z","deleted":false,"downloads":5290,"id":"fa5160fc-ba7b-40fd-8e99-8becd6dadbe4","internal_name":"forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_1_03-07-2024_14:44:59_forge-std-v1.9.1.zip","version":"2024-07"},{"created_at":"2024-07-03T14:44:58.148723Z","deleted":false,"downloads":21,"id":"b463683a-c4b4-40bf-b707-1c4eb343c4d2","internal_name":"forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","project_id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","url":"https://soldeer-revisions.s3.amazonaws.com/forge-std/v1_9_0_03-07-2024_14:44:57_forge-std-v1.9.0.zip","version":"2024-06"}],"status":"success"}"#;
        server
            .mock("GET", "/api/v1/revision")
            .match_query(Matcher::Any)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create_async()
            .await;

        let dependency: Dependency =
            HttpDependency::builder().name("forge-std").version_req("2024-06").build().into();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            get_latest_supported_version(&dependency),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "2024-06"); // should resolve to the exact match

        let dependency: Dependency =
            HttpDependency::builder().name("forge-std").version_req("non-existant").build().into();
        let res = async_with_vars(
            [("SOLDEER_API_URL", Some(server.url()))],
            get_latest_supported_version(&dependency),
        )
        .await;
        assert!(matches!(res, Err(RegistryError::NoMatchingVersion { .. })));
    }

    #[test]
    fn test_parse_version_req() {
        assert_eq!(parse_version_req("1.9.0"), Some(VersionReq::parse("=1.9.0").unwrap()));
        assert_eq!(parse_version_req("=1.9.0"), Some(VersionReq::parse("=1.9.0").unwrap()));
        assert_eq!(parse_version_req("^1.9.0"), Some(VersionReq::parse("^1.9.0").unwrap()));
        assert_eq!(
            parse_version_req("^1.9.0,^1.10.0"),
            Some(VersionReq::parse("^1.9.0, ^1.10.0").unwrap())
        );
        assert_eq!(
            parse_version_req("1.9.0,1.10.0"),
            Some(VersionReq::parse("=1.9.0,=1.10.0").unwrap())
        );
        assert_eq!(parse_version_req(">=1.9.0"), Some(VersionReq::parse(">=1.9.0").unwrap()));
        assert_eq!(parse_version_req(""), None);
        assert_eq!(parse_version_req("foobar"), None);
        assert_eq!(parse_version_req("*"), Some(VersionReq::STAR));
    }
}
