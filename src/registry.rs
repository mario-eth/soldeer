use crate::{
    config::{Dependency, HttpDependency},
    errors::RegistryError,
    utils::api_url,
};
use chrono::{DateTime, Utc};
use semver::{Version, VersionReq};
use serde::Deserialize;

pub type Result<T> = std::result::Result<T, RegistryError>;

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Revision {
    pub id: uuid::Uuid,
    pub version: String,
    pub internal_name: String,
    pub url: String,
    pub project_id: uuid::Uuid,
    pub deleted: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RevisionResponse {
    data: Vec<Revision>,
    status: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
        let res =
            async_with_vars([("SOLDEER_API_URL", Some(server.url()))], get_latest_forge_std())
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

        let res = get_all_versions_descending("forge-std").await;
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
}
