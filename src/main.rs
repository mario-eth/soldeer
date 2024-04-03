extern crate soldeer;
use yansi::Paint;

use crate::soldeer::commands::Args;
use clap::Parser;

pub fn main() {
    let args = Args::parse();
    match soldeer::run(args.command) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("{}", Paint::red(err.message))
        }
    }
}
