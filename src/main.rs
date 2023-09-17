mod error;
mod exec;
mod matcher;
mod parser;
mod test_case;
mod validator;

use std::{fs::File, process::Command};

use clap::Parser;
use exec::execute_command;

use crate::parser::parse;

#[derive(Parser)]
struct Args {
    files: Vec<String>,
}

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
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
            std::process::exit(2);
        });

        let mut results = test_cases.iter().map(|test_case| {
            Command::new(test_case.command.get(0).unwrap())
                .args(test_case.command.get(1..).unwrap())
                .output()
                .map_or(false, |output| output.status.success());

            rt.block_on(execute_command(
                test_case.command.clone(),
                test_case.stdin.clone(),
                test_case.timeout,
            ))
            .map(|output| {
                if test_case.tee_stdout {
                    println!("{}", output.stdout);
                }
                if test_case.tee_stderr {
                    println!("{}", output.stderr);
                }
                output
            })
        });

        if !results.all(|result| {
            result
                .map(|output| matches!(output.status, exec::Status::Exit(0)))
                .unwrap_or(false)
        }) {
            std::process::exit(1)
        }
    });
}
