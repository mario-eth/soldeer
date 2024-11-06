use mockito::{Matcher, Mock, ServerGuard};
use reqwest::StatusCode;
use soldeer_commands::{commands::push::Push, run};
use soldeer_core::{errors::PublishError, SoldeerError};
use std::{env, fs, path::PathBuf};
use temp_env::async_with_vars;
use testdir::testdir;

#[allow(clippy::unwrap_used)]
fn setup_project(dotfile: bool) -> (PathBuf, PathBuf) {
    let dir = testdir!();
    let login_file: PathBuf = dir.join("test_save_jwt");
    fs::write(&login_file, "jwt_token_example").unwrap();
    let project_path = dir.join("mypkg");
    fs::create_dir(&project_path).unwrap();
    fs::write(project_path.join("foundry.toml"), "[dependencies]\n").unwrap();
    if dotfile {
        fs::write(project_path.join(".env"), "super-secret-stuff").unwrap();
    }
    (login_file, project_path)
}

async fn mock_api_server(status_code: Option<StatusCode>) -> (ServerGuard, Mock) {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{"data":[{"created_at":"2024-02-27T19:19:23.938837Z","deleted":false,"description":"","downloads":67634,"github_url":"","id":"37adefe5-9bc6-4777-aaf2-e56277d1f30b","image":"","long_description":"","name":"mock","updated_at":"2024-02-27T19:19:23.938837Z","user_id":"96228bb5-f777-4c19-ba72-363d14b8beed"}],"status":"success"}"#;
    server
        .mock("GET", "/api/v1/project")
        .match_query(Matcher::Any)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;
    let mock = match status_code {
        Some(status_code) => {
            server
                .mock("POST", "/api/v1/revision/upload")
                .with_header("content-type", "application/json")
                .with_status(status_code.as_u16() as usize)
                .with_body(r#"{"status":"fail","message": "failure"}"#)
                .create_async()
                .await
        }
        None => {
            server
                .mock("POST", "/api/v1/revision/upload")
                .with_header("content-type", "application/json")
                .with_body(r#"{"status":"success","data":{"data":{"project_id":"mock"}}}"#)
                .create_async()
                .await
        }
    };

    (server, mock)
}

#[tokio::test]
async fn test_push_success() {
    let (login_file, project_path) = setup_project(false);

    let (server, mock) = mock_api_server(None).await;

    env::set_current_dir(&project_path).unwrap();
    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder().dependency("mypkg~0.1.0").build().into()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.expect(1);
}

#[tokio::test]
async fn test_push_other_dir_success() {
    let dir = testdir!();
    fs::write(dir.join("foundry.toml"), "[dependencies]\n").unwrap();
    let login_file = dir.join("test_save_jwt");
    fs::write(&login_file, "jwt_token_example").unwrap();
    let project_path = dir.join("mypkg");
    fs::create_dir(&project_path).unwrap();
    fs::write(project_path.join("test.sol"), "contract Foo {}\n").unwrap();

    let (server, mock) = mock_api_server(None).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(dir.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder().dependency("mypkg~0.1.0").path(project_path).build().into()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.expect(1);
}

#[tokio::test]
async fn test_push_not_found() {
    let (login_file, project_path) = setup_project(false);

    let (server, mock) = mock_api_server(Some(StatusCode::NO_CONTENT)).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder().dependency("mypkg~0.1.0").path(project_path).build().into()),
    )
    .await;
    assert!(matches!(res, Err(SoldeerError::PublishError(PublishError::ProjectNotFound))));
    mock.expect(1);
}

#[tokio::test]
async fn test_push_already_exists() {
    let (login_file, project_path) = setup_project(false);

    let (server, mock) = mock_api_server(Some(StatusCode::ALREADY_REPORTED)).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder().dependency("mypkg~0.1.0").path(project_path).build().into()),
    )
    .await;
    assert!(matches!(res, Err(SoldeerError::PublishError(PublishError::AlreadyExists))));
    mock.expect(1);
}

#[tokio::test]
async fn test_push_unauthorized() {
    let (login_file, project_path) = setup_project(false);

    let (server, mock) = mock_api_server(Some(StatusCode::UNAUTHORIZED)).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder().dependency("mypkg~0.1.0").path(project_path).build().into()),
    )
    .await;
    assert!(matches!(res, Err(SoldeerError::PublishError(PublishError::AuthError(_)))));
    mock.expect(1);
}

#[tokio::test]
async fn test_push_payload_too_large() {
    let (login_file, project_path) = setup_project(false);

    let (server, mock) = mock_api_server(Some(StatusCode::PAYLOAD_TOO_LARGE)).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder().dependency("mypkg~0.1.0").path(project_path).build().into()),
    )
    .await;
    assert!(matches!(res, Err(SoldeerError::PublishError(PublishError::PayloadTooLarge))));
    mock.expect(1);
}

#[tokio::test]
async fn test_push_other_error() {
    let (login_file, project_path) = setup_project(false);

    let (server, mock) = mock_api_server(Some(StatusCode::INTERNAL_SERVER_ERROR)).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder().dependency("mypkg~0.1.0").path(project_path).build().into()),
    )
    .await;
    assert!(matches!(res, Err(SoldeerError::PublishError(PublishError::HttpError(_)))));
    mock.expect(1);
}

#[tokio::test]
async fn test_push_dry_run() {
    let (login_file, project_path) = setup_project(true); // insert a .env file

    let (server, mock) = mock_api_server(None).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder()
            .dependency("mypkg~0.1.0")
            .path(&project_path)
            .dry_run(true)
            .build()
            .into()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.expect(0);
    assert!(project_path.join("mypkg.zip").exists());
}

#[tokio::test]
async fn test_push_skip_warnings() {
    let (login_file, project_path) = setup_project(true); // insert a .env file

    let (server, mock) = mock_api_server(None).await;

    let res = async_with_vars(
        [
            ("SOLDEER_PROJECT_ROOT", Some(project_path.to_string_lossy().to_string())),
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Push::builder()
            .dependency("mypkg~0.1.0")
            .path(&project_path)
            .skip_warnings(true)
            .build()
            .into()),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.expect(1);
}
