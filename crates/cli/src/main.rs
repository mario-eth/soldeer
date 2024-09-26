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

// Banner edittable 
fn banner() {
    println!(
        "{}",
        "
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
    ╔═╗╔═╗╦  ╔╦╗╔═╗╔═╗╦═╗     Solidity Package Manager 
    ╚═╗║ ║║   ║║║╣ ║╣ ╠╦╝          built in rust
    ╚═╝╚═╝╩═╝═╩╝╚═╝╚═╝╩╚═    github.com/mario-eth/soldeer   
         soldeer.xyz              x.com/m4rio_eth
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
"
        .bright_cyan()
    );
}
