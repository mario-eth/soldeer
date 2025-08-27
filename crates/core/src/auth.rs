//! Registry authentication
use crate::{errors::AuthError, registry::api_url, utils::login_file_path};
use log::{debug, info, warn};
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION},
    Client, StatusCode,
};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

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

/// Get the JWT token from the environment or from the login file
///
/// Precedence is given to the `SOLDEER_API_TOKEN` environment variable.
pub fn get_token() -> Result<String> {
    if let Ok(token) = env::var("SOLDEER_API_TOKEN") {
        if !token.is_empty() {
            return Ok(token)
        }
    }
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

/// Save an access token in the login file
pub fn save_token(token: &str) -> Result<PathBuf> {
    let token_path = login_file_path()?;
    fs::write(&token_path, token)?;
    Ok(token_path)
}

/// Retrieve user profile for the token to check its validity, returning the username
pub async fn check_token(token: &str) -> Result<String> {
    let client = Client::new();
    let url = api_url("auth/validate-cli-token", &[]);
    let mut headers: HeaderMap = HeaderMap::new();
    let header_value =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| AuthError::InvalidToken)?;
    headers.insert(AUTHORIZATION, header_value);
    let response = client.get(url).headers(headers).send().await?;
    match response.status() {
        s if s.is_success() => {
            #[derive(Deserialize)]
            struct User {
                id: String,
                username: String,
            }
            #[derive(Deserialize)]
            struct UserResponse {
                data: User,
            }
            let res: UserResponse = response.json().await?;
            debug!("token is valid for user {} with ID {}", res.data.username, res.data.id);
            Ok(res.data.username)
        }
        StatusCode::UNAUTHORIZED => Err(AuthError::InvalidToken),
        _ => Err(AuthError::HttpError(
            response.error_for_status().expect_err("result should be an error"),
        )),
    }
}

/// Execute the login request and store the JWT token in the login file
pub async fn execute_login(login: &Credentials) -> Result<PathBuf> {
    warn!("the option to login via email and password will be removed in a future version of Soldeer. Please update your usage by either using `soldeer login --token [YOUR CLI TOKEN]` or passing the `SOLDEER_API_TOKEN` environment variable to the `push` command.");

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
    use temp_env::{async_with_vars, with_var};
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

    #[tokio::test]
    async fn test_check_token_success() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/auth/validate-cli-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"status":"success","data":{"created_at": "2024-08-04T14:21:31.622589Z","email": "test@test.net","id": "b6d56bf0-00a5-474f-b732-f416bef53e92","organization": "test","role": "owner","updated_at": "2024-08-04T14:21:31.622589Z","username": "test","verified": true}}"#,
            )
            .create_async()
            .await;

        let res =
            async_with_vars([("SOLDEER_API_URL", Some(server.url()))], check_token("eyJ0..."))
                .await;
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "test");
    }

    #[tokio::test]
    async fn test_check_token_failure() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/auth/validate-cli-token")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"fail","message":"Invalid token"}"#)
            .create_async()
            .await;

        let res =
            async_with_vars([("SOLDEER_API_URL", Some(server.url()))], check_token("foobar")).await;
        assert!(res.is_err(), "{res:?}");
    }

    #[test]
    fn test_get_token_env() {
        let res = with_var("SOLDEER_API_TOKEN", Some("test"), get_token);
        assert!(res.is_ok(), "{res:?}");
        assert_eq!(res.unwrap(), "test");
    }
}
