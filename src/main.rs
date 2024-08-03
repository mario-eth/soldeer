use clap::Parser;
use soldeer::commands::Args;
use yansi::Paint as _;

fn main() {
    let args = Args::parse();
    match soldeer::run(args.command) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("{}", err.to_string().red())
        }
    }
}
