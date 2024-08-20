use crate::{
    errors::AuthError,
    utils::{get_base_url, security_file_path},
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
    let security_file = security_file_path()?;
    let jwt =
        fs::read_to_string(&security_file).map_err(|_| AuthError::MissingToken)?.trim().to_string();
    if jwt.is_empty() {
        return Err(AuthError::MissingToken);
    }
    Ok(jwt)
}

async fn execute_login(login: &Login) -> Result<()> {
    let security_file = security_file_path()?;
    let url = format!("{}/api/v1/auth/login", get_base_url());
    let client = Client::new();
    let res = client.post(&url).json(login).send().await?;
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
    use crate::utils::read_file_to_string;
    use serial_test::serial;
    use std::{env, fs::remove_file};

    #[tokio::test]
    #[serial]
    async fn login_success() {
        let data = r#"
        {
            "status": "200",
            "token": "jwt_token_example"
        }"#;

        // Request a new server from the pool
        let mut server = mockito::Server::new_async().await;
        unsafe {
            // became unsafe in Rust 1.80
            env::set_var("base_url", format!("http://{}", server.host_with_port()));
        }

        // Create a mock
        let _ = server
            .mock("POST", "/api/v1/auth/login")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create();

        match execute_login(&Login {
            email: "test@test.com".to_string(),
            password: "1234".to_string(),
        })
        .await
        {
            Ok(_) => {
                let results = read_file_to_string("./test_save_jwt");
                assert_eq!(results, "jwt_token_example");
                let _ = remove_file("./test_save_jwt");
            }
            Err(_) => {
                assert_eq!("Invalid State", "");
            }
        };
    }

    #[tokio::test]
    #[serial]
    async fn login_401() {
        let mut server = mockito::Server::new_async().await;
        unsafe {
            // became unsafe in Rust 1.80
            env::set_var("base_url", format!("http://{}", server.host_with_port()));
        }

        let data = r#"
        {
            "status": "401",
        }"#;

        let _ = server
            .mock("POST", "/api/v1/auth/login")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create();

        assert!(matches!(
            execute_login(&Login {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            })
            .await,
            Err(AuthError::InvalidCredentials)
        ));
    }

    #[tokio::test]
    #[serial]
    async fn login_500() {
        let mut server = mockito::Server::new_async().await;

        unsafe {
            // became unsafe in Rust 1.80
            env::set_var("base_url", format!("http://{}", server.host_with_port()));
        }

        let data = r#"
        {
            "status": "401",
        }"#;

        let _ = server
            .mock("POST", "/api/v1/auth/login")
            .with_status(500)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create();

        assert!(matches!(
            execute_login(&Login {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            })
            .await,
            Err(AuthError::HttpError(_))
        ));
    }
}
