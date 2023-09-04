mod error;
mod parser;
mod test_case;
mod validator;

use std::process::Command;

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
        let test_cases = parse(input).unwrap_or_else(|err| {
            eprintln!("cannot parse {:?}", err);
            std::process::exit(1);
        });

        let mut results = test_cases.iter().map(|test_case| {
            Command::new(test_case.command.get(0).unwrap())
                .args(test_case.command.get(1..).unwrap())
                .output()
                .map_or(false, |output| output.status.success())
        });

        if !results.all(|result| result) {
            std::process::exit(1)
        }
    });
}
