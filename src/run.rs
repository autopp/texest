use std::fs::File;

use crate::{
    parser::{self, parse},
    reporter::{Formatter, Reporter},
    test_case::TestCaseFile,
    test_case_expr::{eval_test_expr, TestExprError},
    test_case_runner::run_tests,
    tmp_dir,
};

pub enum Input {
    File(String),
    Stdin,
}

pub enum TexestError {
    TestFailed,
    InvalidInput,
    InternalError,
}

impl TexestError {
    pub fn to_exit_status(&self) -> i32 {
        match self {
            TexestError::TestFailed => 1,
            TexestError::InvalidInput => 2,
            TexestError::InternalError => 3,
        }
    }
}

pub fn run(inputs: Vec<Input>, use_color: bool, f: Formatter) -> Result<(), TexestError> {
    let (test_case_expr_files, errs) = partition_results(inputs.iter().map(|input| {
        match input {
            Input::File(filename) => File::open(filename)
                .map_err(|err| {
                    parser::Error::without_violations(filename, format!("cannot open: {}", err))
                })
                .and_then(|file| parse(filename, file)),
            Input::Stdin => parse("<stdin>", std::io::stdin()),
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
        return Err(TexestError::InvalidInput);
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
        return Err(TexestError::InvalidInput);
    }

    let mut w = std::io::stdout();
    let mut r = Reporter::new(&mut w, use_color, f);

    let result = run_tests(test_case_files, &mut r);

    if let Err(err) = result {
        eprintln!("internal error: {}", err);
        return Err(TexestError::InternalError);
    }

    if !result.unwrap().is_all_passed() {
        return Err(TexestError::TestFailed);
    }

    Ok(())
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
