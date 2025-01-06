use std::time::Duration;

use indexmap::{indexmap, IndexMap};
use once_cell::sync::Lazy;
use regex::Regex;

use saphyr::Yaml;

use crate::{
    ast::Map,
    expr::Expr,
    test_case_expr::{
        BackgroundConfigExpr, ProcessExpr, ProcessMatchersExpr, ProcessModeExpr, ProcessesExpr,
        ProcessesMatchersExpr, TestCaseExpr, TestCaseExprFile, WaitConditionExpr,
    },
    validator::{Validator, Violation},
};

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct Error {
    pub filename: String,
    pub message: String,
    pub violations: Vec<Violation>,
}

impl Error {
    pub fn without_violations(filename: &str, message: impl Into<String>) -> Self {
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

pub fn parse(filename: &str, mut reader: impl std::io::Read) -> Result<TestCaseExprFile, Error> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf).map_err(|err| {
        Error::without_violations(filename, format!("cannot read {}: {}", filename, err))
    })?;

    let ast = &Yaml::load_from_str(&buf).map_err(|err| {
        Error::without_violations(filename, format!("cannot parse {}: {}", filename, err))
    })?[0];

    let mut v = Validator::new(filename);

    let test_case_exprs = v
        .must_be_map(ast)
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

                        let (processes_matchers, files_matchers): (ProcessesMatchersExpr,  IndexMap<String, IndexMap<String, Expr>>) = v
                            .may_have_map(&test, "expect", |v, expect| {
                                let processes_matchers = v.may_have_map(expect, "processes", |v, processes| {
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
                                });

                                let files_matchers = v.may_have_map(expect, "files", |v, files| {
                                    files
                                        .iter()
                                        .filter_map(|(path, expectations)| {
                                            v.in_field(path, |v| {
                                                v.must_be_map(expectations).map(|expectations| {
                                                    (
                                                        path.to_string(),
                                                        parse_expected(v, &expectations),
                                                    )
                                                })
                                            })
                                        })
                                        .collect()
                                }).unwrap_or_default();

                                (processes_matchers, files_matchers)
                            })
                            .unwrap_or((ProcessesMatchersExpr::Multi(indexmap! {}), indexmap! {}));

                        if let (ProcessesExpr::Multi(_), ProcessesMatchersExpr::Single(_)) =
                            (&processes, &processes_matchers)
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
                            processes_matchers,
                            files_matchers,
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
    let command_and_args = v
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

    let (command, args) = command_and_args
        .split_first()
        .map(|(command, args)| (command.clone(), args.to_vec()))
        .unwrap_or_else(|| (Expr::Literal(Yaml::String("true".to_string())), vec![]));

    let stdin = v
        .may_have(m, "stdin", parse_expr)
        .unwrap_or(Expr::Literal(Yaml::String("".to_string())));
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
            let wait_condition = v.may_have_map(background, "wait_for", |v, wait_for| {
                let name = v.must_have_string(wait_for, "type").unwrap_or_default();
                let params = wait_for
                    .iter()
                    .filter_map(|(k, value)| {
                        if *k == "type" {
                            None
                        } else {
                            Some((k.to_string(), parse_expr(v, value)))
                        }
                    })
                    .collect();
                WaitConditionExpr { name, params }
            });
            ProcessModeExpr::Background(BackgroundConfigExpr { wait_condition })
        })
        .unwrap_or(ProcessModeExpr::Foreground);
    let tee_stdout = v.may_have_bool(m, "tee_stdout").unwrap_or(false);
    let tee_stderr = v.may_have_bool(m, "tee_stderr").unwrap_or(false);

    ProcessExpr {
        command,
        args,
        stdin,
        env,
        timeout,
        mode,
        tee_stdout,
        tee_stderr,
    }
}

