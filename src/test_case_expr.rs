use std::time::Duration;

use serde_yaml::Mapping;

use crate::{
    expr::{eval_expr, Expr},
    matcher::{StatusMatcherRegistry, StreamMatcherRegistry},
    test_case::TestCase,
    validator::{Validator, Violation},
};

#[derive(Debug, PartialEq)]
pub struct TestExprError {
    pub violations: Vec<Violation>,
}

#[derive(Debug, PartialEq)]
pub struct TestCaseExpr {
    pub filename: String,
    pub path: String,
    pub command: Vec<Expr>,
    pub stdin: Expr,
    pub env: Vec<(String, Expr)>,
    pub timeout: Duration,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
    pub status_matchers: Mapping,
    pub stdout_matchers: Mapping,
    pub stderr_matchers: Mapping,
}

#[derive(Debug, PartialEq)]
pub struct TestCaseExprFile {
    pub filename: String,
    pub test_case_exprs: Vec<TestCaseExpr>,
}

pub fn eval_test_expr(
    status_mr: &StatusMatcherRegistry,
    stream_mr: &StreamMatcherRegistry,
    test_case_expr: &TestCaseExpr,
) -> Result<Vec<TestCase>, TestExprError> {
    let mut v = Validator::new_with_paths(
        test_case_expr.filename.clone(),
        vec![test_case_expr.path.clone()],
    );

    let command: Vec<String> = v.in_field("command", |v| {
        test_case_expr
            .command
            .clone()
            .into_iter()
            .enumerate()
            .filter_map(|(i, x)| match eval_expr(&x) {
                Ok(value) => v.in_index(i, |v| v.must_be_string(&value)),
                Err(message) => {
                    v.in_index(i, |v| v.add_violation(format!("eval error: {}", message)));
                    None
                }
            })
            .collect()
    });

    let stdin = v
        .in_field("stdin", |v| match eval_expr(&test_case_expr.stdin) {
            Ok(value) => v.must_be_string(&value),
            Err(message) => {
                v.add_violation(format!("eval error: {}", message));
                None
            }
        })
        .unwrap_or("".to_string());

    let env: Vec<(String, String)> = v.in_field("env", |v| {
        test_case_expr
            .env
            .iter()
            .filter_map(|(name, expr)| {
                match eval_expr(expr) {
                    Ok(value) => v.in_field(name, |v| v.must_be_string(&value)),
                    Err(message) => {
                        v.in_field(name, |v| {
                            v.add_violation(format!("eval error: {}", message))
                        });
                        None
                    }
                }
                .map(|value| (name.clone(), value))
            })
            .collect()
    });

    let status_matchers = v.in_field("expect.status", |v| {
        test_case_expr
            .status_matchers
            .iter()
            .filter_map(|(name, param)| {
                v.must_be_string(name)
                    .and_then(|name| status_mr.parse(name, v, param))
            })
            .collect::<Vec<_>>()
    });

    let stdout_matchers = v.in_field("expect.stdout", |v| {
        test_case_expr
            .stdout_matchers
            .iter()
            .filter_map(|(name, param)| {
                v.must_be_string(name)
                    .and_then(|name| stream_mr.parse(name, v, param))
            })
            .collect::<Vec<_>>()
    });

    let stderr_matchers = v.in_field("expect.stderr", |v| {
        test_case_expr
            .stderr_matchers
            .iter()
            .filter_map(|(name, param)| {
                v.must_be_string(name)
                    .and_then(|name| stream_mr.parse(name, v, param))
            })
            .collect::<Vec<_>>()
    });

    if v.violations.is_empty() {
        Ok(vec![TestCase {
            filename: test_case_expr.filename.clone(),
            path: test_case_expr.path.clone(),
            command,
            stdin,
            env,
            timeout: test_case_expr.timeout,
            tee_stdout: test_case_expr.tee_stdout,
            tee_stderr: test_case_expr.tee_stderr,
            status_matchers,
            stdout_matchers,
            stderr_matchers,
        }])
    } else {
        Err(TestExprError {
            violations: v.violations,
        })
    }
}

#[cfg(test)]
pub mod testutil {
    use std::time::Duration;

    use serde_yaml::Mapping;

    use crate::expr::Expr;

    use crate::expr::testutil::*;

    use super::TestCaseExpr;

