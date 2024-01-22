extern crate soldeer_lib;

use crate::soldeer_lib::commands::Args;
use clap::Parser;

pub fn main() {
    let args = Args::parse();
    soldeer_lib::run(args);
}
