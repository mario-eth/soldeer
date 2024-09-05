use crate::{errors::AuthError, utils::login_file_path};
use serde::{Deserialize, Serialize};
use std::fs;

#[cfg(feature = "cli")]
use crate::registry::api_url;
#[cfg(feature = "cli")]
use cliclack::{
    input,
    log::{info, remark, success},
};
#[cfg(feature = "cli")]
use email_address_parser::{EmailAddress, ParsingOptions};
#[cfg(feature = "cli")]
use path_slash::PathBufExt as _;
#[cfg(feature = "cli")]
use reqwest::{Client, StatusCode};
#[cfg(feature = "cli")]
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, AuthError>;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Login {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoginResponse {
    pub status: String,
    pub token: String,
}

#[cfg(feature = "cli")]
pub async fn login() -> Result<()> {
    #[cfg(feature = "cli")]
    remark("If you do not have an account, please visit soldeer.xyz to create one.")?;

    let email: String = input("Email address")
        .validate(|input: &String| {
            if input.is_empty() {
                Err("Email is required")
            } else {
                match EmailAddress::parse(input, Some(ParsingOptions::default())) {
                    None => Err("Invalid email address"),
                    Some(_) => Ok(()),
                }
            }
        })
        .interact()?;

    let password = cliclack::password("Password").mask('▪').interact()?;

    execute_login(&Login { email, password }).await?;
    Ok(())
}

pub fn get_token() -> Result<String> {
    let security_file = login_file_path()?;
    let jwt =
        fs::read_to_string(&security_file).map_err(|_| AuthError::MissingToken)?.trim().to_string();
    if jwt.is_empty() {
        return Err(AuthError::MissingToken);
    }
    Ok(jwt)
}

#[cfg(feature = "cli")]
async fn execute_login(login: &Login) -> Result<()> {
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
        _ => Err(AuthError::HttpError(res.error_for_status().unwrap_err())),
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
            execute_login(&Login {
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
            execute_login(&Login {
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
            execute_login(&Login {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            }),
        )
        .await;
        assert!(matches!(res, Err(AuthError::HttpError(_))), "{res:?}");
    }
}
