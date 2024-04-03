use email_address_parser::{
    EmailAddress,
    ParsingOptions,
};
use yansi::Paint;

use crate::utils::{
    define_security_file_location,
    read_file,
};
use reqwest::Client;
use rpassword::read_password;
use serde_derive::{
    Deserialize,
    Serialize,
};
use std::{
    fs::OpenOptions,
    io::{
        self,
        Write,
    },
};

use crate::errors::LoginError;

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

pub async fn login() -> Result<(), LoginError> {
    print!("ℹ️  If you do not have an account, please go to soldeer.xyz to create one.\n📧 Please enter your email: ");
    std::io::stdout().flush().unwrap();
    let mut email = String::new();
    if io::stdin().read_line(&mut email).is_err() {
        return Err(LoginError {
            cause: "Invalid email".to_string(),
        });
    }
    email = email.trim().to_string().to_ascii_lowercase();

    let email: Option<EmailAddress> = EmailAddress::parse(&email, Some(ParsingOptions::default()));
    if email.is_none() {
        return Err(LoginError {
            cause: "Invalid email".to_string(),
        });
    }
    print!("🔓 Please enter your password: ");
    std::io::stdout().flush().unwrap();
    let password = read_password().unwrap();

    let login: Login = Login {
        email: email.unwrap().to_string(),
        password,
    };

    let url = format!("{}/api/v1/auth/login", crate::BASE_URL);
    let req = Client::new().post(url).json(&login);

    let login_response = req.send().await;
    let security_file = define_security_file_location();
    if let Ok(response) = login_response {
        if response.status().is_success() {
            println!("{}", Paint::green("Login successful"));
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
            if let Err(e) = write!(file, "{}", &jwt) {
                return Err(LoginError {
                    cause: format!(
                        "Couldn't write to the security file {}: {}",
                        &security_file, e
                    ),
                });
            }
            println!(
                "{}",
                Paint::green(format!("Login details saved in: {:?}", &security_file))
            );

            return Ok(());
        } else if response.status().as_u16() == 401 {
            return Err(LoginError {
                cause: "Authentication failed. Invalid email or password".to_string(),
            });
        }
    }
    Err(LoginError {
        cause: "Authentication failed. Unknown error.".to_string(),
    })
}

pub fn get_token() -> Result<String, LoginError> {
    let security_file = define_security_file_location();
    let jwt = read_file(security_file);
    match jwt {
        Ok(token) => {
            Ok(String::from_utf8(token)
                .expect("You are not logged in. Please login using the 'soldeer login' command"))
        }
        Err(_) => {
            Err(LoginError {
                cause: "You are not logged in. Please login using the 'login' command".to_string(),
            })
        }
    }
}
