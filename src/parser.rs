use std::time::Duration;

use indexmap::{indexmap, IndexMap};
use once_cell::sync::Lazy;
use regex::Regex;

use serde_yaml::Value;

use crate::{
    ast::Map,
    expr::Expr,
    test_case::{
        wait_condition::{HttpCondition, SleepCondition},
        BackgroundConfig, ProcessMode, WaitCondition,
    },
    test_case_expr::{
        ProcessExpr, ProcessMatchersExpr, ProcessesExpr, ProcessesMatchersExpr, TestCaseExpr,
        TestCaseExprFile,
    },
    validator::{Validator, Violation},
};

#[derive(PartialEq, Debug)]
pub struct Error {
    pub filename: String,
    pub message: String,
    pub violations: Vec<Violation>,
}

impl Error {
    fn without_violations(filename: &str, message: impl Into<String>) -> Self {
        Self {
            filename: filename.to_string(),
            message: message.into(),
            violations: vec![],
        }
    }

    fn with_violations(
        filename: &str,
        message: impl Into<String>,
        violations: Vec<Violation>,
    ) -> Self {
        Self {
            filename: filename.to_string(),
            message: message.into(),
            violations,
        }
    }
}

const DEFAULT_TIMEOUT: u64 = 10;
static VAR_NAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap());

pub fn parse(filename: &str, reader: impl std::io::Read) -> Result<TestCaseExprFile, Error> {
    let ast = serde_yaml::from_reader(reader).map_err(|err| {
        Error::without_violations(filename, format!("cannot parse {}: {}", filename, err))
    })?;

    let mut v = Validator::new(filename);

    let test_case_exprs = v
        .must_be_map(&ast)
        .and_then(|root| {
            v.must_have_seq(&root, "tests", |v, tests| {
                v.map_seq(tests, |v, test| {
                    v.must_be_map(test).map(|test| {
                        let name = v.may_have(&test, "name", parse_expr);

                        let processes: ProcessesExpr = v
                            .may_have(&test, "processes", |v, processes| {
                                v.must_be_map(processes)
                                    .map(|processes| {
                                        if processes.is_empty() {
                                            v.add_violation("should not be empty");
                                        }
                                        ProcessesExpr::Multi(
                                            processes
                                                .iter()
                                                .filter_map(|(name, process)| {
                                                    v.in_field(name, |v| {
                                                        v.must_be_map(process).map(|process| {
                                                            (
                                                                name.to_string(),
                                                                parse_process(v, &process),
                                                            )
                                                        })
                                                    })
                                                })
                                                .collect(),
                                        )
                                    })
                                    .unwrap_or_else(|| ProcessesExpr::Multi(indexmap! {}))
                            })
                            .unwrap_or_else(|| ProcessesExpr::Single(parse_process(v, &test)));

                        let matcher_exprs: ProcessesMatchersExpr = v
                            .may_have_map(&test, "expect", |v, expect| {
                                v.may_have_map(expect, "processes", |v, processes| {
                                    ProcessesMatchersExpr::Multi(
                                        processes
                                            .iter()
                                            .filter_map(|(name, process)| {
                                                v.in_field(name, |v| {
                                                    v.must_be_map(process).map(|process| {
                                                        (
                                                            name.to_string(),
                                                            parse_expectations(v, &process),
                                                        )
                                                    })
                                                })
                                            })
                                            .collect(),
                                    )
                                })
                                .unwrap_or_else(|| {
                                    ProcessesMatchersExpr::Single(parse_expectations(v, expect))
                                })
                            })
                            .unwrap_or(ProcessesMatchersExpr::Multi(indexmap! {}));

                        if let (ProcessesExpr::Multi(_), ProcessesMatchersExpr::Single(_)) =
                            (&processes, &matcher_exprs)
                        {
                            v.in_field("expect", |v| {
                                v.add_violation(
                                    "expect should be multiple mode when multiple processes are given",
                                );
                            })
                        }

                        TestCaseExpr {
                            name,
                            filename: v.filename.clone(),
                            path: v.current_path(),
                            processes,
                            matchers: matcher_exprs,
                        }
                    })
                })
            })
        })
        .flatten();

    match test_case_exprs {
        Some(test_case_exprs) => {
            if v.violations.is_empty() {
                Ok(TestCaseExprFile {
                    filename: filename.to_string(),
                    test_case_exprs,
                })
            } else {
                Err(Error::with_violations(
                    filename,
                    "parse error",
                    v.violations,
                ))
            }
        }
        None => Err(Error::with_violations(
            filename,
            "parse error",
            v.violations,
        )),
    }
}

