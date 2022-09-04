mod error;
mod parser;
mod test_case;
mod validator;

use std::{fs::File, process::Command};

use clap::Parser;

use crate::parser::parse;

#[derive(Parser)]
struct Args {
    files: Vec<String>,
}

fn main() {
    let args = Args::parse();
    args.files.iter().for_each(|filename| {
        let test_cases = if filename == "-" {
            parse("<stdin>".to_string(), std::io::stdin())
        } else {
            let file = File::open(filename).unwrap_or_else(|err| {
                eprintln!("cannot open {}: {}", filename, err);
                std::process::exit(2)
            });
            parse(filename.clone(), file)
        }
        .unwrap_or_else(|err| {
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
