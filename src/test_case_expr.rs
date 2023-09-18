use std::time::Duration;

use serde_yaml::Value;

use crate::{matcher::StatusMatcherRegistry, parser::Error, test_case::TestCase};

#[derive(Debug, PartialEq)]
pub struct TestCaseExpr {
    pub filename: String,
    pub path: String,
    pub command: Vec<String>,
    pub stdin: String,
    pub timeout: Duration,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
    pub status_matcher: Option<Value>,
}

pub fn eval(
    status_mr: &StatusMatcherRegistry,
    test_case_expr: &TestCaseExpr,
) -> Result<Vec<TestCase>, Error> {
    Ok(vec![TestCase {
        filename: test_case_expr.filename.clone(),
        path: test_case_expr.path.clone(),
        command: test_case_expr.command.clone(),
        stdin: test_case_expr.stdin.clone(),
        timeout: test_case_expr.timeout,
        tee_stdout: test_case_expr.tee_stdout,
        tee_stderr: test_case_expr.tee_stderr,
        status_matcher: None,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    mod eval {
        use crate::matcher::new_status_matcher_registry;

        use super::*;
        use rstest::rstest;

        struct GivenTestCaseExpr {
            filename: &'static str,
            path: &'static str,
            command: Vec<&'static str>,
            stdin: &'static str,
            timeout: u64,
            tee_stdout: bool,
            tee_stderr: bool,
            status_matcher: Option<Value>,
        }

        const FILENAME: &str = "test.yaml";
        const PATH: &str = "$.tests[0]";

        impl Default for GivenTestCaseExpr {
            fn default() -> Self {
                Self {
                    filename: FILENAME,
                    path: PATH,
                    command: vec!["echo", "hello"],
                    stdin: "",
                    timeout: 1,
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matcher: None,
                }
            }
        }

        impl GivenTestCaseExpr {
            fn build(&self) -> TestCaseExpr {
                TestCaseExpr {
                    filename: self.filename.to_string(),
                    path: self.path.to_string(),
                    command: self.command.iter().map(|x| x.to_string()).collect(),
                    stdin: self.stdin.to_string(),
                    timeout: Duration::from_secs(self.timeout),
                    tee_stdout: self.tee_stdout,
                    tee_stderr: self.tee_stderr,
                    status_matcher: self.status_matcher.clone(),
                }
            }
        }

        #[rstest]
        #[case("with smallest case", GivenTestCaseExpr::default(), vec![TestCase {
            filename: FILENAME.to_string(),
            path: PATH.to_string(),
            command: vec!["echo".to_string(), "hello".to_string()],
            stdin: "".to_string(),
            timeout: Duration::from_secs(1),
            tee_stdout: false,
            tee_stderr: false,
            status_matcher: None,
        }])]
        fn success_cases(
            #[case] title: &str,
            #[case] given: GivenTestCaseExpr,
            #[case] expected: Vec<TestCase>,
        ) {
            let status_mr = new_status_matcher_registry();

            let actual = eval(&status_mr, &given.build());

            assert_eq!(actual, Ok(expected), "{}", title);
        }
    }
}