fn parse_process(v: &mut Validator, m: &Map) -> ProcessExpr {
    let command = v
        .must_have_seq(m, "command", |v, command| {
            if command.is_empty() {
                v.add_violation("should not be empty");
                None
            } else {
                v.map_seq(command, |v, x| Some(parse_expr(v, x)))
            }
        })
        .flatten()
        .unwrap_or_default();
    let stdin = v
        .may_have(m, "stdin", parse_expr)
        .unwrap_or(Expr::Literal(Value::from("")));
    let env: Vec<(String, Expr)> = v
        .may_have_map(m, "env", |v, env| {
            env.into_iter()
                .filter_map(|(name, value)| {
                    if !VAR_NAME_RE.is_match(name) {
                        v.add_violation(
                            "should have valid env var name (^[a-zA-Z_][a-zA-Z0-9_]*$)",
                        );
                        return None;
                    }
                    Some((name.to_string(), parse_expr(v, value)))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let timeout = v
        .may_have_duration(m, "timeout")
        .unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT));
    let mode = v
        .may_have_map(m, "background", |v, background| {
            let wait_condition = v
                .may_have_map(background, "wait_for", |v, wait_for| {
                    v.must_have_string(wait_for, "type")
                        .and_then(|condition_type| match &*condition_type {
                            "sleep" => {
                                let duration = v
                                    .must_have_duration(wait_for, "duration")
                                    .unwrap_or(Duration::from_secs(0));
                                Some(WaitCondition::Sleep(SleepCondition { duration }))
                            }
                            "http" => {
                                let port = v
                                    .must_have_uint(wait_for, "port")
                                    .and_then(|port64| {
                                        v.in_field("port", |v| {
                                            TryFrom::try_from(port64)
                                                .map_err(|_| {
                                                    v.add_violation("should be in range of u16");
                                                })
                                                .ok()
                                        })
                                    })
                                    .unwrap_or_default();
                                let path = v.must_have_string(wait_for, "path").unwrap_or_default();
                                let initial_delay = v
                                    .may_have_duration(wait_for, "initial_delay")
                                    .unwrap_or(Duration::from_secs(0));
                                let interval = v
                                    .may_have_duration(wait_for, "interval")
                                    .unwrap_or(Duration::from_secs(0));
                                let max_retry = v.may_have_uint(wait_for, "max_retry").unwrap_or(3);
                                let timeout = v
                                    .may_have_duration(wait_for, "timeout")
                                    .unwrap_or(Duration::from_secs(1));
                                Some(WaitCondition::Http(HttpCondition {
                                    port,
                                    path,
                                    initial_delay,
                                    interval,
                                    max_retry,
                                    timeout,
                                }))
                            }
                            _ => {
                                v.in_field("type", |v| {
                                    v.add_violation(format!(
                                        "\"{}\" is not valid wait condition type",
                                        condition_type
                                    ));
                                });
                                None
                            }
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            ProcessMode::Background(BackgroundConfig { wait_condition })
        })
        .unwrap_or(ProcessMode::Foreground);
    let tee_stdout = v.may_have_bool(m, "teeStdout").unwrap_or(false);
    let tee_stderr = v.may_have_bool(m, "teeStderr").unwrap_or(false);

    ProcessExpr {
        command,
        stdin,
        env,
        timeout,
        mode,
        tee_stdout,
        tee_stderr,
    }
}

fn parse_expr(v: &mut Validator, x: &Value) -> Expr {
    if v.may_be_string(x).is_some() {
        return Expr::Literal(x.clone());
    }

    v.may_be_qualified(x)
        .and_then(|(q, value)| match q {
            "env" => v.in_field(".$env", |v| {
                v.may_be_string(value).map(|name| Expr::EnvVar(name, None))
            }),
            "yaml" => Some(Expr::Yaml(value.clone())),
            "json" => Some(Expr::Json(value.clone())),
            "tmp_file" => v.in_field("$tmp_file", |v| {
                v.must_be_map(value).map(|m| {
                    let filename = v.must_have_string(&m, "filename").unwrap_or_default();
                    let contents = v
                        .must_have(&m, "contents", parse_expr)
                        .unwrap_or_else(|| Expr::Literal(Value::from(false)));
                    Expr::TmpFile(filename, Box::new(contents))
                })
            }),
            _ => None,
        })
        .unwrap_or_else(|| Expr::Literal(x.clone()))
}

fn parse_expectations(v: &mut Validator, m: &Map) -> ProcessMatchersExpr {
    let status_matcher_exprs = v
        .may_have_map(m, "status", parse_expected)
        .unwrap_or_default();
    let stdout_matcher_exprs = v
        .may_have_map(m, "stdout", parse_expected)
        .unwrap_or_default();
    let stderr_matcher_exprs = v
        .may_have_map(m, "stderr", parse_expected)
        .unwrap_or_default();
    ProcessMatchersExpr {
        status_matcher_exprs,
        stdout_matcher_exprs,
        stderr_matcher_exprs,
    }
}

fn parse_expected(v: &mut Validator, m: &Map) -> IndexMap<String, Expr> {
    let mut result = IndexMap::<String, Expr>::new();
    m.iter().for_each(|(name, value)| {
        result.insert(name.to_string(), parse_expr(v, value));
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    mod parse {

        use crate::{
            ast::testuitl::mapping,
            expr::{
                testutil::{env_var_expr, literal_expr},
                Expr,
            },
            test_case::{wait_condition::HttpCondition, BackgroundConfig},
            test_case_expr::testutil::{
                ProcessExprTemplate, ProcessMatchersExprTemplate, ProcessesExprTemplate,
                ProcessesMatchersExprTemplate, TestCaseExprTemplate,
            },
        };

        use super::*;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;
        use rstest::rstest;
        use serde_yaml::Value;

        const FILENAME: &str = "test.yaml";
        fn parse_error(violations: Vec<Violation>) -> Result<TestCaseExprFile, Error> {
            Err(Error::with_violations(
                FILENAME,
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

        #[rstest]
        #[case("with command only", "
tests:
    - command:
        - echo
        - hello", vec![TestCaseExprTemplate::default()])]
        #[case("with name", "
tests:
    - name: mytest
      command:
        - echo
        - hello", vec![TestCaseExprTemplate{name: Some(literal_expr("mytest")), ..TestCaseExprTemplate::default()}])]
        #[case("with command contains env var", "
tests:
    - command:
        - echo
        - $env: MESSAGE", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
                ProcessExprTemplate {
                    command: vec![
                        Expr::Literal(Value::from("echo".to_string())),
                        Expr::EnvVar("MESSAGE".to_string(), None),
                    ],
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with command contains timeout", "
tests:
    - command:
        - echo
        - hello
      timeout: 5s", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
                ProcessExprTemplate {
                    timeout: 5,
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with command cotains tee_stdout & tee_stderr", "
tests:
    - command:
        - echo
        - hello
      teeStdout: true
      teeStderr: true", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
            ProcessExprTemplate {
                    tee_stdout: true,
                    tee_stderr: true,
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with command contains simple stdin", "
tests:
    - command:
        - cat
      stdin: hello", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
                ProcessExprTemplate {
                    command: vec![Expr::Literal(Value::from("cat".to_string()))],
                    stdin: literal_expr("hello"),
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with command contains yaml stdin", "
tests:
    - command:
        - cat
      stdin:
        $yaml:
          message: hello", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
                ProcessExprTemplate {
                    command: vec![Expr::Literal(Value::from("cat".to_string()))],
                    stdin: Expr::Yaml(Value::from(mapping(vec![("message", Value::from("hello"))]))),
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with command contains json stdin", "
tests:
    - command:
        - cat
      stdin:
        $json:
          message: hello", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
                ProcessExprTemplate {
                    command: vec![Expr::Literal(Value::from("cat".to_string()))],
                    stdin: Expr::Json(Value::from(mapping(vec![("message", Value::from("hello"))]))),
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with command contains env", "
tests:
    - command:
        - echo
        - hello
      env:
        MESSAGE1: hello
        MESSAGE2:
          $env: FOO", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
                ProcessExprTemplate {
                    env: vec![("MESSAGE1", literal_expr("hello")), ("MESSAGE2", env_var_expr("FOO"))],
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case(
            "with command contains tmp_file",
            "
tests:
    - command:
        - cat
        - $tmp_file:
            filename: input.yaml
            contents:
                $yaml:
                    answer: 42", vec![TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(
                    ProcessExprTemplate {
                        command: vec![
                            Expr::Literal(Value::from("cat".to_string())),
                            Expr::TmpFile(
                                "input.yaml".to_string(),
                                Box::new(Expr::Yaml(Value::from(mapping(vec![("answer", Value::from(42))])))),
                            ),
                        ],
                        ..Default::default()
                    }
                ),
                ..Default::default()
        }])]
        #[case("with multiple processes", "
tests:
    - processes:
        process1:
            command:
                - echo
                - hello
        process2:
            command:
                - echo
                - world
    ", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Multi(indexmap! {
                "process1" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("hello"),
                    ],
                    ..Default::default()
                },
                "process2" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("world"),
                    ],
                    ..Default::default()
                },
            }),
            ..Default::default()
        }])]
        #[case("with multiple background processes", "
tests:
    - processes:
        process1:
            command:
                - echo
                - hello
            background: {}
        process2:
            command:
                - echo
                - world
    ", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Multi(indexmap! {
                "process1" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("hello"),
                    ],
                    mode: ProcessMode::Background(BackgroundConfig::default()),
                    ..Default::default()
                },
                "process2" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("world"),
                    ],
                    ..Default::default()
                },
            }),
            ..Default::default()
        }])]
        #[case("with background processes waiting by sleep", "
