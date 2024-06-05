use crate::errors::DownloadError;
use crate::errors::ProjectNotFound;
use chrono::DateTime;
use chrono::Utc;
use reqwest::Client;
use serde_derive::{
    Deserialize,
    Serialize,
};

pub async fn get_dependency_url_remote(
    dependency_name: &String,
    dependency_version: &String,
) -> Result<String, DownloadError> {
    let url = format!(
        "{}/api/v1/revision-cli?project_name={}&revision={}",
        crate::BASE_URL,
        dependency_name,
        dependency_version
    );
    let req = Client::new().get(url);

    if let Ok(response) = req.send().await {
        if response.status().is_success() {
            let response_text = response.text().await.unwrap();
            let revision = serde_json::from_str::<RevisionResponse>(&response_text);
            if let Ok(revision) = revision {
                if revision.data.is_empty() {
                    return Err(DownloadError {
                        name: dependency_name.to_string(),
                        version: dependency_version.to_string(),
                    });
                }
                return Ok(revision.data[0].clone().url);
            }
        }
    }
    Err(DownloadError {
        name: dependency_name.to_string(),
        version: dependency_version.to_string(),
    })
}
//TODO clean this up and do error handling
pub async fn get_project_id(dependency_name: &String) -> Result<String, ProjectNotFound> {
    let url = format!(
        "{}/api/v1/project?project_name={}",
        crate::BASE_URL,
        dependency_name
    );
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
                    return Err(ProjectNotFound {
                        name: dependency_name.to_string(),
                        cause: "Error from the server or check the internet connection."
                            .to_string(),
                    });
                }
            }
        }
    }
    Err(ProjectNotFound{name: dependency_name.to_string(), cause:"Project not found, please check the dependency name (project name) or create a new project on https://soldeer.xyz".to_string()})
}

pub async fn get_latest_forge_std_dependency() -> Result<String,DownloadError> {
    let dependency_name = "forge-std";
    let url = format!(
        "{}/api/v1/revision?project_name={}&offset=0&limit=1",
        crate::BASE_URL,
        dependency_name
    );
    let req = Client::new().get(url);
    if let Ok(response) = req.send().await {
        if response.status().is_success() {
            let response_text = response.text().await.unwrap();
            let revision = serde_json::from_str::<RevisionResponse>(&response_text);
            if let Ok(revision) = revision {
                if revision.data.is_empty() {
                    return Err(DownloadError {
                        name: dependency_name.to_string(),
                        version: "".to_string(),
                    });
                }
                return Ok(
                    format!("{}~{}", dependency_name, revision.data[0].clone().version)
                );
            }
        }
    }
    Err(DownloadError {
        name: dependency_name.to_string(),
        version: "".to_string(),
    })   
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
