use soldeer_commands::{commands::Parser as _, run, Args};
use yansi::Paint as _;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if let Err(err) = run(args.command).await {
        eprintln!("{}", err.to_string().red())
    }
}
