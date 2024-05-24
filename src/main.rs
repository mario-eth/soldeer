use clap::Parser;
use soldeer::commands::Args;
use yansi::Paint;

fn main() {
    let args = Args::parse();
    match soldeer::run(args.command) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("{}", Paint::red(&err.message))
        }
    }
}
