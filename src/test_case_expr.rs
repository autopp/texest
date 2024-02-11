use std::time::Duration;

use indexmap::{indexmap, IndexMap};

use crate::{
    expr::{Context, EvalOutput, Expr},
    matcher::{StatusMatcherRegistry, StreamMatcherRegistry},
    test_case::{Process, SetupHook, TestCase},
    tmp_dir::TmpDirSupplier,
    validator::{Validator, Violation},
};

#[derive(Debug, PartialEq)]
pub struct TestExprError {
    pub violations: Vec<Violation>,
}

#[derive(Debug, PartialEq)]
pub struct ProcessExpr {
    pub command: Vec<Expr>,
    pub stdin: Expr,
    pub env: Vec<(String, Expr)>,
    pub timeout: Duration,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
}

#[derive(Debug, PartialEq)]
pub enum ProcessesExpr {
    Single(ProcessExpr),
    Multi(IndexMap<String, ProcessExpr>),
}

#[derive(Debug, PartialEq)]
pub struct TestCaseExpr {
    pub name: Option<Expr>,
    pub filename: String,
    pub path: String,
    pub processes: ProcessesExpr,
    pub status_matchers: IndexMap<String, Expr>,
    pub stdout_matchers: IndexMap<String, Expr>,
    pub stderr_matchers: IndexMap<String, Expr>,
}

#[derive(Debug, PartialEq)]
pub struct TestCaseExprFile {
    pub filename: String,
    pub test_case_exprs: Vec<TestCaseExpr>,
}

const DEFAULT_PROCESS_NAME: &str = "main";

