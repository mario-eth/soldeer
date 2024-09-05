//! Soldeer is a package manager for Solidity projects
use owo_colors::{OwoColorize, Stream::Stderr};
use soldeer_commands::{commands::Parser as _, run, Args};

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(err) = run(args.command).await {
        eprintln!("{}", err.to_string().if_supports_color(Stderr, |t| t.red()))
    }
}
