mod ast;
mod error;
mod exec;
mod expr;
mod matcher;
mod parser;
mod reporter;
mod runner;
mod test_case;
mod test_case_expr;
mod validator;

use std::{
    collections::HashSet,
    fs::File,
    io::{IsTerminal, Write},
};

use clap::{Parser, ValueEnum};

use reporter::{Formatter, Reporter};
use runner::run_tests;
use test_case::TestCaseFile;
use test_case_expr::eval_test_expr;

use crate::parser::parse;

#[derive(Debug, Clone, ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, ValueEnum)]
enum Format {
    Simple,
    Json,
}

#[derive(Parser)]
struct Args {
    files: Vec<String>,
    #[clap(value_enum, long = "color", default_value_t = Color::Auto)]
    color: Color,
    #[clap(value_enum, long = "format", default_value_t = Format::Simple)]
    format: Format,
}

const EXIT_CODE_TEST_FAILED: i32 = 1;
const EXIT_CODE_INVALID_INPUT: i32 = 2;
const EXIT_CODE_INTERNAL_ERROR: i32 = 3;

fn main() {
    let args = Args::parse();

    // Check duplicated filenames
    let mut unique_files = HashSet::<&String>::new();
    let mut duplicated: Vec<&str> = vec![];
    args.files.iter().for_each(|filename| {
        if !unique_files.insert(filename) {
            duplicated.push(filename);
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
                parse("<stdin>", std::io::stdin())
            } else {
                let file = File::open(filename).unwrap_or_else(|err| {
                    eprintln!("cannot open {}: {}", filename, err);
                    std::process::exit(EXIT_CODE_INVALID_INPUT)
                });
                parse(filename, file)
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
    let stream_mr = matcher::new_stream_matcher_registry();

    let eval_results = oks
        .iter()
        .map(|ok| {
            let test_case_expr_file = ok.as_ref().unwrap();
            let test_cases = test_case_expr_file
                .test_case_exprs
                .iter()
                .map(|test_case_expr| eval_test_expr(&status_mr, &stream_mr, test_case_expr))
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

    let use_color = match args.color {
        Color::Auto => std::io::stdout().is_terminal(),
        Color::Always => true,
        Color::Never => false,
    };

    let mut f: Box<dyn Formatter> = match args.format {
        Format::Simple => Box::new(reporter::SimpleFormatter {}),
        Format::Json => Box::new(reporter::JsonFormatter {}),
    };

    let mut w: Box<dyn Write> = Box::new(std::io::stdout());
    let mut r = Reporter::new(&mut w, use_color, &mut f);

    let result = run_tests(test_case_files, &mut r);

    if let Err(err) = result {
        eprintln!("internal error: {}", err);
        std::process::exit(EXIT_CODE_INTERNAL_ERROR);
    }

    if !result.unwrap().is_all_passed() {
        std::process::exit(EXIT_CODE_TEST_FAILED)
    }
}
