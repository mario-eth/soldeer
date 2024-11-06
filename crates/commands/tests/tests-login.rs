use std::{fs, path::PathBuf};

use mockito::{Matcher, Mock, ServerGuard};
use reqwest::StatusCode;
use soldeer_commands::{commands::login::Login, run, Command};
use temp_env::async_with_vars;
use testdir::testdir;

async fn mock_api_server(status_code: Option<StatusCode>) -> (ServerGuard, Mock) {
    let mut server = mockito::Server::new_async().await;
    let body = r#"{"status":"success","token": "example_token_jwt"}"#;
    server
        .mock("POST", "/api/v1/auth/login")
        .match_query(Matcher::Any)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;
    let mock = match status_code {
        Some(status_code) => {
            server
                .mock("POST", "/api/v1/auth/login")
                .with_header("content-type", "application/json")
                .with_status(status_code.as_u16() as usize)
                .with_body(r#"{"status":"success","token": "example_token_jwt"}"#)
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
async fn test_login_without_prompt_err_400() {
    let cmd: Command = Login::builder().email("test@test.com").password("111111").build().into();
    let res = run(cmd).await;
    assert_eq!(res.unwrap_err().to_string(), "error during login: http error during login: HTTP status client error (400 Bad Request) for url (https://api.soldeer.xyz/api/v1/auth/login)");
}

#[tokio::test]
async fn test_login_without_prompt_success() {
    let (server, mock) = mock_api_server(None).await;
    let dir = testdir!();
    let login_file: PathBuf = dir.join("test_save_jwt");

    let cmd: Command = Login::builder().email("test@test.com").password("111111").build().into();
    let res = async_with_vars(
        [
            ("SOLDEER_API_URL", Some(server.url())),
            ("SOLDEER_LOGIN_FILE", Some(login_file.to_string_lossy().to_string())),
        ],
        run(cmd),
    )
    .await;
    assert!(res.is_ok());
    assert!(login_file.exists());
    assert_eq!(fs::read_to_string(login_file).unwrap(), "example_token_jwt");
    mock.expect(1);
}