static ENV_VAR_EXPR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?ms)\A([a-zA-Z_][a-zA-Z0-9_]*)(?:-(.*))?\z").unwrap());
static VAR_EXPR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap());
fn parse_expr(v: &mut Validator, x: &Yaml) -> Expr {
    if v.may_be_string(x).is_some() {
        return Expr::Literal(x.clone());
    }

    v.may_be_qualified(x)
        .and_then(|(q, value)| match q {
            "env" => v.in_field("$env", |v| {
                v.must_be_string(value).and_then(|s| {
                    ENV_VAR_EXPR_RE
                        .captures(&s)
                        .map(|caps| {
                            let name = caps.get(1).unwrap().as_str().to_string();
                            let default = caps.get(2).map(|m| m.as_str().to_string());
                            Expr::EnvVar(name, default)
                        })
                        .or_else(|| {
                            v.add_violation(format!(
                                "should be valid env var name (got \"{}\")",
                                s
                            ));
                            None
                        })
                })
            }),
            "yaml" => Some(Expr::Yaml(value.clone())),
            "json" => Some(Expr::Json(value.clone())),
            "tmp_file" => v.in_field("$tmp_file", |v| {
                v.must_be_map(value).map(|m| {
                    let filename = v.must_have_string(&m, "filename").unwrap_or_default();
                    let contents = v
                        .must_have(&m, "contents", parse_expr)
                        .unwrap_or_else(|| Expr::Literal(Yaml::Boolean(false)));
                    Expr::TmpFile(filename, Box::new(contents))
                })
            }),
            "var" => v.in_field("$var", |v| {
                v.must_be_string(value).and_then(|s| {
                    if VAR_EXPR_RE.is_match(&s) {
                        Some(Expr::Var(s))
                    } else {
                        v.add_violation(format!("should be valid var name (got \"{}\")", s));
                        None
                    }
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
            test_case_expr::{
                testutil::{
                    ProcessExprTemplate, ProcessMatchersExprTemplate, ProcessesExprTemplate,
                    ProcessesMatchersExprTemplate, TestCaseExprTemplate,
                },
                BackgroundConfigExpr, ProcessModeExpr, WaitConditionExpr,
            },
        };

        use super::*;
        use indexmap::indexmap;
        use pretty_assertions::assert_eq;
        use rstest::rstest;
        use saphyr::Yaml;

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
        - hello", vec![TestCaseExprTemplate{name: Some(literal_expr(Yaml::String("mytest".to_string()))), ..TestCaseExprTemplate::default()}])]
        #[case("with command contains env var", "
tests:
    - command:
        - echo
        - $env: MESSAGE
        - $env: |-
            NAME-John
            Doe
        - $env: SUFFIX-", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Single(
                ProcessExprTemplate {
                    command: Expr::Literal(Yaml::String("echo".to_string().to_string())),
                    args: vec![
                        Expr::EnvVar("MESSAGE".to_string(), None),
                        Expr::EnvVar("NAME".to_string(), Some("John\nDoe".to_string())),
                        Expr::EnvVar("SUFFIX".to_string(), Some("".to_string())),
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
      tee_stdout: true
      tee_stderr: true", vec![TestCaseExprTemplate {
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
                    command: Expr::Literal(Yaml::String("cat".to_string())),
                    args: vec![],
                    stdin: literal_expr(Yaml::String("hello".to_string())),
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
                    command: Expr::Literal(Yaml::String("cat".to_string())),
                    args: vec![],
                    stdin: Expr::Yaml(Yaml::Hash(mapping(vec![("message", Yaml::String("hello".to_string()))]))),
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
                    command: Expr::Literal(Yaml::String("cat".to_string())),
                    args: vec![],
                    stdin: Expr::Json(Yaml::Hash(mapping(vec![("message", Yaml::String("hello".to_string()))]))),
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
                    env: vec![("MESSAGE1", literal_expr(Yaml::String("hello".to_string()))), ("MESSAGE2", env_var_expr("FOO"))],
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
                        command: Expr::Literal(Yaml::String("cat".to_string())),
                        args: vec![
                            Expr::TmpFile(
                                "input.yaml".to_string(),
                                Box::new(Expr::Yaml(Yaml::Hash(mapping(vec![("answer", Yaml::Integer(42))])))),
                            ),
                        ],
                        ..Default::default()
                    }
                ),
                ..Default::default()
        }])]
        #[case(
            "with command contains var",
            "
tests:
    - command:
        - echo
        - $var: message", vec![TestCaseExprTemplate {
                processes: ProcessesExprTemplate::Single(
                    ProcessExprTemplate {
                        command: Expr::Literal(Yaml::String("echo".to_string())),
                        args: vec![Expr::Var("message".to_string())],
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
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("hello".to_string())),
                    ],
                    ..Default::default()
                },
                "process2" => ProcessExprTemplate {
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("world".to_string())),
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
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("hello".to_string())),
                    ],
                    mode: ProcessModeExpr::Background(BackgroundConfigExpr { wait_condition: None }),
                    ..Default::default()
                },
                "process2" => ProcessExprTemplate {
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("world".to_string())),
                    ],
                    ..Default::default()
                },
            }),
            ..Default::default()
        }])]
        #[case("with multiple background processes & wait_for", "
