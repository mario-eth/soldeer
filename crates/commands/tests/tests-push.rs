use mockito::{Matcher, Mock, ServerGuard};
use reqwest::StatusCode;
use soldeer_commands::{commands::push::Push, run, Command};
use soldeer_core::{errors::PublishError, SoldeerError};
use std::{fs, path::PathBuf};
use temp_env::async_with_vars;
use testdir::testdir;

fn setup_project() -> (PathBuf, PathBuf) {
    let dir = testdir!();
    let login_file = dir.join("test_save_jwt");
    fs::write(&login_file, "jwt_token_example").unwrap();
    let project_path = dir.join("mypkg");
    fs::create_dir(&project_path).unwrap();
    fs::write(project_path.join("foundry.toml"), "[dependencies]\n").unwrap();
    std::env::set_current_dir(&project_path).unwrap();
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
    let (login_file, _) = setup_project();

    let (server, mock) = mock_api_server(None).await;

    let res = async_with_vars(
        [
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Command::Push(Push {
            dependency: "mypkg~0.1.0".to_string(),
            path: None,
            dry_run: false,
            skip_warnings: false,
        })),
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
        run(Command::Push(Push {
            dependency: "mypkg~0.1.0".to_string(),
            path: Some(project_path),
            dry_run: false,
            skip_warnings: false,
        })),
    )
    .await;
    assert!(res.is_ok(), "{res:?}");
    mock.expect(1);
}

#[tokio::test]
async fn test_push_not_found() {
    let (login_file, _) = setup_project();

    let (server, mock) = mock_api_server(Some(StatusCode::NO_CONTENT)).await;

    let res = async_with_vars(
        [
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(Command::Push(Push {
            dependency: "mypkg~0.1.0".to_string(),
            path: None,
            dry_run: false,
            skip_warnings: false,
        })),
    )
    .await;
    assert!(matches!(res, Err(SoldeerError::PublishError(PublishError::ProjectNotFound))));
    mock.expect(1);
}
