use std::{net::TcpListener, time::Duration};

use indexmap::{indexmap, IndexMap};
use saphyr::Yaml;

use crate::{
    expr::{Context, EvalOutput, Expr},
    matcher::{StatusMatcher, StreamMatcher},
    test_case::{
        setup_hook::SetupHook, BackgroundConfig, Process, ProcessMode, TestCase, WaitCondition,
    },
    tmp_dir::TmpDirSupplier,
    validator::{Validator, Violation},
};

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct TestExprError {
    pub violations: Vec<Violation>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct BackgroundConfigExpr {
    pub wait_condition: Option<WaitConditionExpr>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct WaitConditionExpr {
    pub name: String,
    pub params: IndexMap<String, Expr>,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ProcessModeExpr {
    Foreground,
    Background(BackgroundConfigExpr),
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct ProcessExpr {
    pub command: Expr,
    pub args: Vec<Expr>,
    pub stdin: Expr,
    pub env: Vec<(String, Expr)>,
    pub timeout: Duration,
    pub mode: ProcessModeExpr,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ProcessesExpr {
    Single(ProcessExpr),
    Multi(IndexMap<String, ProcessExpr>),
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct ProcessMatchersExpr {
    pub status_matcher_exprs: IndexMap<String, Expr>,
    pub stdout_matcher_exprs: IndexMap<String, Expr>,
    pub stderr_matcher_exprs: IndexMap<String, Expr>,
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum ProcessesMatchersExpr {
    Single(ProcessMatchersExpr),
    Multi(IndexMap<String, ProcessMatchersExpr>),
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct TestCaseExpr {
    pub name: Option<Expr>,
    pub filename: String,
    pub path: String,
    pub let_decls: IndexMap<String, Expr>,
    pub processes: ProcessesExpr,
    pub processes_matchers: ProcessesMatchersExpr,
    pub files_matchers: IndexMap<String, IndexMap<String, Expr>>,
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct TestCaseExprFile {
    pub filename: String,
    pub test_case_exprs: Vec<TestCaseExpr>,
}

type ProcessMatchersTuple = (
    Vec<(StatusMatcher, bool)>,
    Vec<(StreamMatcher, bool)>,
    Vec<(StreamMatcher, bool)>,
);

const DEFAULT_PROCESS_NAME: &str = "main";

pub fn eval_test_expr<T: TmpDirSupplier>(
    tmp_dir_supplier: &mut T,
    tmp_port_reservers: &mut IndexMap<u16, TcpListener>,
    test_case_expr: &TestCaseExpr,
) -> Result<Vec<TestCase>, TestExprError> {
    let mut v =
        Validator::new_with_paths(&test_case_expr.filename, vec![test_case_expr.path.clone()]);
    let mut ctx = Context::new(tmp_dir_supplier, tmp_port_reservers);
    let mut setup_hooks: Vec<SetupHook> = vec![];

    test_case_expr.let_decls.iter().for_each(|(name, expr)| {
        if let Err(message) = ctx.eval_expr(expr).and_then(|output| {
            setup_hooks.extend(output.setup_hooks);
            ctx.define_var(name.clone(), output.value)
        }) {
            v.in_field(name, |v| {
                v.add_violation(format!("eval error: {}", message))
            });
        }
    });

    let mut processes_matchers: IndexMap<
        String,
        ProcessMatchersTuple,
    > = v.in_field("expect", |v| match &test_case_expr.processes_matchers {
        ProcessesMatchersExpr::Single(pm) => {
            indexmap! {
                DEFAULT_PROCESS_NAME.to_string() => (
                    eval_matcher_exprs(v, &mut ctx, "status", StatusMatcher::parse, &pm.status_matcher_exprs),
                    eval_matcher_exprs(v, &mut ctx, "stdout", StreamMatcher::parse, &pm.stdout_matcher_exprs),
                    eval_matcher_exprs(v, &mut ctx, "stderr", StreamMatcher::parse, &pm.stderr_matcher_exprs),
                )
            }
        }
        ProcessesMatchersExpr::Multi(process_matcher_exprs) => process_matcher_exprs
            .iter()
            .map(|(process_name, pm)| {
                v.in_field(process_name, |v| {
                    (
                        process_name.clone(),
                        (
                            eval_matcher_exprs(
                                v,
                                &mut ctx,
                                "status",
                                StatusMatcher::parse,
                                &pm.status_matcher_exprs,
                            ),
                            eval_matcher_exprs(
                                v,
                                &mut ctx,
                                "stdout",
                                StreamMatcher::parse,
                                &pm.stdout_matcher_exprs,
                            ),
                            eval_matcher_exprs(
                                v,
                                &mut ctx,
                                "stderr",
                                StreamMatcher::parse,
                                &pm.stderr_matcher_exprs,
                            ),
                        ),
                    )
                })
            })
            .collect(),
    });

    let processes = match &test_case_expr.processes {
        ProcessesExpr::Single(process_expr) => {
            let (status_matchers, stdout_matchers, stderr_matchers) = processes_matchers
                .shift_remove(DEFAULT_PROCESS_NAME)
                .unwrap_or_default();
            indexmap! { DEFAULT_PROCESS_NAME.to_string() => eval_process_expr(&mut v, &mut ctx, &mut setup_hooks, status_matchers, stdout_matchers, stderr_matchers, process_expr) }
        }
        ProcessesExpr::Multi(process_exprs) => process_exprs
            .iter()
            .map(|(name, process_expr)| {
                (
                    name.clone(),
                    v.in_field(name, |v| {
                        let (status_matchers, stdout_matchers, stderr_matchers) =
                            processes_matchers.shift_remove(name).unwrap_or_default();
                        eval_process_expr(
                            v,
                            &mut ctx,
                            &mut setup_hooks,
                            status_matchers,
                            stdout_matchers,
                            stderr_matchers,
                            process_expr,
                        )
                    }),
                )
            })
            .collect(),
    };

    if !processes_matchers.is_empty() {
        panic!(
            "processes_matchers contains unmatched processes: {:?}",
            processes_matchers.keys().collect::<Vec<_>>()
        );
    }

    let files_matchers = v.in_field("expect.files", |v| {
        test_case_expr
            .files_matchers
            .iter()
            .map(|(path, matcher_exprs)| {
                (
                    path.clone(),
                    eval_matcher_exprs(v, &mut ctx, path, StreamMatcher::parse, matcher_exprs),
                )
            })
            .collect()
    });

    let name = if let Some(name_expr) = &test_case_expr.name {
        v.in_field("name", |v| match ctx.eval_expr(name_expr) {
            Ok(EvalOutput {
                value,
                setup_hooks: setup_hook,
            }) => {
                setup_hooks.extend(setup_hook);
                v.must_be_string(&value)
            }
            Err(message) => {
                v.add_violation(format!("eval error: {}", message));
                None
            }
        })
    } else {
        let process = processes.values().last().unwrap();
        let mut command_and_args = vec![process.command.clone()];
        command_and_args.extend(process.args.clone());
        Some(
            command_and_args
                .iter()
                .map(|x| yash_quote::quote(x))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
    .unwrap_or("".to_string());

    if v.violations.is_empty() {
        Ok(vec![TestCase {
            name,
            filename: test_case_expr.filename.clone(),
            path: test_case_expr.path.clone(),
            processes,
            files_matchers,
            setup_hooks,
            teardown_hooks: vec![],
        }])
    } else {
        Err(TestExprError {
            violations: v.violations,
        })
    }
}

fn eval_matcher_exprs<
    T,
    TS: TmpDirSupplier,
    F: Fn(&mut Validator, &str, &Yaml) -> Option<(T, bool)>,
>(
    v: &mut Validator,
    ctx: &mut Context<'_, '_, TS>,
    subject: &str,
    parse: F,
    matcher_exprs: &IndexMap<String, Expr>,
) -> Vec<(T, bool)> {
    v.in_field(subject, |v| {
        matcher_exprs
            .iter()
            .filter_map(|(name, param_expr)| match ctx.eval_expr(param_expr) {
                Ok(param) => parse(v, name, &param.value),
                Err(message) => {
                    v.in_field(name, |v| {
                        v.add_violation(format!("eval error: {}", message))
                    });
                    None
                }
            })
            .collect()
    })
}

fn eval_process_expr<T: TmpDirSupplier>(
    v: &mut Validator,
    ctx: &mut Context<'_, '_, T>,
    setup_hooks: &mut Vec<SetupHook>,
    status_matchers: Vec<(StatusMatcher, bool)>,
    stdout_matchers: Vec<(StreamMatcher, bool)>,
    stderr_matchers: Vec<(StreamMatcher, bool)>,
    process_expr: &ProcessExpr,
) -> Process {
    let command = v.in_field("command[0]", |v| {
        match ctx.eval_expr(&process_expr.command) {
            Ok(EvalOutput {
                value,
                setup_hooks: setup_hook,
            }) => {
                setup_hooks.extend(setup_hook);
                v.must_be_string(&value).unwrap_or_default()
            }
            Err(message) => {
                v.add_violation(format!("eval error: {}", message));
                "".to_string()
            }
        }
    });

    let args: Vec<String> = v.in_field("command", |v| {
        process_expr
            .args
            .iter()
            .enumerate()
            .filter_map(|(i, x)| match ctx.eval_expr(x) {
                Ok(EvalOutput {
                    value,
                    setup_hooks: output_setup_hooks,
                }) => {
                    setup_hooks.extend(output_setup_hooks);
                    v.in_index(i + 1, |v| v.must_be_string(&value))
                }
                Err(message) => {
                    v.in_index(i + 1, |v| {
                        v.add_violation(format!("eval error: {}", message))
                    });
                    None
                }
            })
            .collect()
    });

    let stdin = v
        .in_field("stdin", |v| match ctx.eval_expr(&process_expr.stdin) {
            Ok(EvalOutput {
                value,
                setup_hooks: output_setup_hooks,
            }) => {
                setup_hooks.extend(output_setup_hooks);
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
                    Ok(EvalOutput {
                        value,
                        setup_hooks: output_setup_hooks,
                    }) => {
                        setup_hooks.extend(output_setup_hooks);
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

    let mode = match &process_expr.mode {
        ProcessModeExpr::Foreground => ProcessMode::Foreground,
        ProcessModeExpr::Background(BackgroundConfigExpr { wait_condition }) => {
            v.in_field("background", |v| {
                v.in_field("wait_for", |v| {
                    wait_condition
                        .as_ref()
                        .and_then(|wait_condition| {
                            let params: Option<IndexMap<&String, Yaml>> = wait_condition
                                .params
                                .iter()
                                .map(|(k, expr)| match ctx.eval_expr(expr) {
                                    Ok(EvalOutput {
                                        value,
                                        setup_hooks: output_setup_hooks,
                                    }) => {
                                        setup_hooks.extend(output_setup_hooks);
                                        Some((k, value))
                                    }
                                    Err(message) => {
                                        v.in_field(k, |v| {
                                            v.add_violation(format!("eval error: {}", message))
                                        });
                                        None
                                    }
                                })
                                .collect();

                            params.and_then(|params| {
                                WaitCondition::parse(
                                    v,
                                    &wait_condition.name,
                                    &params.iter().map(|(k, v)| (k.as_str(), v)).collect(),
                                )
                                .map(|wait_condition| {
                                    ProcessMode::Background(BackgroundConfig { wait_condition })
                                })
                            })
                        })
                        .unwrap_or(ProcessMode::Background(BackgroundConfig::default()))
                })
            })
        }
    };

    Process {
        command,
        args,
        stdin,
        env,
        status_matchers,
        stdout_matchers,
        stderr_matchers,
        timeout: process_expr.timeout,
        mode,
        tee_stdout: process_expr.tee_stdout,
        tee_stderr: process_expr.tee_stderr,
    }
}

#[cfg(test)]
pub mod testutil {
    use std::time::Duration;

    use indexmap::indexmap;
    use indexmap::IndexMap;
    use saphyr::Yaml;

    use crate::expr::Expr;

    use crate::expr::testutil::*;

    use super::ProcessExpr;
    use super::ProcessMatchersExpr;
    use super::ProcessModeExpr;
    use super::ProcessesExpr;
    use super::ProcessesMatchersExpr;
    use super::TestCaseExpr;

    pub struct ProcessExprTemplate {
        pub command: Expr,
        pub args: Vec<Expr>,
        pub stdin: Expr,
        pub env: Vec<(&'static str, Expr)>,
        pub timeout: u64,
        pub mode: ProcessModeExpr,
        pub tee_stdout: bool,
        pub tee_stderr: bool,
    }

    impl ProcessExprTemplate {
        pub fn build(self) -> ProcessExpr {
            ProcessExpr {
                command: self.command.clone(),
                args: self.args.clone(),
                stdin: self.stdin.clone(),
                env: self
                    .env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
                timeout: Duration::from_secs(self.timeout),
                mode: self.mode,
                tee_stdout: self.tee_stdout,
                tee_stderr: self.tee_stderr,
            }
        }
    }

    impl Default for ProcessExprTemplate {
        fn default() -> Self {
            Self {
                command: TestCaseExprTemplate::default_command(),
                args: TestCaseExprTemplate::default_args(),
                stdin: literal_expr(Yaml::String("".to_string())),
                env: vec![],
                timeout: 10,
                mode: ProcessModeExpr::Foreground,
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
        pub fn build(self) -> ProcessesExpr {
            match self {
                ProcessesExprTemplate::Single(p) => ProcessesExpr::Single(p.build()),
                ProcessesExprTemplate::Multi(ps) => ProcessesExpr::Multi(
                    ps.into_iter()
                        .map(|(k, v)| (k.to_string(), v.build()))
                        .collect(),
                ),
            }
        }
    }

    pub struct ProcessMatchersExprTemplate {
        pub status_matcher_exprs: IndexMap<&'static str, Expr>,
        pub stdout_matcher_exprs: IndexMap<&'static str, Expr>,
        pub stderr_matcher_exprs: IndexMap<&'static str, Expr>,
    }

    impl ProcessMatchersExprTemplate {
        pub fn build(self) -> ProcessMatchersExpr {
            ProcessMatchersExpr {
                status_matcher_exprs: self
                    .status_matcher_exprs
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                stdout_matcher_exprs: self
                    .stdout_matcher_exprs
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                stderr_matcher_exprs: self
                    .stderr_matcher_exprs
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
            }
        }
    }

    impl Default for ProcessMatchersExprTemplate {
        fn default() -> Self {
            Self {
                status_matcher_exprs: indexmap! {},
                stdout_matcher_exprs: indexmap! {},
                stderr_matcher_exprs: indexmap! {},
            }
        }
    }

    pub enum ProcessesMatchersExprTemplate {
        Single(ProcessMatchersExprTemplate),
        Multi(IndexMap<&'static str, ProcessMatchersExprTemplate>),
    }

    impl ProcessesMatchersExprTemplate {
        pub fn build(self) -> ProcessesMatchersExpr {
            match self {
                ProcessesMatchersExprTemplate::Single(pm) => {
                    ProcessesMatchersExpr::Single(pm.build())
                }
                ProcessesMatchersExprTemplate::Multi(matchers) => ProcessesMatchersExpr::Multi(
                    matchers
                        .into_iter()
                        .map(|(process_name, pm)| (process_name.to_string(), pm.build()))
                        .collect(),
                ),
            }
        }
    }

    pub struct TestCaseExprTemplate {
        pub name: Option<Expr>,
        pub filename: &'static str,
        pub path: &'static str,
        pub let_decls: IndexMap<&'static str, Expr>,
        pub processes: ProcessesExprTemplate,
        pub processes_matchers: ProcessesMatchersExprTemplate,
        pub files_matchers: IndexMap<&'static str, IndexMap<&'static str, Expr>>,
    }

    impl TestCaseExprTemplate {
        pub const NAME_FOR_DEFAULT_COMMAND: &'static str = "echo hello";
        pub const DEFAULT_FILENAME: &'static str = "test.yaml";
        pub const DEFAULT_PATH: &'static str = "$.tests[0]";

        pub fn default_command() -> Expr {
            literal_expr(Yaml::String("echo".to_string()))
        }

        pub fn default_args() -> Vec<Expr> {
            vec![literal_expr(Yaml::String("hello".to_string()))]
        }

        pub fn build(self) -> TestCaseExpr {
            TestCaseExpr {
                name: self.name.clone(),
                filename: self.filename.to_string(),
                path: self.path.to_string(),
                let_decls: self
                    .let_decls
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                processes: self.processes.build(),
                processes_matchers: self.processes_matchers.build(),
                files_matchers: self
                    .files_matchers
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k.to_string(),
                            v.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                        )
                    })
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
                let_decls: indexmap! {},
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate::default()),
                processes_matchers: ProcessesMatchersExprTemplate::Multi(indexmap! {}),
                files_matchers: indexmap! {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod eval_test_case_expr {
        use crate::{
            expr::testutil::{env_var_expr, literal_expr, var_expr},
            matcher::testutil::{
                new_status_test_success, new_stream_test_success, PARSE_ERROR_VIOLATION_MESSAGE,
                TEST_PARSE_ERROR_NAME, TEST_SUCCESS_NAME, TEST_SUCCESS_NAME_WITH_NOT,
            },
            test_case::{setup_hook::SetupHook, BackgroundConfig, ProcessMode},
            test_case_expr::testutil::{
                ProcessExprTemplate, ProcessMatchersExprTemplate, ProcessesExprTemplate,
                ProcessesMatchersExprTemplate, TestCaseExprTemplate,
            },
            tmp_dir::testutil::StubTmpDirFactory,
        };

        use super::*;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

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
                    command: "echo".to_string(),
                    args: vec!["hello".to_string()],
                    stdin: "".to_string(),
                    env: vec![],
                    timeout: Duration::from_secs(10),
                    mode: ProcessMode::Foreground,
                    tee_stdout: false,
                    tee_stderr: false,
                    status_matchers: vec![],
                    stdout_matchers: vec![],
                    stderr_matchers: vec![],
                }
            },
            files_matchers: indexmap! {},
            setup_hooks: vec![],
            teardown_hooks: vec![],
        }])]
        #[case("with name",
            TestCaseExprTemplate {
                name: Some(literal_expr(Yaml::String("mytest".to_string()))),
                ..Default::default()
            },
            vec![
                TestCase {
                    name: "mytest".to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("wuth multi processes case",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Multi(indexmap! {
                    "process1" => ProcessExprTemplate {
                        mode: ProcessModeExpr::Background(BackgroundConfigExpr {
                            wait_condition: Some(WaitConditionExpr {
                                name: "success_stub".to_string(),
                                params: indexmap! { "answer".to_string() => literal_expr(Yaml::Integer(42)) }
                            }),
                        }),
                        ..Default::default()
                    },
                    "process2" => ProcessExprTemplate::default(),
                }),
                ..Default::default()
            },
            vec![
                TestCase {
                    name: TestCaseExprTemplate::NAME_FOR_DEFAULT_COMMAND.to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "process1".to_string() => Process {
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Background(BackgroundConfig {
                                wait_condition: WaitCondition::SuccessStub(indexmap! { "answer".to_string() => Yaml::Integer(42) }),
                            }),
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        },
                        "process2".to_string() => Process {
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                }
            ]
        )]
        #[case("with stdin case",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    stdin: literal_expr(Yaml::String("hello".to_string())),
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
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "hello".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with env case",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    env: vec![("MESSAGE1", literal_expr(Yaml::String("hello".to_string()))), ("MESSAGE2", literal_expr(Yaml::String("world".to_string())))],
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
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![("MESSAGE1".to_string(), "hello".to_string()), ("MESSAGE2".to_string(), "world".to_string())],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with let case",
            TestCaseExprTemplate {
                let_decls: indexmap! {
                    "MESSAGE" => literal_expr(Yaml::String("hello".to_string())),
                    "MESSAGE2" => var_expr("MESSAGE"),
                },
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![var_expr("MESSAGE"), var_expr("MESSAGE2")],
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                TestCase {
                    name: "echo hello hello".to_string(),
                    filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                    path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: "echo".to_string(),
                            args: vec!["hello".to_string(), "hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with status matcher case",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap!{
                        TEST_SUCCESS_NAME => literal_expr(Yaml::Boolean(true)),
                        TEST_SUCCESS_NAME_WITH_NOT => literal_expr(Yaml::Boolean(true)),
                    },
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
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![
                                (new_status_test_success(Yaml::Boolean(true)), true),
                                (new_status_test_success(Yaml::Boolean(true)), false),
                            ],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with stdout matcher case",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap!{
                        TEST_SUCCESS_NAME => literal_expr(Yaml::Boolean(true)),
                        TEST_SUCCESS_NAME_WITH_NOT => literal_expr(Yaml::Boolean(true)),
                    },
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
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![
                                (new_stream_test_success(Yaml::Boolean(true)), true),
                                (new_stream_test_success(Yaml::Boolean(true)), false),
                            ],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with stderr matcher case",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stderr_matcher_exprs: indexmap! {
                        TEST_SUCCESS_NAME => literal_expr(Yaml::Boolean(true)),
                        TEST_SUCCESS_NAME_WITH_NOT => literal_expr(Yaml::Boolean(true)),
                    },
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
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![
                                (new_stream_test_success(Yaml::Boolean(true)), true),
                                (new_stream_test_success(Yaml::Boolean(true)), false),
                            ],
                        }
                    },
                    files_matchers: indexmap! {},
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                },
            ]
        )]
        #[case("with file matcher case",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    ..Default::default()
                }),
                files_matchers: indexmap! {
                    "/tmp/output.txt" => indexmap! {
                        TEST_SUCCESS_NAME => literal_expr(Yaml::Boolean(true)),
                        TEST_SUCCESS_NAME_WITH_NOT => literal_expr(Yaml::Boolean(true)),
                    },
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
                            command: "echo".to_string(),
                            args: vec!["hello".to_string()],
                            stdin: "".to_string(),
                            env: vec![],
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! {
                        "/tmp/output.txt".to_string() => vec![
                            (new_stream_test_success(Yaml::Boolean(true)), true),
                            (new_stream_test_success(Yaml::Boolean(true)), false),
                        ],
                    },
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
            let mut tmp_port_reserver = indexmap! {};

            let actual = eval_test_expr(
                &mut tmp_dir_supplier,
                &mut tmp_port_reserver,
                &given.build(),
            );

            assert_eq!(Ok(expected), actual, "{}", title);
        }

        #[rstest]
        fn success_case_with_tmp_dir() {
            let tmp_dir = tempfile::tempdir().unwrap();
            let tmp_dir_path_buf = tmp_dir.path().to_path_buf();
            let mut tmp_dir_supplier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let mut tmp_port_reserver = indexmap! {};

            let given = TestCaseExprTemplate {
                name: Some(literal_expr(Yaml::String("test".to_string()))),
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    command: literal_expr(Yaml::String("cat".to_string())),
                    args: vec![Expr::TmpFile(
                        "input.txt".to_string(),
                        Box::new(literal_expr(Yaml::String("hello".to_string()))),
                    )],
                    ..Default::default()
                }),
                ..Default::default()
            };

            let actual = eval_test_expr(
                &mut tmp_dir_supplier,
                &mut tmp_port_reserver,
                &given.build(),
            );

            let tmp_file_path_buf = tmp_dir_path_buf.join("input.txt");

            let expected = vec![TestCase {
                name: "test".to_string(),
                filename: TestCaseExprTemplate::DEFAULT_FILENAME.to_string(),
                path: TestCaseExprTemplate::DEFAULT_PATH.to_string(),
                processes: indexmap! {
                    "main".to_string() => Process {
                        command: "cat".to_string(),
                        args: vec![
                            tmp_file_path_buf.to_str().unwrap().to_string(),
                        ],
                        stdin: "".to_string(),
                        env: vec![],
                        timeout: Duration::from_secs(10),
                        mode: ProcessMode::Foreground,
                        tee_stdout: false,
                        tee_stderr: false,
                        status_matchers: vec![],
                        stdout_matchers: vec![],
                        stderr_matchers: vec![],
                    }
                },
                files_matchers: indexmap! {},
                setup_hooks: vec![SetupHook::new_tmp_file(
                    tmp_file_path_buf.clone(),
                    "hello".to_string(),
                )],
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
                name: Some(literal_expr(Yaml::Boolean(true))),
                ..Default::default()
            },
            vec![
                violation(".name", "should be string, but is bool"),
            ]
        )]
        #[case("with eval error in command",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    command: literal_expr(Yaml::Boolean(true)),
                    args: vec![env_var_expr("_undefined")],
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
                        command: literal_expr(Yaml::Boolean(true)),
                        args: vec![env_var_expr("_undefined")],
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
                    env: vec![("MESSAGE1", literal_expr(Yaml::Boolean(true))), ("MESSAGE2", env_var_expr("_undefined_env")), ("MESSAGE3", var_expr("_undefined_var"))],
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".env.MESSAGE1", "should be string, but is bool"),
                violation(".env.MESSAGE2", "eval error: env var _undefined_env is not defined"),
                violation(".env.MESSAGE3", "eval error: variable _undefined_var is not defined"),
            ]
        )]
        #[case("with eval error in background.wait_for",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    mode: ProcessModeExpr::Background(BackgroundConfigExpr {
                        wait_condition: Some(WaitConditionExpr{
                            name: "success_stub".to_string(),
                            params: indexmap!{
                                "x".to_string() => env_var_expr("_undefined"),
                            },
                        })
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".background.wait_for.x", "eval error: env var _undefined is not defined"),
            ]
        )]
        #[case("with invalid wait condition",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    mode: ProcessModeExpr::Background(BackgroundConfigExpr {
                        wait_condition: Some(WaitConditionExpr{
                            name: "unknown".to_string(),
                            params: indexmap!{},
                        })
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".background.wait_for.type", "\"unknown\" is not valid wait condition type"),
            ]
        )]
        #[case("with not string stdin",
            TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(ProcessExprTemplate {
                    stdin: literal_expr(Yaml::Boolean(true)),
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
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap!{
                        TEST_SUCCESS_NAME => env_var_expr("_undefined"),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.status.test_success", "eval error: env var _undefined is not defined")
            ]
        )]
        #[case("with undefined status matcher",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap!{
                        "unknown" => literal_expr(Yaml::Boolean(true)),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.status", "status matcher \"unknown\" is not defined")
            ]
        )]
        #[case("with invalid status matcher",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap! {
                        TEST_PARSE_ERROR_NAME => literal_expr(Yaml::Boolean(true)),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.status.test_parse_error", PARSE_ERROR_VIOLATION_MESSAGE)
            ]
        )]
        #[case("with eval error in stdout matcher param",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap! {
                        TEST_SUCCESS_NAME => env_var_expr("_undefined"),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.stdout.test_success", "eval error: env var _undefined is not defined")
            ]
        )]
        #[case("with undefined stdout matcher",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap! {
                        "unknown" => literal_expr(Yaml::Boolean(true)),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.stdout", "stream matcher \"unknown\" is not defined")
            ]
        )]
        #[case("with invalid stdout matcher",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap! {
                        TEST_PARSE_ERROR_NAME => literal_expr(Yaml::Boolean(true)),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.stdout.test_parse_error", PARSE_ERROR_VIOLATION_MESSAGE)
            ]
        )]
        #[case("with eval error in stdout matcher param",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap! {
                        TEST_SUCCESS_NAME => env_var_expr("_undefined"),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.stdout.test_success", "eval error: env var _undefined is not defined")
            ]
        )]
        #[case("with undefined stderr matcher",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stderr_matcher_exprs: indexmap! {
                        "unknown" => literal_expr(Yaml::Boolean(true)),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.stderr", "stream matcher \"unknown\" is not defined")
            ]
        )]
        #[case("with invalid stderr matcher",
            TestCaseExprTemplate {
                processes_matchers: ProcessesMatchersExprTemplate::Single(ProcessMatchersExprTemplate {
                    stderr_matcher_exprs: indexmap! {
                        TEST_PARSE_ERROR_NAME => literal_expr(Yaml::Boolean(true)),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            vec![
                violation(".expect.stderr.test_parse_error", PARSE_ERROR_VIOLATION_MESSAGE)
            ]
        )]
        #[case("with undefined file matcher",
            TestCaseExprTemplate {
                files_matchers: indexmap! {
                    "/tmp/output.txt" => indexmap! {
                        "unknown" => literal_expr(Yaml::Boolean(true)),
                    },
                },
                ..Default::default()
            },
            vec![
                violation(".expect.files./tmp/output.txt", "stream matcher \"unknown\" is not defined")
            ]
        )]
        #[case("with invalid file matcher",
            TestCaseExprTemplate {
                files_matchers: indexmap! {
                    "/tmp/output.txt" => indexmap! {
                        TEST_PARSE_ERROR_NAME => literal_expr(Yaml::Boolean(true)),
                    },
                },
                ..Default::default()
            },
            vec![
                violation(".expect.files./tmp/output.txt.test_parse_error", PARSE_ERROR_VIOLATION_MESSAGE)
            ]
        )]
        #[case("with eval error in files matcher param",
            TestCaseExprTemplate {
                files_matchers: indexmap! {
                    "/tmp/output.txt" => indexmap! {
                        TEST_SUCCESS_NAME => env_var_expr("_undefined"),
                    },
                },
                ..Default::default()
            },
            vec![
                violation(".expect.files./tmp/output.txt.test_success", "eval error: env var _undefined is not defined")
            ]
        )]
        fn failure_cases(
            #[case] title: &str,
            #[case] given: TestCaseExprTemplate,
            #[case] expected_violations: Vec<Violation>,
        ) {
            let tmp_dir = tempfile::tempdir().unwrap();
            let mut tmp_dir_supplier = StubTmpDirFactory { tmp_dir: &tmp_dir };
            let mut tmp_port_reserver = indexmap! {};
            let actual = eval_test_expr(
                &mut tmp_dir_supplier,
                &mut tmp_port_reserver,
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
