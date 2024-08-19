mod ast;
mod exec;
mod expr;
mod matcher;
mod parser;
mod reporter;
mod run;
mod test_case;
mod test_case_expr;
mod test_case_runner;
mod tmp_dir;
mod validator;

use std::{collections::HashSet, fs::File, io::IsTerminal};

use clap::{Parser, ValueEnum};

use reporter::{Formatter, Reporter};
use run::run;
use test_case_runner::run_tests;

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

fn main() {
    let args = Args::parse();

    // Check duplicated filenames
    let mut unique_files = HashSet::<&String>::new();
    let mut duplicated: Vec<&str> = vec![];
    let mut inputs: Vec<run::Input> = vec![];
    args.files.iter().for_each(|filename| {
        if unique_files.insert(filename) {
            inputs.push(match filename.as_ref() {
                "-" => run::Input::Stdin,
                _ => run::Input::File(filename.clone()),
            })
        } else {
            duplicated.push(filename);
        }
    });

    if !duplicated.is_empty() {
        eprintln!("duplicated input files: {}", duplicated.join(", "));
        std::process::exit(run::TexestError::InvalidInput.to_exit_status());
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

    if let Err(err) = run(inputs, use_color, f) {
        std::process::exit(err.to_exit_status());
    }
}
