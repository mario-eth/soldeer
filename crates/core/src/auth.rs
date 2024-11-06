//! Registry authentication
use crate::{errors::AuthError, registry::api_url, utils::login_file_path};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::fs;

#[cfg(feature = "cli")]
use cliclack::log::{info, success};
#[cfg(feature = "cli")]
use path_slash::PathBufExt as _;
#[cfg(feature = "cli")]
use std::path::PathBuf;

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
    let login_file = login_file_path()?;
    let jwt =
        fs::read_to_string(&login_file).map_err(|_| AuthError::MissingToken)?.trim().to_string();
    if jwt.is_empty() {
        return Err(AuthError::MissingToken);
    }
    Ok(jwt)
}

/// Execute the login request and store the JWT token in the login file
pub async fn execute_login(login: &Credentials) -> std::result::Result<(), AuthError> {
    let security_file = login_file_path()?;
    let url = api_url("auth/login", &[]);
    let client = Client::new();
    let res = client.post(url).json(login).send().await?;
    match res.status() {
        s if s.is_success() => {
            #[cfg(feature = "cli")]
            success("Login successful")?;

            let response: LoginResponse = res.json().await?;
            fs::write(&security_file, response.token)?;

            #[cfg(feature = "cli")]
            info(format!(
                "Login details saved in: {}",
                PathBuf::from_slash_lossy(&security_file).to_string_lossy() /* normalize separators */
            ))?;

            Ok(())
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
