mod error;
mod exec;
mod matcher;
mod parser;
mod test_case;
mod test_case_expr;
mod validator;

use std::{fs::File, process::Command};

use clap::Parser;
use exec::execute_command;
use test_case_expr::eval;

use crate::parser::parse;

#[derive(Parser)]
struct Args {
    files: Vec<String>,
}

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let args = Args::parse();

    let (oks, errs): (Vec<_>, Vec<_>) = args
        .files
        .iter()
        .map(|filename| {
            if filename == "-" {
                parse("<stdin>".to_string(), std::io::stdin())
            } else {
                let file = File::open(filename).unwrap_or_else(|err| {
                    eprintln!("cannot open {}: {}", filename, err);
                    std::process::exit(2)
                });
                parse(filename.clone(), file)
            }
        })
        .partition(Result::is_ok);

    if !errs.is_empty() {
        errs.iter().for_each(|err| {
            let err = err.as_ref().unwrap_err();
            eprintln!("{}: {}", err.filename, err.message);
            err.violations.iter().for_each(|violation| {
                eprintln!(
                    "{}:{}: {}",
                    violation.filename, violation.path, violation.message
                );
            });
        });
        std::process::exit(2);
    }

    let status_mr = matcher::new_status_matcher_registry();

    let (oks, errs): (Vec<_>, Vec<_>) = oks
        .iter()
        .flat_map(|ok| {
            let test_case_exprs = ok.as_ref().unwrap();
            test_case_exprs
                .iter()
                .map(|test_case_expr| eval(&status_mr, test_case_expr))
                .collect::<Vec<_>>()
        })
        .partition(Result::is_ok);

    if !errs.is_empty() {
        errs.iter().for_each(|err| {
            let err = err.as_ref().unwrap_err();
            eprintln!("{}: {}", err.filename, err.message);
            err.violations.iter().for_each(|violation| {
                eprintln!(
                    "{}:{}: {}",
                    violation.filename, violation.path, violation.message
                );
            });
        });
        std::process::exit(2);
    }

    let test_cases = oks.iter().flat_map(|ok| ok.as_ref().unwrap());

    let mut results = test_cases.map(|test_case| {
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
}
