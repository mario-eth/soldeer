extern crate soldeer_lib;
use yansi::Paint;

use crate::soldeer_lib::commands::Args;
use clap::Parser;

pub fn main() {
    let args = Args::parse();
    match soldeer_lib::run(args) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("{}", Paint::red(err.message))
        }
    }
}