tests:
    - processes:
        process1:
            command:
                - echo
                - hello
            background:
                wait_for:
                    type: success_stub
                    answer: 42
        process2:
            command:
                - echo
                - world
    ", vec![TestCaseExprTemplate {
            processes: ProcessesExprTemplate::Multi(indexmap! {
                "process1" => ProcessExprTemplate {
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("hello".to_string())),
                    ],
                    mode: ProcessModeExpr::Background(
                        BackgroundConfigExpr {
                            wait_condition: Some(WaitConditionExpr {
                                name: "success_stub".to_string(),
                                params: indexmap! { "answer".to_string() => literal_expr(Yaml::Integer(42)) }
                            })
                        }
                    ),
                    ..Default::default()
                },
                "process2" => ProcessExprTemplate {
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("world".to_string())),
                    ],
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
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("hello".to_string())),
                    ],
                    ..Default::default()
                },
                "process2" => ProcessExprTemplate {
                    command: literal_expr(Yaml::String("echo".to_string())),
                    args: vec![
                        literal_expr(Yaml::String("world".to_string())),
                    ],
                    ..Default::default()
                },
            }),
            processes_matchers: ProcessesMatchersExprTemplate::Multi(indexmap! {
                "process1" => ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap!{ "success" => literal_expr(Yaml::Boolean(true)) },
                    ..Default::default()
                },
                "process2" => ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap!{ "be_empty" => literal_expr(Yaml::Boolean(true)) },
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
            processes_matchers: ProcessesMatchersExprTemplate::Single(
                ProcessMatchersExprTemplate {
                    status_matcher_exprs: indexmap!{ "success" => literal_expr(Yaml::Boolean(true)) },
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
            processes_matchers: ProcessesMatchersExprTemplate::Single(
                ProcessMatchersExprTemplate {
                    stdout_matcher_exprs: indexmap!{ "be_empty" => literal_expr(Yaml::Boolean(true)) },
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
            processes_matchers: ProcessesMatchersExprTemplate::Single(
                ProcessMatchersExprTemplate {
                    stderr_matcher_exprs: indexmap!{ "be_empty" => literal_expr(Yaml::Boolean(true)) },
                    ..Default::default()
                }
            ),
            ..Default::default()
        }])]
        #[case("with files matcher", "
tests:
    - command:
        - echo
        - hello
      expect:
        files:
          hello.txt:
            be_empty: true", vec![TestCaseExprTemplate {
            processes_matchers: ProcessesMatchersExprTemplate::Single(
                ProcessMatchersExprTemplate {
                    ..Default::default()
                }
            ),
            files_matchers: indexmap!{ "hello.txt" => indexmap!{ "be_empty" => literal_expr(Yaml::Boolean(true))} },
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
        #[case("when backgound is not map", "tests: [{ processes: { main: { command: [echo], background: 42 } } }]", vec![("$.tests[0].processes.main.background", "should be map, but is uint")])]
        #[case("when wait condition type is not string", "tests: [{ processes: { main: { command: [echo], background: { wait_for: { type: 42 } } } } }]", vec![("$.tests[0].processes.main.background.wait_for.type", "should be string, but is uint")])]
        #[case("when some process is not map", "tests: [{processes: {proc1: true}}]", vec![("$.tests[0].processes.proc1", "should be map, but is bool")])]
        #[case("when some process's command is empty", "tests: [{processes: {proc1: {command: []}}}]", vec![("$.tests[0].processes.proc1.command", "should not be empty")])]
        #[case("when backgroud is not map", "tests: [{processes: {proc1: {command: [true], background: true}}}]", vec![("$.tests[0].processes.proc1.background", "should be map, but is bool")])]
        #[case("when test expect is not map", "tests: [{command: [echo], expect: 42}]", vec![("$.tests[0].expect", "should be map, but is uint")])]
        #[case("when test multi expect is not map", "tests: [{command: [echo], expect: {processes: 42}}]", vec![("$.tests[0].expect.processes", "should be map, but is uint")])]
        #[case("when multiple process givenm but expect is single", "tests: [{processes: {process1: {command: [echo]}}, expect: {stdin: {eq: 0}}}]", vec![("$.tests[0].expect", "expect should be multiple mode when multiple processes are given")])]
        #[case("when test env is not map", "tests: [{command: [echo], env: 42}]", vec![("$.tests[0].env", "should be map, but is uint")])]
        #[case("when test env contains not string key", "tests: [{command: [echo], env: {true: hello}}]", vec![("$.tests[0].env", "should be string keyed map, but contains Boolean(true)")])]
        #[case("when test env contains empty name", "tests: [{command: [echo], env: {'': hello}}]", vec![("$.tests[0].env", "should have valid env var name (^[a-zA-Z_][a-zA-Z0-9_]*$)")])]
        #[case("when test env contains empty name", "tests: [{command: [echo], env: {'1MESSAGE': hello}}]", vec![("$.tests[0].env", "should have valid env var name (^[a-zA-Z_][a-zA-Z0-9_]*$)")])]
        #[case("when test status matcher is not map", "tests: [{command: [echo], expect: {status: 42}}]", vec![("$.tests[0].expect.status", "should be map, but is uint")])]
        #[case("when test status matcher contains not string key", "tests: [{command: [echo], expect: {status: {true: 42}}}]", vec![("$.tests[0].expect.status", "should be string keyed map, but contains Boolean(true)")])]
        #[case("when test stdout matcher is not map", "tests: [{command: [echo], expect: {stdout: 42}}]", vec![("$.tests[0].expect.stdout", "should be map, but is uint")])]
        #[case("when test stdout matcher contains not string key", "tests: [{command: [echo], expect: {stdout: {true: 42}}}]", vec![("$.tests[0].expect.stdout", "should be string keyed map, but contains Boolean(true)")])]
        #[case("when test stderr matcher is not map", "tests: [{command: [echo], expect: {stderr: 42}}]", vec![("$.tests[0].expect.stderr", "should be map, but is uint")])]
        #[case("when test stderr matcher contains not string key", "tests: [{command: [echo], expect: {stderr: {true: 42}}}]", vec![("$.tests[0].expect.stderr", "should be string keyed map, but contains Boolean(true)")])]
        #[case("when test files matcher is not map", "tests: [{command: [echo], expect: {files: 42}}]", vec![("$.tests[0].expect.files", "should be map, but is uint")])]
        #[case("when test file matcher is not map", "tests: [{command: [echo], expect: {files: {hello: 42}}}]", vec![("$.tests[0].expect.files.hello", "should be map, but is uint")])]
        #[case("when test file matcher contains not string key", "tests: [{command: [echo], expect: {files: {hello: {true: 42}}}}]", vec![("$.tests[0].expect.files.hello", "should be string keyed map, but contains Boolean(true)")])]
        #[case("when $env is not string", "tests: [{command: [cat, {$env: 42}]}]", vec![("$.tests[0].command[1].$env", "should be string, but is uint")])]
        #[case("when $env is not valid env var name", "tests: [{command: [cat, {$env: \"MESS AGE\"}]}]", vec![("$.tests[0].command[1].$env", "should be valid env var name (got \"MESS AGE\")")])]
        #[case("when $tmp_file is not map", "tests: [{command: [cat, {$tmp_file: 42}]}]", vec![("$.tests[0].command[1].$tmp_file", "should be map, but is uint")])]
        #[case("when $tmp_file dosen't have filename", "tests: [{command: [cat, {$tmp_file: {contents: hello}}]}]", vec![("$.tests[0].command[1].$tmp_file", "should have .filename as string")])]
        #[case("when $tmp_file has filename as not string", "tests: [{command: [cat, {$tmp_file: {filename: 42, contents: hello}}]}]", vec![("$.tests[0].command[1].$tmp_file.filename", "should be string, but is uint")])]
        #[case("when $tmp_file dosen't have contents", "tests: [{command: [cat, {$tmp_file: {filename: input.txt}}]}]", vec![("$.tests[0].command[1].$tmp_file", "should have .contents")])]
        #[case("when $env is not valid var name", "tests: [{command: [cat, {$var: \"MESS AGE\"}]}]", vec![("$.tests[0].command[1].$var", "should be valid var name (got \"MESS AGE\")")])]
        #[case("when $env is not string", "tests: [{command: [cat, {$var: 42}]}]", vec![("$.tests[0].command[1].$var", "should be string, but is uint")])]
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
