use std::{fs, path::PathBuf};

use mockito::{Matcher, Mock, ServerGuard};
use soldeer_commands::{commands::login::Login, run, Command, Verbosity};
use temp_env::async_with_vars;
use testdir::testdir;

async fn mock_api_server() -> (ServerGuard, Mock) {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{"status":"success","token": "example_token_jwt"}"#;
    let mock = server
        .mock("POST", "/api/v1/auth/login")
        .match_query(Matcher::Any)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;
    (server, mock)
}

async fn mock_api_server_token() -> (ServerGuard, Mock) {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{"status":"success","data":{"created_at": "2024-08-04T14:21:31.622589Z","email": "test@test.net","id": "b6d56bf0-00a5-474f-b732-f416bef53e92","organization": "test","role": "owner","updated_at": "2024-08-04T14:21:31.622589Z","username": "test","verified": true}}"#;
    let mock = server
        .mock("GET", "/api/v1/auth/validate-cli-token")
        .match_query(Matcher::Any)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;
    (server, mock)
}

#[tokio::test]
async fn test_login_without_prompt_err_400() {
    let cmd: Command = Login::builder().email("test@test.com").password("111111").build().into();
    let res = run(cmd, Verbosity::default()).await;
    assert_eq!(res.unwrap_err().to_string(), "error during login: http error during login: HTTP status client error (400 Bad Request) for url (https://api.soldeer.xyz/api/v1/auth/login)");
}

#[tokio::test]
async fn test_login_without_prompt_success() {
    let (server, mock) = mock_api_server().await;
    let dir = testdir!();
    let login_file: PathBuf = dir.join("test_save_jwt");

    let cmd: Command = Login::builder().email("test@test.com").password("111111").build().into();
    let res = async_with_vars(
        [
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok());
    assert!(login_file.exists());
    assert_eq!(fs::read_to_string(login_file).unwrap(), "example_token_jwt");
    mock.expect(1);
}

#[tokio::test]
async fn test_login_token_success() {
    let (server, mock) = mock_api_server_token().await;
    let dir = testdir!();
    let login_file: PathBuf = dir.join("test_save_jwt");
    let cmd: Command = Login::builder().token("example_token_jwt").build().into();
    let res = async_with_vars(
        [
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(cmd, Verbosity::default()),
    )
    .await;
    assert!(res.is_ok());
    assert!(login_file.exists());
    assert_eq!(fs::read_to_string(login_file).unwrap(), "example_token_jwt");
    mock.expect(1);
}

#[tokio::test]
async fn test_login_token_failure() {
    let cmd: Command = Login::builder().token("asdf").build().into();
    let res = run(cmd, Verbosity::default()).await;
    assert_eq!(res.unwrap_err().to_string(), "error during login: login error: invalid token");
}
