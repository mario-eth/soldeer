use crate::{
    errors::AuthError,
    utils::{define_security_file_location, get_base_url, read_file},
};
use email_address_parser::{EmailAddress, ParsingOptions};
use reqwest::{Client, StatusCode};
use rpassword::read_password;
use serde_derive::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{self, Write},
};
use yansi::Paint as _;

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
    print!("ℹ️  If you do not have an account, please go to soldeer.xyz to create one.\n📧 Please enter your email: ");
    std::io::stdout().flush().unwrap();
    let mut email = String::new();
    if io::stdin().read_line(&mut email).is_err() {
        return Err(AuthError::InvalidEmail);
    }
    email = match check_email(email) {
        Ok(e) => e,
        Err(err) => return Err(err),
    };

    print!("🔓 Please enter your password: ");
    std::io::stdout().flush().unwrap();
    let password = read_password().unwrap();

    let login: Login = Login { email, password };

    execute_login(login).await.unwrap();
    Ok(())
}

pub fn get_token() -> Result<String> {
    let security_file = define_security_file_location();
    let jwt = read_file(security_file);
    match jwt {
        Ok(token) => Ok(String::from_utf8(token)
            .expect("You are not logged in. Please login using the 'soldeer login' command")),
        Err(_) => Err(AuthError::MissingToken),
    }
}

fn check_email(email_str: String) -> Result<String> {
    let email_str = email_str.trim().to_string().to_ascii_lowercase();

    let email: Option<EmailAddress> =
        EmailAddress::parse(&email_str, Some(ParsingOptions::default()));
    if email.is_none() {
        Err(AuthError::InvalidEmail)
    } else {
        Ok(email_str)
    }
}

async fn execute_login(login: Login) -> Result<()> {
    let url = format!("{}/api/v1/auth/login", get_base_url());
    let req = Client::new().post(url).json(&login);

    let login_response = req.send().await;

    let security_file = define_security_file_location();
    let response = login_response?;

    match response.status() {
        s if s.is_success() => {
            println!("{}", "Login successful".green());
            let jwt = serde_json::from_str::<LoginResponse>(&response.text().await.unwrap())
                .unwrap()
                .token;
            let mut file: std::fs::File = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .append(false)
                .open(&security_file)
                .unwrap();
            write!(file, "{}", &jwt)?;
            println!("{}", format!("Login details saved in: {:?}", &security_file).green());
            Ok(())
        }
        StatusCode::UNAUTHORIZED => Err(AuthError::InvalidCredentials),
        _ => Err(AuthError::HttpError(response.error_for_status().unwrap_err())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::read_file_to_string;
    use serial_test::serial;
    use std::{env, fs::remove_file};

    #[test]
    #[serial]
    fn email_validation() {
        let valid_email = String::from("test@test.com");
        let invalid_email = String::from("test");

        assert_eq!(check_email(valid_email.clone()).unwrap(), valid_email);

        assert!(matches!(check_email(invalid_email), Err(AuthError::InvalidEmail)));
    }

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
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

        // Create a mock
        let _ = server
            .mock("POST", "/api/v1/auth/login")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(data)
            .create();

        match execute_login(Login {
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
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

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
            execute_login(Login {
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
        env::set_var("base_url", format!("http://{}", server.host_with_port()));

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
            execute_login(Login {
                email: "test@test.com".to_string(),
                password: "1234".to_string(),
            })
            .await,
            Err(AuthError::HttpError(_))
        ));
    }
}
