//! Registry authentication
use crate::{errors::AuthError, registry::api_url, utils::login_file_path};
use log::{debug, info};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

pub type Result<T> = std::result::Result<T, AuthError>;

/// Credentials to be used for login
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Response from the login endpoint
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoginResponse {
    pub status: String,
    /// JWT token
    pub token: String,
}

/// Get the JWT token from the login file
pub fn get_token() -> Result<String> {
    let token_path = login_file_path()?;
    let jwt =
        fs::read_to_string(&token_path).map_err(|_| AuthError::MissingToken)?.trim().to_string();
    if jwt.is_empty() {
        debug!(token_path:?; "token file exists but is empty");
        return Err(AuthError::MissingToken);
    }
    debug!(token_path:?; "token retrieved from file");
    Ok(jwt)
}

/// Execute the login request and store the JWT token in the login file
pub async fn execute_login(login: &Credentials) -> std::result::Result<PathBuf, AuthError> {
    let token_path = login_file_path()?;
    let url = api_url("auth/login", &[]);
    let client = Client::new();
    let res = client.post(url).json(login).send().await?;
    match res.status() {
        s if s.is_success() => {
            debug!("login request completed");
            let response: LoginResponse = res.json().await?;
            fs::write(&token_path, response.token)?;
            info!(token_path:?; "login successful");
            Ok(token_path)
        }
        StatusCode::UNAUTHORIZED => Err(AuthError::InvalidCredentials),
        _ => Err(AuthError::HttpError(
            res.error_for_status().expect_err("result should be an error"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temp_env::async_with_vars;
    use testdir::testdir;

    #[tokio::test]
    async fn test_login_success() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"200","token":"jwt_token_example"}"#)
            .create_async()
            .await;

        let test_file = testdir!().join("test_save_jwt");
        let res = async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            execute_login(&Credentials {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            }),
        )
        .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(fs::canonicalize(res.unwrap()).unwrap(), fs::canonicalize(&test_file).unwrap());
        assert_eq!(fs::read_to_string(test_file).unwrap(), "jwt_token_example");
    }

    #[tokio::test]
    async fn test_login_401() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"401"}"#)
            .create_async()
            .await;

        let test_file = testdir!().join("test_save_jwt");
        let res = async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            execute_login(&Credentials {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            }),
        )
        .await;
        assert!(matches!(res, Err(AuthError::InvalidCredentials)), "{res:?}");
    }

    #[tokio::test]
    async fn test_login_500() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"500"}"#)
            .create_async()
            .await;

        let test_file = testdir!().join("test_save_jwt");
        let res = async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            execute_login(&Credentials {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            }),
        )
        .await;
        assert!(matches!(res, Err(AuthError::HttpError(_))), "{res:?}");
    }
}
