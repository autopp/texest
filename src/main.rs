mod ast;
mod exec;
mod expr;
mod matcher;
mod parser;
mod reporter;
mod runner;
mod test_case;
mod test_case_expr;
mod tmp_dir;
mod validator;

use std::{collections::HashSet, fs::File, io::IsTerminal};

use clap::{Parser, ValueEnum};

use reporter::{Formatter, Reporter};
use runner::run_tests;

use test_case::TestCaseFile;
use test_case_expr::{eval_test_expr, TestExprError};

use crate::parser::parse;

#[derive(Clone, ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}

#[derive(Clone, ValueEnum)]
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

    let (test_case_expr_files, errs) = partition_results(args.files.iter().map(|filename| {
        if filename == "-" {
            parse("<stdin>", std::io::stdin())
        } else {
            let file = File::open(filename).unwrap_or_else(|err| {
                eprintln!("cannot open {}: {}", filename, err);
                std::process::exit(EXIT_CODE_INVALID_INPUT)
            });
            parse(filename, file)
        }
    }));

    if !errs.is_empty() {
        errs.iter().for_each(|err| {
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

    let mut tmp_dir_supplier = tmp_dir::TmpDirFactory::new();

    let (test_case_files, errs): (Vec<TestCaseFile>, Vec<TestExprError>) =
        test_case_expr_files
            .iter()
            .map(|test_case_expr_file| {
                let (test_cases, errs) =
                    partition_results(test_case_expr_file.test_case_exprs.iter().map(
                        |test_case_expr| eval_test_expr(&mut tmp_dir_supplier, test_case_expr),
                    ));

                (
                    TestCaseFile {
                        filename: test_case_expr_file.filename.clone(),
                        test_cases: test_cases.into_iter().flatten().collect(),
                    },
                    errs,
                )
            })
            .fold(
                (Vec::new(), Vec::new()),
                |(mut test_case_files, mut errs), (tcs, es)| {
                    test_case_files.push(tcs);
                    errs.extend(es);
                    (test_case_files, errs)
                },
            );

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

    let use_color = match args.color {
        Color::Auto => std::io::stdout().is_terminal(),
        Color::Always => true,
        Color::Never => false,
    };

    let f = match args.format {
        Format::Simple => Formatter::new_simple(),
        Format::Json => Formatter::new_json(),
    };

    let mut w = std::io::stdout();
    let mut r = Reporter::new(&mut w, use_color, f);

    let result = run_tests(test_case_files, &mut r);

    if let Err(err) = result {
        eprintln!("internal error: {}", err);
        std::process::exit(EXIT_CODE_INTERNAL_ERROR);
    }

    if !result.unwrap().is_all_passed() {
        std::process::exit(EXIT_CODE_TEST_FAILED)
    }
}

fn partition_results<T, E>(results: impl Iterator<Item = Result<T, E>>) -> (Vec<T>, Vec<E>) {
    let mut oks = vec![];
    let mut errs = vec![];

    results.into_iter().for_each(|result| match result {
        Ok(ok) => oks.push(ok),
        Err(err) => errs.push(err),
    });

    (oks, errs)
}