tests:
    - processes:
        main:
            command:
                - echo
                - hello
            background:
                wait_for:
                    type: sleep
                    duration: 100ms
    ", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Multi(indexmap! {
                "main" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("hello"),
                    ],
                    mode: ProcessMode::Background(BackgroundConfig {
                        wait_condition: WaitCondition::Sleep(SleepCondition { duration: Duration::from_millis(100) }),
                    }),
                    ..Default::default()
                },
            }),
            ..Default::default()
        }])]
        #[case("with background processes waiting by http", "
tests:
    - processes:
        main:
            command:
                - echo
                - hello
            background:
                wait_for:
                    type: http
                    port: 8080
                    path: /health
                    max_retry: 10
                    initial_delay: 1s
                    interval: 100ms
                    timeout: 3s
    ", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Multi(indexmap! {
                "main" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("hello"),
                    ],
                    mode: ProcessMode::Background(BackgroundConfig {
                        wait_condition: WaitCondition::Http(HttpCondition {
                            port: 8080, path: "/health".to_string(), initial_delay: Duration::from_secs(1),
                            interval: Duration::from_millis(100), max_retry: 10, timeout: Duration::from_secs(3)
                        }),
                    }),
                    ..Default::default()
                },
            }),
            ..Default::default()
        }])]
        #[case("with background processes waiting by http (default parameters)", "
