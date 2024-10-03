//! Soldeer is a package manager for Solidity projects
use soldeer_commands::{commands::Parser as _, run, Args};
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
