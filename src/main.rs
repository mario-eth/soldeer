use clap::Parser;
use soldeer::commands::Args;

fn main() {
    let args = Args::parse();
    match soldeer::run(args.command) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("{err}");
        }
    }
}
