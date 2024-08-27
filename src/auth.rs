use crate::{
    errors::AuthError,
    utils::{api_url, login_file_path},
};
use cliclack::log::{info, remark, success};
use email_address_parser::{EmailAddress, ParsingOptions};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::fs;

pub type Result<T> = std::result::Result<T, AuthError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Login {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub status: String,
    pub token: String,
}

pub async fn login() -> Result<()> {
    remark("If you do not have an account, please visit soldeer.xyz to create one.")?;
    let email: String = cliclack::input("Email address")
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

async fn execute_login(login: &Login) -> Result<()> {
    let security_file = login_file_path()?;
    let url = api_url("auth/login", &[]);
    let client = Client::new();
    let res = client.post(url).json(login).send().await?;
    match res.status() {
        s if s.is_success() => {
            success("Login successful")?;
            let response: LoginResponse = res.json().await?;
            fs::write(&security_file, response.token)?;
            info(format!("Login details saved in: {:?}", &security_file))?;
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
        let data = r#"
        {
            "status": "200",
            "token": "jwt_token_example"
        }"#;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create();

        let test_file = testdir!().join("test_save_jwt");
        async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            async move {
                println!("env var: {:?}", std::env::var("SOLDEER_LOGIN_FILE"));
                let res = execute_login(&Login {
                    email: "test@test.com".to_string(),
                    password: "1234".to_string(),
                })
                .await;
                if let Err(err) = res {
                    panic!("Error: {:?}", err);
                }
                assert!(res.is_ok(), "{res:?}");
                assert_eq!(fs::read_to_string(test_file).unwrap(), "jwt_token_example");
            },
        )
        .await;
    }

    #[tokio::test]
    async fn test_login_401() {
        let mut server = mockito::Server::new_async().await;
        let data = r#"{ "status": "401" }"#;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create();

        let test_file = testdir!().join("test_save_jwt");
        async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            async move {
                let res = execute_login(&Login {
                    email: "test@test.com".to_string(),
                    password: "1234".to_string(),
                })
                .await;
                assert!(matches!(res, Err(AuthError::InvalidCredentials)), "{res:?}");
            },
        )
        .await;
    }

    #[tokio::test]
    async fn test_login_500() {
        let mut server = mockito::Server::new_async().await;
        let data = r#"{ "status": "500" }"#;
        server
            .mock("POST", "/api/v1/auth/login")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create();

        let test_file = testdir!().join("test_save_jwt");
        async_with_vars(
            [
                ("SOLDEER_API_URL", Some(server.url())),
                ("SOLDEER_LOGIN_FILE", Some(test_file.to_string_lossy().to_string())),
            ],
            async move {
                let res = execute_login(&Login {
                    email: "test@test.com".to_string(),
                    password: "1234".to_string(),
                })
                .await;
                assert!(matches!(res, Err(AuthError::HttpError(_))), "{res:?}");
            },
        )
        .await;
    }
}
