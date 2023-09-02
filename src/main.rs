mod error;
mod parser;

use clap::Parser;

use crate::parser::parse;

#[derive(Parser)]
struct Args {
    files: Vec<String>,
}

fn main() {
    let args = Args::parse();
    args.files.iter().for_each(|file| {
        let input = if file == "-" {
            parser::Input::Stdin
        } else {
            parser::Input::File(file.clone())
        };
        parse(input).unwrap_or_else(|err| {
            eprintln!("cannot parse {:?}", err);
            std::process::exit(1);
        });
    });
}