tests:
    - processes:
        main:
            command:
                - echo
                - hello
            background:
                wait_for:
                    type: http
                    port: 8080
                    path: /health
    ", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Multi(indexmap! {
                "main" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("hello"),
                    ],
                    mode: ProcessMode::Background(BackgroundConfig {
                        wait_condition: WaitCondition::Http(HttpCondition {
                            port: 8080, path: "/health".to_string(), initial_delay: Duration::from_secs(0),
                            interval: Duration::from_secs(0), max_retry: 3, timeout: Duration::from_secs(1)
                        }),
                    }),
                    ..Default::default()
                },
            }),
            ..Default::default()
        }])]
        #[case("with multiple processes and expectations", "
tests:
    - processes:
        process1:
            command:
                - echo
                - hello
        process2:
            command:
                - echo
                - world
      expect:
        processes:
            process1:
                status:
                    success: true
            process2:
                stdout:
                    be_empty: true
    ", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Multi(indexmap! {
                "process1" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("hello"),
                    ],
                    ..Default::default()
                },
                "process2" => ProcessExprTemplate {
                    command: vec![
                        literal_expr("echo"),
                        literal_expr("world"),
                    ],
                    ..Default::default()
                },
            }),
            matchers: ProcessesMatchersExprTemplate::Multi(indexmap! {
                "process1" => ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap!{ "success" => literal_expr(true) },
                    ..Default::default()
                },
                "process2" => ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap!{ "be_empty" => literal_expr(true) },
                    ..Default::default()
                },
            }),
            ..Default::default()
        }])]
        #[case("with status matcher", "
