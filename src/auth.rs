use email_address_parser::{
    EmailAddress,
    ParsingOptions,
};

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
    process::exit,
};

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

pub async fn login() {
    print!("ℹ️  If you do not have an account, please go to soldeer.xyz to create one.\n📧 Please enter your email: ");
    std::io::stdout().flush().unwrap();
    let mut email = String::new();
    if io::stdin().read_line(&mut email).is_err() {
        println!("Invalid email");
        exit(500);
    }
    email = email.trim().to_string().to_ascii_lowercase();

    let email: Option<EmailAddress> = EmailAddress::parse(&email, Some(ParsingOptions::default()));
    if email.is_none() {
        eprintln!("Invalid email");
        exit(500);
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
    match login_response {
        Ok(response) => {
            if response.status().is_success() {
                println!("Login successful");
                let jwt = serde_json::from_str::<LoginResponse>(&response.text().await.unwrap())
                    .unwrap()
                    .token;
                let mut file: std::fs::File = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(false)
                    .open(&security_file)
                    .unwrap();
                if let Err(e) = write!(file, "{}", &jwt) {
                    eprintln!("Couldn't write to security file{}: {}", &security_file, e);
                }
                println!("Login details saved in: {:?}", &security_file);
            } else {
                if response.status().as_u16() == 401 {
                    println!("Authentication failed. Invalid email or password");
                    exit(500);
                }
                println!("Authentication failed. {}", response.status());
                exit(500);
            }
        }
        Err(error) => {
            println!("Login failed {}", error);
            exit(500);
        }
    }
}

pub fn get_token() -> String {
    let security_file = define_security_file_location();
    let jwt = read_file(security_file);
    match jwt {
        Ok(token) => {
            String::from_utf8(token)
                .expect("You are not logged in. Please login using the 'soldeer login' command")
        }
        Err(_) => {
            println!("You are not logged in. Please login using the 'soldeer login' command");
            exit(500);
        }
    }
}
