use std::{fs::File, io::Write};

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

#[cfg_attr(test, derive(Debug, PartialEq))]
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

pub struct Runner<ReportW: Write, ErrW: Write> {
    use_color: bool,
    formatter: Formatter,
    rw: ReportW,
    errw: ErrW,
    tee_stdout: bool,
    tee_stderr: bool,
}

impl<ReportW: Write, ErrW: Write> Runner<ReportW, ErrW> {
    pub fn new(
        use_color: bool,
        formatter: Formatter,
        rw: ReportW,
        errw: ErrW,
        tee_stdout: bool,
        tee_stderr: bool,
    ) -> Self {
        Self {
            use_color,
            formatter,
            rw,
            errw,
            tee_stdout,
            tee_stderr,
        }
    }

    pub fn run(mut self, inputs: Vec<Input>) -> Result<(), TexestError> {
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
            errs.iter()
                .try_for_each(|err| -> std::io::Result<()> {
                    writeln!(self.errw, "{}: {}", err.filename, err.message)?;
                    err.violations
                        .iter()
                        .try_for_each(|violation| -> std::io::Result<()> {
                            writeln!(
                                self.errw,
                                "{}:{}: {}",
                                violation.filename, violation.path, violation.message
                            )
                        })
                })
                .or(Err(TexestError::InternalError))?;
            return Err(TexestError::InvalidInput);
        }

        let mut tmp_dir_supplier = tmp_dir::TmpDirFactory::new();

        let (test_case_files, errs): (Vec<TestCaseFile>, Vec<TestExprError>) = test_case_expr_files
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
            errs.iter()
                .try_for_each(|err| -> std::io::Result<()> {
                    err.violations
                        .iter()
                        .try_for_each(|violation| -> std::io::Result<()> {
                            writeln!(
                                self.errw,
                                "{}:{}: {}",
                                violation.filename, violation.path, violation.message
                            )
                        })
                })
                .or(Err(TexestError::InternalError))?;
            return Err(TexestError::InvalidInput);
        }

        let mut r = Reporter::new(&mut self.rw, self.use_color, self.formatter);

        let result = run_tests(test_case_files, &mut r, self.tee_stdout, self.tee_stderr);

        let test_result_summary = match result {
            Ok(test_result_summary) => test_result_summary,
            Err(err) => {
                writeln!(self.errw, "internal error: {}", err)
                    .or(Err(TexestError::InternalError))?;
                return Err(TexestError::InternalError);
            }
        };

        if !test_result_summary.is_all_passed() {
            return Err(TexestError::TestFailed);
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use serde_json::json;
    use tempfile::NamedTempFile;

    #[rstest]
    fn when_all_case_passed() {
        let formatter = Formatter::new_json();
        let mut rw: Vec<u8> = vec![];
        let mut errw: Vec<u8> = vec![];
        let runner = Runner::new(true, formatter, &mut rw, &mut errw, false, false);

        let mut file = NamedTempFile::new().unwrap();
        let spec = r#"{ tests: [{ command: ["true"], expect: { status: { eq: 0 } } }]}"#;
        file.write_all(spec.as_bytes()).unwrap();

        let result = runner.run(vec![Input::File(file.path().to_str().unwrap().to_string())]);

        assert_eq!("", String::from_utf8_lossy(&errw));
        assert_eq!(
            json!({
                "num_test_cases": 1,
                "num_passed_test_cases": 1,
                "num_failed_test_cases": 0,
                "success": true,
                "test_results": [
                    {
                        "name": "true",
                        "passed": true,
                        "failures": []
                    },
                ]
            }),
            serde_json::from_slice::<serde_json::Value>(rw.as_slice()).unwrap(),
        );
        assert_eq!(Ok(()), result);
    }

    #[rstest]
    fn when_failure_occured() {
        let formatter = Formatter::new_json();
        let mut rw: Vec<u8> = vec![];
        let mut errw: Vec<u8> = vec![];
        let runner = Runner::new(true, formatter, &mut rw, &mut errw, false, false);

        let mut file = NamedTempFile::new().unwrap();
        let spec = r#"{ tests: [{ command: ["true"], expect: { status: { eq: 1 } } }]}"#;
        file.write_all(spec.as_bytes()).unwrap();

        let result = runner.run(vec![Input::File(file.path().to_str().unwrap().to_string())]);

        assert_eq!("", String::from_utf8_lossy(&errw));
        assert_eq!(
            json!({
                "num_test_cases": 1,
                "num_passed_test_cases": 0,
                "num_failed_test_cases": 1,
                "success": false,
                "test_results": [
                    {
                        "name": "true",
                        "passed": false,
                        "failures": [
                            {
                                "messages": ["should be 1, but got 0"],
                                "subject": "main:status"
                            }
                        ]
                    },
                ]
            }),
            serde_json::from_slice::<serde_json::Value>(rw.as_slice()).unwrap(),
        );
        assert_eq!(Err(TexestError::TestFailed), result);
    }

    #[rstest]
    fn when_file_is_not_exists() {
        let formatter = Formatter::new_json();
        let mut rw: Vec<u8> = vec![];
        let mut errw: Vec<u8> = vec![];
        let runner = Runner::new(true, formatter, &mut rw, &mut errw, false, false);

        let result = runner.run(vec![Input::File("not_exist.yaml".to_string())]);

        assert_eq!(
            "not_exist.yaml: cannot open: No such file or directory (os error 2)\n",
            String::from_utf8_lossy(&errw)
        );
        assert_eq!(Err(TexestError::InvalidInput), result);
    }

    #[rstest]
    fn when_invalid_syntax_given() {
        let formatter = Formatter::new_json();
        let mut rw: Vec<u8> = vec![];
        let mut errw: Vec<u8> = vec![];
        let runner = Runner::new(true, formatter, &mut rw, &mut errw, false, false);

        let mut file = NamedTempFile::new().unwrap();
        let spec = r#"{ tests: [{ expect: { status: { eq: 0 } } }]}"#;
        file.write_all(spec.as_bytes()).unwrap();

        let path = file.path().to_str().unwrap().to_string();
        let result = runner.run(vec![Input::File(path.clone())]);

        assert_eq!(
            format!(
                "{}: parse error\n{}:$.tests[0]: should have .command as seq\n",
                path, path
            ),
            String::from_utf8_lossy(&errw)
        );
        assert_eq!(Err(TexestError::InvalidInput), result);
    }

    #[rstest]
    fn when_eval_error_occured() {
        let formatter = Formatter::new_json();
        let mut rw: Vec<u8> = vec![];
        let mut errw: Vec<u8> = vec![];
        let runner = Runner::new(true, formatter, &mut rw, &mut errw, false, false);

        let mut file = NamedTempFile::new().unwrap();
        let spec = r#"{ tests: [{ command: [{ $env: "UNDEFINED_ENV" }],  expect: { status: { eq: 0 } } }]}"#;
        file.write_all(spec.as_bytes()).unwrap();

        let path = file.path().to_str().unwrap().to_string();
        let result = runner.run(vec![Input::File(path.clone())]);

        assert_eq!(
            format!(
                "{}:$.tests[0].command[0]: eval error: env var UNDEFINED_ENV is not defined\n",
                path
            ),
            String::from_utf8_lossy(&errw)
        );
        assert_eq!(Err(TexestError::InvalidInput), result);
    }
}