tests:
    - command:
        - echo
        - hello
      expect:
        status:
          success: true", vec![TestCaseExprTemplate {
            matchers: ProcessesMatchersExprTemplate::Single(
                ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap!{ "success" => literal_expr(true) },
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with stdout matcher", "
tests:
    - command:
        - echo
        - hello
      expect:
        stdout:
          be_empty: true", vec![TestCaseExprTemplate {
            matchers: ProcessesMatchersExprTemplate::Single(
                ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap!{ "be_empty" => literal_expr(true) },
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with stderr matcher", "
tests:
    - command:
        - echo
        - hello
      expect:
        stderr:
          be_empty: true", vec![TestCaseExprTemplate {
            matchers: ProcessesMatchersExprTemplate::Single(
                ProcessMatchersExprTemplate {
                    stderr_matcher_exprs: indexmap!{ "be_empty" => literal_expr(true) },
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        fn success_case(
            #[case] title: &str,
            #[case] input: &str,
            #[case] expected: Vec<TestCaseExprTemplate>,
        ) {
            let actual = parse(FILENAME, input.as_bytes());

            assert_eq!(
                Ok(TestCaseExprFile {
                    filename: FILENAME.to_string(),
                    test_case_exprs: expected.into_iter().map(|x| x.build()).collect()
                }),
                actual,
                "{}",
                title
            )
        }

        #[rstest]
        #[case("when root is not map", "tests", vec![("$", "should be map, but is string")])]
        #[case("when root dosen't have .tests", "{}", vec![("$", "should have .tests as seq")])]
        #[case("when root.tests is not seq", "tests: {}", vec![("$.tests", "should be seq, but is map")])]
        #[case("when test is not map", "tests: [42]", vec![("$.tests[0]", "should be map, but is uint")])]
        #[case("when test dosen't have .command", "tests: [{}]", vec![("$.tests[0]", "should have .command as seq")])]
        #[case("when test command is not seq", "tests: [{command: 42}]", vec![("$.tests[0].command", "should be seq, but is uint")])]
        #[case("when test command is empty", "tests: [{command: []}]", vec![("$.tests[0].command", "should not be empty")])]
        #[case("when multi processes is not map", "tests: [processes: true]", vec![("$.tests[0].processes", "should be map, but is bool")])]
        #[case("when multi processes is empty", "tests: [processes: {}]", vec![("$.tests[0].processes", "should not be empty")])]
        #[case("when backgound is not map", "tests: [processes: { main: { command: [echo], background: 42 } }]", vec![("$.tests[0].processes.main.background", "should be map, but is uint")])]
        #[case("when wait condition type is not string", "tests: [processes: { main: { command: [echo], background: { wait_for: { type: 42 } } } }]", vec![("$.tests[0].processes.main.background.wait_for.type", "should be string, but is uint")])]
        #[case("when wait condition type is not defined", "tests: [processes: { main: { command: [echo], background: { wait_for: { type: unknown } } } }]", vec![("$.tests[0].processes.main.background.wait_for.type", "\"unknown\" is not valid wait condition type")])]
        #[case("when sleep wait condition dose not have duration", "tests: [processes: { main: { command: [echo], background: { wait_for: { type: sleep, duration: true } } } }]", vec![("$.tests[0].processes.main.background.wait_for.duration", "should be duration, but is bool")])]
        #[case("when http wait condition dose not have required parameters", "tests: [processes: { main: { command: [echo], background: { wait_for: { type: http } } } }]", vec![("$.tests[0].processes.main.background.wait_for", "should have .port as uint"), ("$.tests[0].processes.main.background.wait_for", "should have .path as string")])]
        #[case("when http wait condition dose not have invalid parameter", "tests: [processes: { main: { command: [echo], background: { wait_for: { type: http, port: 100000, path: true, initial_delay: true, interval: true, max_retry: -1, timeout: true } } } }]", vec![("$.tests[0].processes.main.background.wait_for.port", "should be in range of u16"), ("$.tests[0].processes.main.background.wait_for.path", "should be string, but is bool"), ("$.tests[0].processes.main.background.wait_for.initial_delay", "should be duration, but is bool"), ("$.tests[0].processes.main.background.wait_for.interval", "should be duration, but is bool"), ("$.tests[0].processes.main.background.wait_for.max_retry", "should be uint, but is int"), ("$.tests[0].processes.main.background.wait_for.timeout", "should be duration, but is bool")])]
        #[case("when some process is not map", "tests: [processes: {proc1: true}]", vec![("$.tests[0].processes.proc1", "should be map, but is bool")])]
        #[case("when some process's command is empty", "tests: [processes: {proc1: {command: []}}]", vec![("$.tests[0].processes.proc1.command", "should not be empty")])]
        #[case("when backgroud is not map", "tests: [processes: {proc1: {command: [true], background: true}}]", vec![("$.tests[0].processes.proc1.background", "should be map, but is bool")])]
        #[case("when test expect is not map", "tests: [{command: [echo], expect: 42}]", vec![("$.tests[0].expect", "should be map, but is uint")])]
        #[case("when test multi expect is not map", "tests: [{command: [echo], expect: {processes: 42}}]", vec![("$.tests[0].expect.processes", "should be map, but is uint")])]
        #[case("when multiple process givenm but expect is single", "tests: [{processes: {process1: {command: [echo]}}, expect: {stdin: {eq: 0}}}]", vec![("$.tests[0].expect", "expect should be multiple mode when multiple processes are given")])]
        #[case("when test env is not map", "tests: [{command: [echo], env: 42}]", vec![("$.tests[0].env", "should be map, but is uint")])]
        #[case("when test env contains not string key", "tests: [{command: [echo], env: {true: hello}}]", vec![("$.tests[0].env", "should be string keyed map, but contains Bool(true)")])]
        #[case("when test env contains empty name", "tests: [{command: [echo], env: {'': hello}}]", vec![("$.tests[0].env", "should have valid env var name (^[a-zA-Z_][a-zA-Z0-9_]*$)")])]
        #[case("when test env contains empty name", "tests: [{command: [echo], env: {'1MESSAGE': hello}}]", vec![("$.tests[0].env", "should have valid env var name (^[a-zA-Z_][a-zA-Z0-9_]*$)")])]
        #[case("when test status matcher is not map", "tests: [{command: [echo], expect: {status: 42}}]", vec![("$.tests[0].expect.status", "should be map, but is uint")])]
        #[case("when test status matcher contains not string key", "tests: [{command: [echo], expect: {status: {true: 42}}}]", vec![("$.tests[0].expect.status", "should be string keyed map, but contains Bool(true)")])]
        #[case("when test stdout matcher is not map", "tests: [{command: [echo], expect: {stdout: 42}}]", vec![("$.tests[0].expect.stdout", "should be map, but is uint")])]
        #[case("when test stdout matcher contains not string key", "tests: [{command: [echo], expect: {stdout: {true: 42}}}]", vec![("$.tests[0].expect.stdout", "should be string keyed map, but contains Bool(true)")])]
        #[case("when test stderr matcher is not map", "tests: [{command: [echo], expect: {stderr: 42}}]", vec![("$.tests[0].expect.stderr", "should be map, but is uint")])]
        #[case("when test stderr matcher contains not string key", "tests: [{command: [echo], expect: {stderr: {true: 42}}}]", vec![("$.tests[0].expect.stderr", "should be string keyed map, but contains Bool(true)")])]
        #[case("when $tmp_file is not map", "tests: [{command: [cat, {$tmp_file: 42}]}]", vec![("$.tests[0].command[1].$tmp_file", "should be map, but is uint")])]
        #[case("when $tmp_file dosen't have filename", "tests: [{command: [cat, {$tmp_file: {contents: hello}}]}]", vec![("$.tests[0].command[1].$tmp_file", "should have .filename as string")])]
        #[case("when $tmp_file has filename as not string", "tests: [{command: [cat, {$tmp_file: {filename: 42, contents: hello}}]}]", vec![("$.tests[0].command[1].$tmp_file.filename", "should be string, but is uint")])]
        #[case("when $tmp_file dosen't have contents", "tests: [{command: [cat, {$tmp_file: {filename: input.txt}}]}]", vec![("$.tests[0].command[1].$tmp_file", "should have .contents")])]
        fn error_case(
            #[case] title: &str,
            #[case] input: &str,
            #[case] violations: Vec<(&str, &str)>,
        ) {
            let actual = parse(FILENAME, input.as_bytes());
            assert_eq!(
                parse_error(
                    violations
                        .iter()
                        .map(|(path, message)| violation(path, message))
                        .collect()
                ),
                actual,
                "{}",
                title
            )
        }
    }
}
