use std::time::Duration;

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::Regex;

use serde_yaml::Value;

use crate::{
    ast::Map,
    expr::Expr,
    test_case_expr::{TestCaseExpr, TestCaseExprFile},
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
                    v.must_be_map(test).and_then(|test| {
                        let name = v.may_have(&test, "name", parse_expr);
                        let stdin = v.may_have(&test, "stdin", parse_expr).unwrap_or(Expr::Literal(Value::from("")));
                        let timeout = v.may_have_uint(&test, "timeout").unwrap_or(DEFAULT_TIMEOUT);
                        let tee_stdout = v.may_have_bool(&test, "teeStdout").unwrap_or(false);
                        let tee_stderr = v.may_have_bool(&test, "teeStderr").unwrap_or(false);
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
                        let env: Vec<(String, Expr)> = v.may_have_map(&test, "env", |v, env| {
                            env.into_iter()
                                .filter_map(|(name, value)| {
                                    if !VAR_NAME_RE.is_match(name) {
                                        v.add_violation("should have valid env var name (^[a-zA-Z_][a-zA-Z0-9_]*$)");
                                        return None
                                    }
                                    Some((name.to_string(), parse_expr(v, value)))
                                })
                                .collect::<Vec<_>>()
                        }).unwrap_or(vec![]);

                        v.must_have_seq(&test, "command", |v, command| {
                            if command.is_empty() {
                                v.add_violation("should not be empty");
                                None
                            } else {
                                v.map_seq(command, |v, x| Some(parse_expr(v, x)))
                            }
                        })
                        .flatten()
                        .map(|command| TestCaseExpr {
                            name,
                            filename: v.filename.clone(),
                            path: v.current_path(),
                            command,
                            stdin,
                            env,
                            timeout: Duration::from_secs(timeout),
                            tee_stdout,
                            tee_stderr,
                            status_matchers,
                            stdout_matchers,
                            stderr_matchers,
                        })
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
            test_case_expr::testutil::TestCaseExprTemplate,
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
            command: vec![
                Expr::Literal(Value::from("echo".to_string())),
                Expr::EnvVar("MESSAGE".to_string(), None),
            ],
            ..Default::default()
        }])]
        #[case("with command contains timeout", "
tests:
    - command:
        - echo
        - hello
      timeout: 5", vec![TestCaseExprTemplate {
            timeout: 5,
            ..Default::default()
        }])]
        #[case("with command cotains tee_stdout & tee_stderr", "
tests:
    - command:
        - echo
        - hello
      teeStdout: true
      teeStderr: true", vec![TestCaseExprTemplate {
            tee_stdout: true,
            tee_stderr: true,
            ..Default::default()
        }])]
        #[case("with command contains simple stdin", "
tests:
    - command:
        - cat
      stdin: hello", vec![TestCaseExprTemplate {
            command: vec![Expr::Literal(Value::from("cat".to_string()))],
            stdin: literal_expr("hello"),
            ..Default::default()
        }])]
        #[case("with command contains yaml stdin", "
tests:
    - command:
        - cat
      stdin:
        $yaml:
          message: hello", vec![TestCaseExprTemplate {
            command: vec![Expr::Literal(Value::from("cat".to_string()))],
            stdin: Expr::Yaml(Value::from(mapping(vec![("message", Value::from("hello"))]))),
            ..Default::default()
        }])]
        #[case("with command contains json stdin", "
tests:
    - command:
        - cat
      stdin:
        $json:
          message: hello", vec![TestCaseExprTemplate {
            command: vec![Expr::Literal(Value::from("cat".to_string()))],
            stdin: Expr::Json(Value::from(mapping(vec![("message", Value::from("hello"))]))),
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
            env: vec![("MESSAGE1", literal_expr("hello")), ("MESSAGE2", env_var_expr("FOO"))],
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
