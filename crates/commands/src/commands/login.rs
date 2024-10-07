use clap::Parser;
use cliclack::{input, log::remark};
use email_address_parser::{EmailAddress, ParsingOptions};
use soldeer_core::{
    auth::{execute_login, Credentials},
    Result,
};

/// Log into the central repository to push packages
#[derive(Debug, Clone, Default, Parser)]
#[clap(after_help = "For more information, read the README.md")]
pub struct Login {}

pub(crate) async fn login_command() -> Result<()> {
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