pub fn eval_test_expr<T: TmpDirSupplier>(
    tmp_dir_supplier: &mut T,
    status_mr: &StatusMatcherRegistry,
    stream_mr: &StreamMatcherRegistry,
    test_case_expr: &TestCaseExpr,
) -> Result<Vec<TestCase>, TestExprError> {
    let mut v =
        Validator::new_with_paths(&test_case_expr.filename, vec![test_case_expr.path.clone()]);
    let mut ctx = Context::new(tmp_dir_supplier);
    let mut setup_hooks: Vec<Box<dyn SetupHook>> = vec![];

    let processes = match &test_case_expr.processes {
        ProcessesExpr::Single(process_expr) => {
            indexmap! { DEFAULT_PROCESS_NAME.to_string() => eval_process_expr(&mut v, &mut ctx, &mut setup_hooks, process_expr) }
        }
        ProcessesExpr::Multi(process_exprs) => process_exprs
            .iter()
            .map(|(name, process_expr)| {
                (
                    name.clone(),
                    v.in_field(name, |v| {
                        eval_process_expr(v, &mut ctx, &mut setup_hooks, process_expr)
                    }),
                )
            })
            .collect(),
    };

    let name = if let Some(name_expr) = &test_case_expr.name {
        v.in_field("name", |v| match ctx.eval_expr(name_expr) {
            Ok(EvalOutput { value, setup_hook }) => {
                if let Some(hook) = setup_hook {
                    setup_hooks.push(hook)
                }
                v.must_be_string(&value)
            }
            Err(message) => {
                v.add_violation(format!("eval error: {}", message));
                None
            }
        })
    } else {
        Some(
            processes
                .values()
                .last()
                .unwrap()
                .command
                .iter()
                .map(|x| yash_quote::quote(x))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
    .unwrap_or("".to_string());

    let status_matchers = v.in_field("expect.status", |v| {
        test_case_expr
            .status_matchers
            .iter()
            .filter_map(|(name, param_expr)| match ctx.eval_expr(param_expr) {
                Ok(param) => status_mr.parse(name, v, &param.value),
                Err(message) => {
                    v.in_field(name, |v| {
                        v.add_violation(format!("eval error: {}", message))
                    });
                    None
                }
            })
            .collect::<Vec<_>>()
    });

    let stdout_matchers = v.in_field("expect.stdout", |v| {
        test_case_expr
            .stdout_matchers
            .iter()
            .filter_map(|(name, param_expr)| match ctx.eval_expr(param_expr) {
                Ok(param) => stream_mr.parse(name, v, &param.value),
                Err(message) => {
                    v.in_field(name, |v| {
                        v.add_violation(format!("eval error: {}", message))
                    });
                    None
                }
            })
            .collect::<Vec<_>>()
    });

    let stderr_matchers = v.in_field("expect.stderr", |v| {
        test_case_expr
            .stderr_matchers
            .iter()
            .filter_map(|(name, param_expr)| match ctx.eval_expr(param_expr) {
                Ok(param) => stream_mr.parse(name, v, &param.value),
                Err(message) => {
                    v.in_field(name, |v| {
                        v.add_violation(format!("eval error: {}", message))
                    });
                    None
                }
            })
            .collect::<Vec<_>>()
    });

    if v.violations.is_empty() {
        Ok(vec![TestCase {
            name,
            filename: test_case_expr.filename.clone(),
            path: test_case_expr.path.clone(),
            processes,
            status_matchers,
            stdout_matchers,
            stderr_matchers,
            setup_hooks,
            teardown_hooks: vec![],
        }])
    } else {
        Err(TestExprError {
            violations: v.violations,
        })
    }
}

fn eval_process_expr<T: TmpDirSupplier>(
    v: &mut Validator,
    ctx: &mut Context<'_, T>,
    setup_hooks: &mut Vec<Box<dyn SetupHook>>,
    process_expr: &ProcessExpr,
) -> Process {
    let command: Vec<String> = v.in_field("command", |v| {
        process_expr
            .command
            .clone()
            .into_iter()
            .enumerate()
            .filter_map(|(i, x)| match ctx.eval_expr(&x) {
                Ok(EvalOutput { value, setup_hook }) => {
                    if let Some(hook) = setup_hook {
                        setup_hooks.push(hook)
                    }
                    v.in_index(i, |v| v.must_be_string(&value))
                }
                Err(message) => {
                    v.in_index(i, |v| v.add_violation(format!("eval error: {}", message)));
                    None
                }
            })
            .collect()
    });

    let stdin = v
        .in_field("stdin", |v| match ctx.eval_expr(&process_expr.stdin) {
            Ok(EvalOutput { value, setup_hook }) => {
                if let Some(hook) = setup_hook {
                    setup_hooks.push(hook)
                }
                v.must_be_string(&value)
            }
            Err(message) => {
                v.add_violation(format!("eval error: {}", message));
                None
            }
        })
        .unwrap_or("".to_string());

    let env: Vec<(String, String)> = v.in_field("env", |v| {
        process_expr
            .env
            .iter()
            .filter_map(|(name, expr)| {
                match ctx.eval_expr(expr) {
                    Ok(EvalOutput { value, setup_hook }) => {
                        if let Some(hook) = setup_hook {
                            setup_hooks.push(hook)
                        }
                        v.in_field(name, |v| v.must_be_string(&value))
                    }
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

    Process {
        command,
        stdin,
        env,
        timeout: process_expr.timeout,
        tee_stdout: process_expr.tee_stdout,
        tee_stderr: process_expr.tee_stderr,
    }
}

#[cfg(test)]
pub mod testutil {
    use std::time::Duration;

    use indexmap::IndexMap;

    use crate::expr::Expr;

    use crate::expr::testutil::*;

    use super::ProcessExpr;
    use super::ProcessesExpr;
    use super::TestCaseExpr;

    pub struct ProcessExprTemplate {
        pub command: Vec<Expr>,
        pub stdin: Expr,
        pub env: Vec<(&'static str, Expr)>,
        pub timeout: u64,
        pub tee_stdout: bool,
        pub tee_stderr: bool,
    }

    impl ProcessExprTemplate {
        pub fn build(&self) -> ProcessExpr {
            ProcessExpr {
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
            }
        }
    }

    impl Default for ProcessExprTemplate {
        fn default() -> Self {
            Self {
                command: TestCaseExprTemplate::default_command(),
                stdin: literal_expr(""),
                env: vec![],
                timeout: 10,
                tee_stdout: false,
                tee_stderr: false,
            }
        }
    }

    pub enum ProcessesExprTemplate {
        Single(ProcessExprTemplate),
        Multi(IndexMap<&'static str, ProcessExprTemplate>),
    }

    impl ProcessesExprTemplate {
        pub fn build(&self) -> ProcessesExpr {
            match self {
                ProcessesExprTemplate::Single(p) => ProcessesExpr::Single(p.build()),
                ProcessesExprTemplate::Multi(ps) => ProcessesExpr::Multi(
                    ps.iter().map(|(k, v)| (k.to_string(), v.build())).collect(),
                ),
            }
        }
    }

    pub struct TestCaseExprTemplate {
        pub name: Option<Expr>,
        pub filename: &'static str,
        pub path: &'static str,
        pub processes: ProcessesExprTemplate,
        pub status_matchers: IndexMap<&'static str, Expr>,
        pub stdout_matchers: IndexMap<&'static str, Expr>,
        pub stderr_matchers: IndexMap<&'static str, Expr>,
    }

    impl TestCaseExprTemplate {
        pub const NAME_FOR_DEFAULT_COMMAND: &str = "echo hello";
        pub const DEFAULT_FILENAME: &str = "test.yaml";
        pub const DEFAULT_PATH: &str = "$.tests[0]";

        pub fn default_command() -> Vec<Expr> {
            vec![literal_expr("echo"), literal_expr("hello")]
        }

        pub fn build(&self) -> TestCaseExpr {
            TestCaseExpr {
                name: self.name.clone(),
                filename: self.filename.to_string(),
                path: self.path.to_string(),
                processes: self.processes.build(),
                status_matchers: self
                    .status_matchers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
                stdout_matchers: self
                    .stdout_matchers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
                stderr_matchers: self
                    .stderr_matchers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            }
        }
    }

    impl Default for TestCaseExprTemplate {
        fn default() -> Self {
            Self {
                name: None,
                filename: TestCaseExprTemplate::DEFAULT_FILENAME,
                path: TestCaseExprTemplate::DEFAULT_PATH,
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate::default()),
                status_matchers: IndexMap::new(),
                stdout_matchers: IndexMap::new(),
                stderr_matchers: IndexMap::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod eval_test_case_expr {
        use crate::{
            expr::{
                testutil::{env_var_expr, literal_expr},
                SetupTmpFileHook,
            },
            matcher::testutil::{
                new_test_matcher_registry, TestMatcher, PARSE_ERROR_MATCHER, SUCCESS_MATCHER,
                VIOLATION_MESSAGE,
            },
            test_case_expr::testutil::{
                ProcessExprTemplate, ProcessesExprTemplate, TestCaseExprTemplate,
            },
            tmp_dir::testutil::StubTmpDirFactory,
        };

        use super::*;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;
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
            name: TestCaseExprTemplate::NAME_FOR_DEFAULT_COMMAND.to_string(),
            filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
            path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
            processes: indexmap! {
                "main".to_string() => Process {
                    command: vec!["echo".to_string(), "hello".to_string()],
                    stdin: "".to_string(),
                    env: vec![],
                    timeout: Duration::from_secs(10),
                    tee_stdout: false,
                    tee_stderr: false,
                }
            },
            status_matchers: vec![],
            stdout_matchers: vec![],
            stderr_matchers: vec![],
            setup_hooks: vec![],
            teardown_hooks: vec![],
        }])]
        #[case("with name",
            TestCaseExprTemplate {
                name: Some(literal_expr("mytest")),
                ..Default::default()
            },
            vec![
                TestCase {
                    name: "mytest".to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: vec!["echo".to_string(), "hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            tee_stdout: false,
                            tee_stderr: false,
                        }
                    },
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with stdin case",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    stdin: literal_expr("hello"),
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                TestCase {
                    name: TestCaseExprTemplate::NAME_FOR_DEFAULT_COMMAND.to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: vec!["echo".to_string(), "hello".to_string()],
                            stdin: "hello".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            tee_stdout: false,
                            tee_stderr: false,
                        }
                    },
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with env case",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    env: vec![("MESSAGE1", literal_expr("hello")), ("MESSAGE2", literal_expr("world"))],
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                TestCase {
                    name: TestCaseExprTemplate::NAME_FOR_DEFAULT_COMMAND.to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: vec!["echo".to_string(), "hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![("MESSAGE1".to_string(), "hello".to_string()), ("MESSAGE2".to_string(), "world".to_string())],
                            timeout: Duration::from_secs(10),
                            tee_stdout: false,
                            tee_stderr: false,
                        }
                    },
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with status matcher case",
            TestCaseExprTemplate {
                status_matchers: indexmap!{
                    SUCCESS_MATCHER => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                TestCase {
                    name: TestCaseExprTemplate::NAME_FOR_DEFAULT_COMMAND.to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: vec!["echo".to_string(), "hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            tee_stdout: false,
                            tee_stderr: false,
                        }
                    },
                    status_matchers: vec!(TestMatcher::new_success(Value::from(true))),
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with stdout matcher case",
            TestCaseExprTemplate {
                stdout_matchers: indexmap!{
                    SUCCESS_MATCHER => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                TestCase {
                    name: TestCaseExprTemplate::NAME_FOR_DEFAULT_COMMAND.to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: vec!["echo".to_string(), "hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            tee_stdout: false,
                            tee_stderr: false,
                        }
                    },
                    status_matchers: vec![],
                    stdout_matchers: vec![TestMatcher::new_success(Value::from(true))],
                    stderr_matchers: vec![],
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with stderr matcher case",
            TestCaseExprTemplate {
                stderr_matchers: indexmap!{
                    SUCCESS_MATCHER => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                TestCase {
                    name: TestCaseExprTemplate::NAME_FOR_DEFAULT_COMMAND.to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: vec!["echo".to_string(), "hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            tee_stdout: false,
                            tee_stderr: false,
                        }
                    },
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![TestMatcher::new_success(Value::from(true))],
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        fn success_cases(
            #[case] title: &str,
            #[case] given: TestCaseExprTemplate,
            #[case] expected: Vec<TestCase>,
        ) {
            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_supplier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let status_mr = new_test_matcher_registry();
            let stream_mr = new_test_matcher_registry();

            let actual = eval_test_expr(
                &mut tmp_dir_supplier,
                &status_mr,
                &stream_mr,
                &given.build(),
            );

            assert_eq!(Ok(expected), actual, "{}", title);
        }

        #[test]
        fn success_case_with_tmp_dir() {
            let tmp_dir = tempfile::tempdir().unwrap();
            let tmp_dir_path_buf = tmp_dir.path().to_path_buf();
            let mut tmp_dir_supplier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let status_mr = new_test_matcher_registry();
            let stream_mr = new_test_matcher_registry();

            let given = TestCaseExprTemplate {
                name: Some(literal_expr("test")),
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    command: vec![
                        literal_expr("cat"),
                        Expr::TmpFile("input.txt".to_string(), Box::new(literal_expr("hello"))),
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            };

            let actual = eval_test_expr(
                &mut tmp_dir_supplier,
                &status_mr,
                &stream_mr,
                &given.build(),
            );

            let tmp_file_path_buf = tmp_dir_path_buf.join("input.txt");

            let expected = vec![TestCase {
                name: "test".to_string(),
                filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                processes: indexmap! {
                    "main".to_string() => Process {
                        command: vec![
                            "cat".to_string(),
                            tmp_file_path_buf.to_str().unwrap().to_string(),
                        ],
                        stdin: "".to_string(),
                        env: vec![],
                        timeout: Duration::from_secs(10),
                        tee_stdout: false,
                        tee_stderr: false,
                    }
                },
                status_matchers: vec![],
                stdout_matchers: vec![],
                stderr_matchers: vec![],
                setup_hooks: vec![Box::new(SetupTmpFileHook {
                    path: tmp_file_path_buf.clone(),
                    contents: "hello".to_string(),
                })],
                teardown_hooks: vec![],
            }];

            assert_eq!(Ok(expected), actual);
        }

        #[rstest]
        #[case("with eval error in name",
            TestCaseExprTemplate {
                name: Some(env_var_expr("_undefined")),
                ..Default::default()
            },
            vec![
                violation(".name", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with not string name",
            TestCaseExprTemplate {
                name: Some(literal_expr(true)),
                ..Default::default()
            },
            vec![
                violation(".name", "should be string, but is bool"),
            ]
        )]
        #[case("with eval error in command",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    command: vec![literal_expr(true), env_var_expr("_undefined")],
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".command[0]", "should be string, but is bool"),
                violation(".command[1]", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with eval error in multiple processess's command",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Multi(indexmap! {
                    "process1" => ProcessExprTemplate {
                        command: vec![literal_expr(true), env_var_expr("_undefined")],
                        ..Default::default()
                    }
                }),
                ..Default::default()
            },
            vec![
                violation(".process1.command[0]", "should be string, but is bool"),
                violation(".process1.command[1]", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with eval error in env",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    env: vec![("MESSAGE1", literal_expr(true)), ("MESSAGE2", env_var_expr("_undefined"))],
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".env.MESSAGE1", "should be string, but is bool"),
                violation(".env.MESSAGE2", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with not string stdin",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    stdin: literal_expr(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".stdin", "should be string, but is bool"),
            ]
        )]
        #[case("with eval error in stdin",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    stdin: env_var_expr("_undefined"),
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".stdin", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with eval error in status matcher param",
            TestCaseExprTemplate {
                status_matchers: indexmap!{
                    SUCCESS_MATCHER => env_var_expr("_undefined"),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.status.success", "eval error: env var _undefined is not defined")
            ]
        )]
        #[case("with undefined status matcher",
            TestCaseExprTemplate {
                status_matchers: indexmap!{
                    "unknown" => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.status", "test matcher unknown is not defined")
            ]
        )]
        #[case("with invalid status matcher",
            TestCaseExprTemplate {
                status_matchers: indexmap!{
                    PARSE_ERROR_MATCHER => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.status.parse_error", VIOLATION_MESSAGE)
            ]
        )]
        #[case("with eval error in stdout matcher param",
            TestCaseExprTemplate {
                stdout_matchers: indexmap!{
                    SUCCESS_MATCHER => env_var_expr("_undefined"),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.stdout.success", "eval error: env var _undefined is not defined")
            ]
        )]
        #[case("with undefined stdout matcher",
            TestCaseExprTemplate {
                stdout_matchers: indexmap!{
                    "unknown" => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.stdout", "test matcher unknown is not defined")
            ]
        )]
        #[case("with invalid stdout matcher",
            TestCaseExprTemplate {
                stdout_matchers: indexmap!{
                    PARSE_ERROR_MATCHER => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.stdout.parse_error", VIOLATION_MESSAGE)
            ]
        )]
        #[case("with eval error in stdout matcher param",
            TestCaseExprTemplate {
                stdout_matchers: indexmap!{
                    SUCCESS_MATCHER => env_var_expr("_undefined"),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.stdout.success", "eval error: env var _undefined is not defined")
            ]
        )]
        #[case("with undefined stderr matcher",
            TestCaseExprTemplate {
                stderr_matchers: indexmap!{
                    "unknown" => literal_expr(true),
                },
                ..Default::default()
            },
            vec![
                violation(".expect.stderr", "test matcher unknown is not defined")
            ]
        )]
        #[case("with invalid stderr matcher",
            TestCaseExprTemplate {
                stderr_matchers: indexmap!{
                    PARSE_ERROR_MATCHER => literal_expr(true),
                },
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
            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_supplier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let status_mr = new_test_matcher_registry();
            let stream_mr = new_test_matcher_registry();

            let actual = eval_test_expr(
                &mut tmp_dir_supplier,
                &status_mr,
                &stream_mr,
                &given.build(),
            );

            assert_eq!(
                Err(TestExprError {
                    violations: expected_violations
                }),
                actual,
                "{}",
                title
            );
        }
    }
}
