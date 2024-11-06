use clap::Parser;
use cliclack::{input, log::remark};
use email_address_parser::{EmailAddress, ParsingOptions};
use soldeer_core::{
    auth::{execute_login, Credentials},
    Result,
};

/// Log into the central repository to push packages
///
/// The credentials are saved by default into ~/.soldeer.
/// If you want to overwrite that location, use the SOLDEER_LOGIN_FILE env var.
#[derive(Debug, Clone, Default, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Login {
    /// Specify the email without prompting.
    #[arg(long, requires = "password")]
    pub email: Option<String>,

    /// Specify the password without prompting.
    #[arg(long, requires = "email")]
    pub password: Option<String>,
}

pub(crate) async fn login_command(cmd: Login) -> Result<()> {
    if let Some(email) = cmd.email {
        if let Some(password) = cmd.password {
            execute_login(&Credentials { email, password }).await?;
            return Ok(());
        }
    }

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

    execute_login(&Credentials { email, password }).await?;
    Ok(())
}
