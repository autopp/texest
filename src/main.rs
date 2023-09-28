mod ast;
mod error;
mod exec;
mod matcher;
mod parser;
mod reporter;
mod runner;
mod test_case;
mod test_case_expr;
mod validator;

use std::{collections::HashSet, fs::File, io::Write};

use clap::Parser;

use reporter::{Formatter, Reporter};
use runner::run_tests;
use test_case::{TestCaseFile, TestResult};
use test_case_expr::eval;

use crate::parser::parse;

#[derive(Parser)]
struct Args {
    files: Vec<String>,
}

const EXIT_CODE_TEST_FAILED: i32 = 1;
const EXIT_CODE_INVALID_INPUT: i32 = 2;

fn main() {
    let args = Args::parse();

    // Check duplicated filenames
    let mut unique_files = HashSet::<&String>::new();
    let mut duplicated: Vec<String> = vec![];
    args.files.iter().for_each(|filename| {
        if !unique_files.insert(filename) {
            duplicated.push(filename.clone());
        }
    });

    if !duplicated.is_empty() {
        eprintln!("duplicated input files: {}", duplicated.join(", "));
        std::process::exit(EXIT_CODE_INVALID_INPUT);
    }

    let (oks, errs): (Vec<_>, Vec<_>) = args
        .files
        .iter()
        .map(|filename| {
            if filename == "-" {
                parse("<stdin>".to_string(), std::io::stdin())
            } else {
                let file = File::open(filename).unwrap_or_else(|err| {
                    eprintln!("cannot open {}: {}", filename, err);
                    std::process::exit(EXIT_CODE_INVALID_INPUT)
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
        std::process::exit(EXIT_CODE_INVALID_INPUT);
    }

    let status_mr = matcher::new_status_matcher_registry();

    let eval_results = oks
        .iter()
        .map(|ok| {
            let test_case_expr_file = ok.as_ref().unwrap();
            let test_cases = test_case_expr_file
                .test_case_exprs
                .iter()
                .map(|test_case_expr| eval(&status_mr, test_case_expr))
                .collect::<Vec<_>>();
            (test_case_expr_file.filename.clone(), test_cases)
        })
        .collect::<Vec<_>>();

    let errs = eval_results
        .iter()
        .flat_map(|(_, test_cases)| {
            test_cases
                .iter()
                .filter(|test_cases| test_cases.is_err())
                .map(|test_cases| test_cases.as_ref().unwrap_err())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    if !errs.is_empty() {
        errs.iter().for_each(|err| {
            err.violations.iter().for_each(|violation| {
                eprintln!(
                    "{}:{}: {}",
                    violation.filename, violation.path, violation.message
                );
            });
        });
        std::process::exit(EXIT_CODE_INVALID_INPUT);
    }

    let test_case_files = eval_results
        .iter()
        .map(|(filename, results)| {
            let test_cases = results
                .iter()
                .flat_map(|test_case| test_case.as_ref().unwrap())
                .collect::<Vec<_>>();

            TestCaseFile {
                filename: filename.clone(),
                test_cases,
            }
        })
        .collect::<Vec<_>>();

    let mut w: Box<dyn Write> = Box::new(std::io::stdout());
    let mut f: Box<dyn Formatter> = Box::new(reporter::SimpleReporter {});
    let mut r = Reporter::new(&mut w, true, &mut f);

    let results = run_tests(test_case_files, &mut r);

    if !results.iter().all(TestResult::is_passed) {
        std::process::exit(EXIT_CODE_TEST_FAILED)
    }
}