    pub struct TestCaseExprTemplate {
        pub filename: &'static str,
        pub path: &'static str,
        pub command: Vec<Expr>,
        pub stdin: Expr,
        pub env: Vec<(&'static str, Expr)>,
        pub timeout: u64,
        pub tee_stdout: bool,
        pub tee_stderr: bool,
        pub status_matchers: Mapping,
        pub stdout_matchers: Mapping,
        pub stderr_matchers: Mapping,
    }

    impl TestCaseExprTemplate {
        pub const DEFAULT_FILENAME: &str = "test.yaml";
        pub const DEFAULT_PATH: &str = "$.tests[0]";

        pub fn default_command() -> Vec<Expr> {
            vec![literal_expr("echo"), literal_expr("hello")]
        }

        pub fn build(&self) -> TestCaseExpr {
            TestCaseExpr {
                filename: self.filename.to_string(),
                path: self.path.to_string(),
                command: self.command.clone(),
                stdin: self.stdin.clone(),
                env: self
                    .env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
                timeout: Duration::from_secs(self.timeout),
                tee_stdout: self.tee_stdout,
                tee_stderr: self.tee_stderr,
                status_matchers: self.status_matchers.clone(),
                stdout_matchers: self.stdout_matchers.clone(),
                stderr_matchers: self.stderr_matchers.clone(),
            }
        }
    }

