//! Soldeer is a package manager for Solidity projects
use soldeer_commands::{commands::Parser as _, run, Args};
use yansi::Paint as _;

#[tokio::main]
async fn main() {
    banner();
    let args = Args::parse();
    if let Err(err) = run(args.command).await {
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
