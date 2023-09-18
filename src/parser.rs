use std::time::Duration;

use crate::{
    test_case_expr::TestCaseExpr,
    validator::{Validator, Violation},
};

#[derive(PartialEq, Debug)]
pub struct Error {
    pub filename: String,
    pub message: String,
    pub violations: Vec<Violation>,
}

impl Error {
    fn without_violations(filename: String, message: String) -> Self {
        Self {
            filename,
            message,
            violations: vec![],
        }
    }

    fn with_violations(filename: String, message: String, violations: Vec<Violation>) -> Self {
        Self {
            filename,
            message,
            violations,
        }
    }
}

const DEFAULT_TIMEOUT: u64 = 10;

pub fn parse(filename: String, reader: impl std::io::Read) -> Result<Vec<TestCaseExpr>, Error> {
    let ast = serde_yaml::from_reader(reader).map_err(|err| {
        Error::without_violations(
            filename.clone(),
            format!("cannot parse {}: {}", filename.clone(), err),
        )
    })?;

    let mut v = Validator::new(filename.clone());

    let test_case_exprs = v
        .must_be_map(&ast)
        .and_then(|root| {
            v.must_have_seq(root, "tests", |v, tests| {
                v.map_seq(tests, |v, test| {
                    v.must_be_map(test).and_then(|test| {
                        let stdin = v.may_have_string(test, "stdin").unwrap_or_default();
                        let timeout = v.may_have_uint(test, "timeout").unwrap_or(DEFAULT_TIMEOUT);
                        let tee_stdout = v.may_have_bool(test, "teeStdout").unwrap_or(false);
                        let tee_stderr = v.may_have_bool(test, "teeStderr").unwrap_or(false);
                        v.must_have_seq(test, "command", |v, command| {
                            if command.is_empty() {
                                v.add_violation("should not be empty");
                                None
                            } else {
                                v.map_seq(command, |v, arg| v.must_be_string(arg))
                            }
                        })
                        .flatten()
                        .map(|command| TestCaseExpr {
                            filename: v.filename.clone(),
                            path: v.current_path(),
                            command,
                            stdin,
                            timeout: Duration::from_secs(timeout),
                            tee_stdout,
                            tee_stderr,
                            status_matcher: None,
                        })
                    })
                })
            })
        })
        .flatten();

    test_case_exprs
        .ok_or_else(|| Error::with_violations(filename, "parse error".to_string(), v.violations))
}

#[cfg(test)]
mod tests {
    use super::*;
    mod parse {
        use std::time::Duration;

        use super::*;
        use rstest::rstest;

        const FILENAME: &str = "test.yaml";
        fn parse_error(violations: Vec<Violation>) -> Result<Vec<TestCaseExpr>, Error> {
            Err(Error::with_violations(
                FILENAME.to_string(),
                "parse error".to_string(),
                violations,
            ))
        }

        fn violation(path: &str, message: &str) -> Violation {
            Violation {
                filename: FILENAME.to_string(),
                path: path.to_string(),
                message: message.to_string(),
            }
        }

        fn test_case_exprs(cases: Vec<(Vec<&str>, &str, u64, bool, bool)>) -> Vec<TestCaseExpr> {
            cases
                .iter()
                .enumerate()
                .map(
                    |(i, (command, stdin, timeout, tee_stdout, tee_stderr))| TestCaseExpr {
                        filename: FILENAME.to_string(),
                        path: format!("$.tests[{}]", i),
                        command: command.iter().map(|x| x.to_string()).collect(),
                        stdin: stdin.to_string(),
                        timeout: Duration::from_secs(*timeout),
                        tee_stdout: *tee_stdout,
                        tee_stderr: *tee_stderr,
                        status_matcher: None,
                    },
                )
                .collect()
        }

        #[rstest]
        #[case("with command only", "
tests:
    - command:
        - echo
        - hello", test_case_exprs(vec![(vec!["echo", "hello"], "", 10, false, false)]))]
        #[case("with command contains timeout", "
tests:
    - command:
        - echo
        - hello
      timeout: 5", test_case_exprs(vec![(vec!["echo", "hello"], "", 5, false, false)]))]
        #[case("with command cotains tee_stdout & tee_stderr", "
tests:
    - command:
        - echo
        - hello
      teeStdout: true
      teeStderr: true", test_case_exprs(vec![(vec!["echo", "hello"], "", 10, true, true)]))]
        #[case("with command contains stdin", "
tests:
    - command:
        - cat
      stdin: hello", test_case_exprs(vec![(vec!["cat"], "hello", 10, false, false)]))]
        fn success_case(
            #[case] title: &str,
            #[case] input: &str,
            #[case] expected: Vec<TestCaseExpr>,
        ) {
            let filename = FILENAME.to_string();
            let actual: Result<Vec<TestCaseExpr>, Error> =
                parse(filename.clone(), input.as_bytes());

            assert_eq!(actual, Ok(expected), "{}", title)
        }

        #[rstest]
        #[case("when root is not map", "tests", vec![("$", "should be map, but is string")])]
        #[case("when root dosen't have .tests", "{}", vec![("$", "should have .tests as seq")])]
        #[case("when root.tests is not seq", "tests: {}", vec![("$.tests", "should be seq, but is map")])]
        #[case("when test is not map", "tests: [42]", vec![("$.tests[0]", "should be map, but is uint")])]
        #[case("when test dosen't have .command", "tests: [{}]", vec![("$.tests[0]", "should have .command as seq")])]
        #[case("when test command is not seq", "tests: [{command: 42}]", vec![("$.tests[0].command", "should be seq, but is uint")])]
        #[case("when test command contains not string", "tests: [{command: [42]}]", vec![("$.tests[0].command[0]", "should be string, but is uint")])]
        #[case("when test command is empty", "tests: [{command: []}]", vec![("$.tests[0].command", "should not be empty")])]
        fn error_case(
            #[case] title: &str,
            #[case] input: &str,
            #[case] violations: Vec<(&str, &str)>,
        ) {
            let filename = FILENAME.to_string();
            let actual = parse(filename, input.as_bytes());
            assert_eq!(
                actual,
                parse_error(
                    violations
                        .iter()
                        .map(|(path, message)| violation(path, message))
                        .collect()
                ),
                "{}",
                title
            )
        }
    }
}
