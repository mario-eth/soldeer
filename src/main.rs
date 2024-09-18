use clap::Parser;
use soldeer::commands::Args;
use yansi::Paint as _;
use ::cfonts::*;

fn main() {
    banr();
    let args = Args::parse();
    match soldeer::run(args.command) {
        Ok(_) => {}
        Err(err) => {
            eprintln!("{}", err.to_string().red())
        }
    }
}

// Bannner function - displays on every command 
pub fn banr() {
    say(Options {
        text: String::from("Soldeer"),
        font: Fonts::FontPallet,
        align: Align::Left,
        colors: vec![Colors::Cyan, Colors::White],
        line_height: 1,
        ..Options::default()
    }) }