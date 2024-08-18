use crate::{
    config::{Dependency, HttpDependency},
    download::Result,
    errors::DownloadError,
    utils::get_base_url,
};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub async fn get_dependency_url_remote(dependency: &Dependency) -> Result<String> {
    let url = format!(
        "{}/api/v1/revision-cli?project_name={}&revision={}",
        get_base_url(),
        dependency.name(),
        dependency.version()
    );
    let req = Client::new().get(url);
    if let Ok(response) = req.send().await {
        if response.status().is_success() {
            let response_text = response.text().await.unwrap();
            let revision = serde_json::from_str::<RevisionResponse>(&response_text);
            if let Ok(revision) = revision {
                if revision.data.is_empty() {
                    return Err(DownloadError::URLNotFound(dependency.to_string()));
                }
                return Ok(revision.data[0].clone().url);
            }
        }
    }
    Err(DownloadError::URLNotFound(dependency.to_string()))
}

//TODO clean this up and do error handling
pub async fn get_project_id(dependency_name: &str) -> Result<String> {
    let url = format!("{}/api/v1/project?project_name={}", get_base_url(), dependency_name);
    let req = Client::new().get(url);
    let get_project_response = req.send().await;

    if let Ok(response) = get_project_response {
        if response.status().is_success() {
            let response_text = response.text().await.unwrap();
            let project = serde_json::from_str::<ProjectResponse>(&response_text);
            match project {
                Ok(project) => {
                    if !project.data.is_empty() {
                        return Ok(project.data[0].id.to_string());
                    }
                }
                Err(_) => {
                    return Err(DownloadError::ProjectNotFound(dependency_name.to_string()));
                }
            }
        }
    }
    Err(DownloadError::ProjectNotFound(dependency_name.to_string()))
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
        return Err(DownloadError::ForgeStdError);
    };
    Ok(Dependency::Http(HttpDependency {
        name: dependency_name.to_string(),
        version: data.clone().version,
        url: Some(data.clone().url),
        checksum: None,
    }))
}

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
