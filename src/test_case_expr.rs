use std::time::Duration;

use serde_yaml::{Mapping, Value};

use crate::{
    matcher::{Matcher, StatusMatcherRegistry},
    test_case::TestCase,
    validator::{Validator, Violation},
};

#[derive(Debug, PartialEq)]
pub struct EvalError {
    pub violations: Vec<Violation>,
}

#[derive(Debug, PartialEq)]
pub struct TestCaseExpr {
    pub filename: String,
    pub path: String,
    pub command: Vec<String>,
    pub stdin: String,
    pub timeout: Duration,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
    pub status_matchers: Option<Mapping>,
}

pub fn eval(
    status_mr: &StatusMatcherRegistry,
    test_case_expr: &TestCaseExpr,
) -> Result<Vec<TestCase>, EvalError> {
    let mut v = Validator::new_with_paths(
        test_case_expr.filename.clone(),
        vec![test_case_expr.path.clone()],
    );

    let status_matchers: Vec<Box<dyn Matcher<i32>>> = test_case_expr
        .status_matchers
        .as_ref()
        .map(|m| {
            m.into_iter()
                .filter_map(|(name, param)| {
                    v.must_be_string(name)
                        .and_then(|name| status_mr.parse(name, &mut v, param))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or(vec![]);

    Ok(vec![TestCase {
        filename: test_case_expr.filename.clone(),
        path: test_case_expr.path.clone(),
        command: test_case_expr.command.clone(),
        stdin: test_case_expr.stdin.clone(),
        timeout: test_case_expr.timeout,
        tee_stdout: test_case_expr.tee_stdout,
        tee_stderr: test_case_expr.tee_stderr,
        status_matchers,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    mod eval {
        use crate::matcher::testutil::{new_test_matcher_registry, TestMatcher, SUCCESS_MATCHER};

        use super::*;
        use rstest::rstest;
        use serde_yaml::Mapping;

        struct GivenTestCaseExpr {
            filename: &'static str,
            path: &'static str,
            command: Vec<&'static str>,
            stdin: &'static str,
            timeout: u64,
            tee_stdout: bool,
            tee_stderr: bool,
            status_matchers: Option<Mapping>,
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
                    status_matchers: None,
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
                    status_matchers: self.status_matchers.clone(),
                }
            }
        }

        fn mapping(v: Vec<(&str, Value)>) -> Mapping {
            let mut m = Mapping::new();
            v.iter().for_each(|(k, v)| {
                m.insert(Value::String(k.to_string()), v.clone());
            });
            m
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
            status_matchers: vec!(),
        }])]
        #[case("with smallest case",
            GivenTestCaseExpr {
                status_matchers: Some(mapping(vec![
                    (SUCCESS_MATCHER, Value::from(true)),
                ])),
                ..Default::default()
            },
            vec![
                TestCase {
                    filename: FILENAME.to_string(),
                    path: PATH.to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                    stdin: "".to_string(),
                    timeout: Duration::from_secs(1),
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matchers: vec!(TestMatcher::new_success(Value::from(true)))
                },
            ]
        )]
        fn success_cases(
            #[case] title: &str,
            #[case] given: GivenTestCaseExpr,
            #[case] expected: Vec<TestCase>,
        ) {
            let status_mr = new_test_matcher_registry();

            let actual = eval(&status_mr, &given.build());

            assert_eq!(actual, Ok(expected), "{}", title);
        }

        fn failure_cases() {}
    }
}
