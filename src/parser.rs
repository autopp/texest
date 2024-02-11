use std::time::Duration;

use indexmap::{indexmap, IndexMap};
use once_cell::sync::Lazy;
use regex::Regex;

use serde_yaml::Value;

use crate::{
    ast::Map,
    expr::Expr,
    test_case_expr::{ProcessExpr, ProcessesExpr, TestCaseExpr, TestCaseExprFile},
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
                        let (status_matchers, stdout_matchers, stderr_matchers) = v
                            .may_have_map(&test, "expect", |v, expect| {
                                let status_matchers = v
                                    .may_have_map(expect, "status", parse_expected)
                                    .unwrap_or(IndexMap::new());
                                let stdout_matchers = v
                                    .may_have_map(expect, "stdout", parse_expected)
                                    .unwrap_or(IndexMap::new());
                                let stderr_matchers = v
                                    .may_have_map(expect, "stderr", parse_expected)
                                    .unwrap_or(IndexMap::new());
                                (status_matchers, stdout_matchers, stderr_matchers)
                            })
                            .unwrap_or((IndexMap::new(), IndexMap::new(), IndexMap::new()));

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

                        TestCaseExpr {
                            name,
                            filename: v.filename.clone(),
                            path: v.current_path(),
                            processes,
                            status_matchers,
                            stdout_matchers,
                            stderr_matchers,
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
        .unwrap_or(vec![]);
    let timeout = v.may_have_uint(m, "timeout").unwrap_or(DEFAULT_TIMEOUT);
    let tee_stdout = v.may_have_bool(m, "teeStdout").unwrap_or(false);
    let tee_stderr = v.may_have_bool(m, "teeStderr").unwrap_or(false);

    ProcessExpr {
        command,
        stdin,
        env,
        timeout: Duration::from_secs(timeout),
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
            test_case_expr::testutil::{
                ProcessExprTemplate, ProcessesExprTemplate, TestCaseExprTemplate,
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
      timeout: 5", vec![TestCaseExprTemplate {
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
        #[case("with status matcher", "
tests:
    - command:
        - echo
        - hello
      expect:
        status:
          success: true", vec![TestCaseExprTemplate {
            status_matchers: indexmap!{ "success" => literal_expr(true) },
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
            stdout_matchers: indexmap!{ "be_empty" => literal_expr(true) },
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
            stderr_matchers: indexmap!{ "be_empty" => literal_expr(true) },
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
        #[case("when some process is not map", "tests: [processes: {proc1: true}]", vec![("$.tests[0].processes.proc1", "should be map, but is bool")])]
        #[case("when some process's command is empty", "tests: [processes: {proc1: {command: []}}]", vec![("$.tests[0].processes.proc1.command", "should not be empty")])]
        #[case("when test expect is not map", "tests: [{command: [echo], expect: 42}]", vec![("$.tests[0].expect", "should be map, but is uint")])]
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