    impl Default for TestCaseExprTemplate {
        fn default() -> Self {
            Self {
                filename: TestCaseExprTemplate::DEFAULT_FILENAME,
                path: TestCaseExprTemplate::DEFAULT_PATH,
                command: TestCaseExprTemplate::default_command(),
                stdin: literal_expr(""),
                env: vec![],
                timeout: 10,
                tee_stdout: false,
                tee_stderr: false,
                status_matchers: Mapping::new(),
                stdout_matchers: Mapping::new(),
                stderr_matchers: Mapping::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod eval_test_case_expr {
        use crate::{
            ast::testuitl::mapping,
            expr::testutil::{env_var_expr, literal_expr},
            matcher::testutil::{
                new_test_matcher_registry, TestMatcher, PARSE_ERROR_MATCHER, SUCCESS_MATCHER,
                VIOLATION_MESSAGE,
            },
            test_case_expr::testutil::TestCaseExprTemplate,
        };

        use super::*;
        use rstest::rstest;
        use serde_yaml::Value;

        fn violation(path: &str, message: &str) -> Violation {
            Violation {
                filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                path: TestCaseExprTemplate::DEFAULT_PATH.to_string() + path,
                message: message.to_string(),
            }
        }

        #[rstest]
        #[case("with smallest case", TestCaseExprTemplate::default(), vec![TestCase {
            filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
            path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
            command: vec!["echo".to_string(), "hello".to_string()],
            stdin: "".to_string(),
            env: vec![],
            timeout: Duration::from_secs(10),
            tee_stdout: false,
            tee_stderr: false,
            status_matchers: vec![],
            stdout_matchers: vec![],
            stderr_matchers: vec![],
        }])]
        #[case("with stdin case",
            TestCaseExprTemplate {
                stdin: literal_expr("hello"),
                ..Default::default()
            },
            vec![
                TestCase {
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                    stdin: "hello".to_string(),
                    env: vec![],
                    timeout: Duration::from_secs(10),
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                },
            ]
        )]
        #[case("with env case",
            TestCaseExprTemplate {
                env: vec![("MESSAGE1", literal_expr("hello")), ("MESSAGE2", literal_expr("world"))],
                ..Default::default()
            },
            vec![
                TestCase {
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                    stdin: "".to_string(),
                    env: vec![("MESSAGE1".to_string(), "hello".to_string()), ("MESSAGE2".to_string(), "world".to_string())],
                    timeout: Duration::from_secs(10),
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                },
            ]
        )]
        #[case("with status matcher case",
            TestCaseExprTemplate {
                status_matchers: mapping(vec![
                    (SUCCESS_MATCHER, Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                TestCase {
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                    stdin: "".to_string(),
                    env: vec![],
                    timeout: Duration::from_secs(10),
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matchers: vec!(TestMatcher::new_success(Value::from(true))),
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                },
            ]
        )]
        #[case("with stdout matcher case",
            TestCaseExprTemplate {
                stdout_matchers: mapping(vec![
                    (SUCCESS_MATCHER, Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                TestCase {
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                    stdin: "".to_string(),
                    env: vec![],
                    timeout: Duration::from_secs(10),
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matchers: vec![],
                    stdout_matchers: vec![TestMatcher::new_success(Value::from(true))],
                    stderr_matchers: vec![],
                },
            ]
        )]
        #[case("with stderr matcher case",
            TestCaseExprTemplate {
                stderr_matchers: mapping(vec![
                    (SUCCESS_MATCHER, Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                TestCase {
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    command: vec!["echo".to_string(), "hello".to_string()],
                    stdin: "".to_string(),
                    env: vec![],
                    timeout: Duration::from_secs(10),
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![TestMatcher::new_success(Value::from(true))],
                },
            ]
        )]
        fn success_cases(
            #[case] title: &str,
            #[case] given: TestCaseExprTemplate,
            #[case] expected: Vec<TestCase>,
        ) {
            let status_mr = new_test_matcher_registry();
            let stream_mr = new_test_matcher_registry();

            let actual = eval_test_expr(&status_mr, &stream_mr, &given.build());

            assert_eq!(actual, Ok(expected), "{}", title);
        }

        #[rstest]
        #[case("with eval error in command",
            TestCaseExprTemplate {
                command: vec![literal_expr(true), env_var_expr("_undefined")],
                ..Default::default()
            },
            vec![
                violation(".command[0]", "should be string, but is bool"),
                violation(".command[1]", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with eval error in env",
            TestCaseExprTemplate {
                env: vec![("MESSAGE1", literal_expr(true)), ("MESSAGE2", env_var_expr("_undefined"))],
                ..Default::default()
            },
            vec![
                violation(".env.MESSAGE1", "should be string, but is bool"),
                violation(".env.MESSAGE2", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with not string stdin",
            TestCaseExprTemplate {
                stdin: literal_expr(true),
                ..Default::default()
            },
            vec![
                violation(".stdin", "should be string, but is bool"),
            ]
        )]
        #[case("with eval error in stdin",
            TestCaseExprTemplate {
                stdin: env_var_expr("_undefined"),
                ..Default::default()
            },
            vec![
                violation(".stdin", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with undefined status matcher",
            TestCaseExprTemplate {
                status_matchers: mapping(vec![
                    ("unknown", Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                violation(".expect.status", "test matcher unknown is not defined")
            ]
        )]
        #[case("with invalid status matcher",
            TestCaseExprTemplate {
                status_matchers: mapping(vec![
                    (PARSE_ERROR_MATCHER, Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                violation(".expect.status.parse_error", VIOLATION_MESSAGE)
            ]
        )]
        #[case("with undefined stdout matcher",
            TestCaseExprTemplate {
                stdout_matchers: mapping(vec![
                    ("unknown", Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                violation(".expect.stdout", "test matcher unknown is not defined")
            ]
        )]
        #[case("with invalid stdout matcher",
            TestCaseExprTemplate {
                stdout_matchers: mapping(vec![
                    (PARSE_ERROR_MATCHER, Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                violation(".expect.stdout.parse_error", VIOLATION_MESSAGE)
            ]
        )]
        #[case("with undefined stderr matcher",
            TestCaseExprTemplate {
                stderr_matchers: mapping(vec![
                    ("unknown", Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                violation(".expect.stderr", "test matcher unknown is not defined")
            ]
        )]
        #[case("with invalid stderr matcher",
            TestCaseExprTemplate {
                stderr_matchers: mapping(vec![
                    (PARSE_ERROR_MATCHER, Value::from(true)),
                ]),
                ..Default::default()
            },
            vec![
                violation(".expect.stderr.parse_error", VIOLATION_MESSAGE)
            ]
        )]
        fn failure_cases(
            #[case] title: &str,
            #[case] given: TestCaseExprTemplate,
            #[case] expected_violations: Vec<Violation>,
        ) {
            let status_mr = new_test_matcher_registry();
            let stream_mr = new_test_matcher_registry();

            let actual = eval_test_expr(&status_mr, &stream_mr, &given.build());

            assert_eq!(
                actual,
                Err(TestExprError {
                    violations: expected_violations
                }),
                "{}",
                title
            );
        }
    }
}
