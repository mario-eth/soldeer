//! Soldeer is a package manager for Solidity projects
use std::env;

use log::Level;
use soldeer_commands::{Args, commands::Parser as _, run};
use yansi::{Condition, Paint as _};

const HAVE_COLOR: Condition = Condition(|| {
    std::env::var_os("NO_COLOR").is_none() &&
        (Condition::CLICOLOR_LIVE)() &&
        Condition::stdouterr_are_tty_live()
});

#[tokio::main]
async fn main() {
    // disable colors if unsupported
    yansi::whenever(HAVE_COLOR);
    let args = Args::parse();
    // setup logging
    if env::var("RUST_LOG").is_ok() {
        env_logger::builder().init();
    } else if let Some(level) = args.verbose.log_level() &&
        level > Level::Error
    {
        // the user requested structured logging (-v[v*])
        // init logger
        env_logger::Builder::new().filter_level(args.verbose.log_level_filter()).init();
    }
    if !args.verbose.is_present() {
        banner();
    }
    if let Err(err) = run(args.command, args.verbose).await {
        eprintln!("{}", err.to_string().red())
    }
}

/// Generate and print a banner
fn banner() {
    println!(
        "{}",
        format!(
            "
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    ╔═╗╔═╗╦  ╔╦╗╔═╗╔═╗╦═╗       Solidity Package Manager
    ╚═╗║ ║║   ║║║╣ ║╣ ╠╦╝
    ╚═╝╚═╝╩═╝═╩╝╚═╝╚═╝╩╚═     github.com/mario-eth/soldeer
           v{}                       soldeer.xyz
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
",
            env!("CARGO_PKG_VERSION")
        )
        .bright_cyan()
    );
}
