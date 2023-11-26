use std::{ffi::OsString, fmt::Debug, time::Duration};

use crate::{
    exec::{execute_command, Status},
    matcher::Matcher,
};

#[derive(Debug)]
pub struct TestCase {
    pub filename: String,
    pub path: String,
    pub command: Vec<String>,
    pub stdin: String,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
    pub status_matchers: Vec<Box<dyn Matcher<i32>>>,
    pub stdout_matchers: Vec<Box<dyn Matcher<OsString>>>,
    pub stderr_matchers: Vec<Box<dyn Matcher<OsString>>>,
}

pub struct TestCaseFile<'a> {
    pub filename: String,
    pub test_cases: Vec<&'a TestCase>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct AssertionResult {
    status: Vec<String>,
    stdout: Vec<String>,
    stderr: Vec<String>,
}

impl AssertionResult {
    pub fn is_passed(&self) -> bool {
        self.status.is_empty() && self.stdout.is_empty() && self.stderr.is_empty()
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum TestResult {
    Asserted(AssertionResult),
    ExecError(String),
}

impl TestResult {
    pub fn is_passed(&self) -> bool {
        match self {
            TestResult::Asserted(assertion_result) => assertion_result.is_passed(),
            _ => false,
        }
    }
}

impl PartialEq for TestCase {
    fn eq(&self, other: &Self) -> bool {
        if self.filename != other.filename
            || self.path != other.path
            || self.command != other.command
            || self.stdin != other.stdin
            || self.timeout != other.timeout
            || self.tee_stdout != other.tee_stdout
            || self.tee_stderr != other.tee_stderr
        {
            return false;
        }

        if self.status_matchers.len() != other.status_matchers.len() {
            return false;
        }

        self.status_matchers
            .iter()
            .zip(other.status_matchers.iter())
            .all(|(self_status_matcher, other_status_matcher)| {
                self_status_matcher.eq(other_status_matcher.as_any())
            })
    }
}

impl TestCase {
    pub fn run(&self) -> TestResult {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let exec_result = rt
            .block_on(execute_command(
                self.command.clone(),
                self.stdin.clone(),
                self.env.clone(),
                self.timeout,
            ))
            .map(|output| {
                if self.tee_stdout {
                    println!("{}", output.stdout.to_string_lossy());
                }
                if self.tee_stderr {
                    println!("{}", output.stderr.to_string_lossy());
                }
                output
            });

        if let Err(err) = exec_result {
            return TestResult::ExecError(err);
        }

        let output = exec_result.unwrap();

        let status = match output.status {
            Status::Exit(code) => self
                .status_matchers
                .iter()
                .filter_map(|matcher| {
                    matcher
                        .matches(code)
                        .map(
                            |(passed, message)| {
                                if passed {
                                    None
                                } else {
                                    Some(message)
                                }
                            },
                        )
                        .unwrap_or_else(Some)
                })
                .collect::<Vec<_>>(),
            Status::Signal(signal) => vec![format!("signaled with {}", signal)],
            Status::Timeout => vec![format!("timed out")],
        };

        let stdout = self
            .stdout_matchers
            .iter()
            .filter_map(|matcher| {
                matcher
                    .matches(output.stdout.clone())
                    .map(
                        |(passed, message)| {
                            if passed {
                                None
                            } else {
                                Some(message)
                            }
                        },
                    )
                    .unwrap_or_else(Some)
            })
            .collect::<Vec<_>>();

        let stderr = self
            .stderr_matchers
            .iter()
            .filter_map(|matcher| {
                matcher
                    .matches(output.stderr.clone())
                    .map(
                        |(passed, message)| {
                            if passed {
                                None
                            } else {
                                Some(message)
                            }
                        },
                    )
                    .unwrap_or_else(Some)
            })
            .collect::<Vec<_>>();

        TestResult::Asserted(AssertionResult {
            status,
            stdout,
            stderr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::testutil::*;

    mod run {
        use super::*;
        use rstest::rstest;
        use serde_yaml::Value;

        const DEFAULT_FILENAME: &str = "test.yaml";
        const DEFAULT_PATH: &str = "$.tests[0]";
        const DEFAULT_TIMEOUT: u64 = 10;

        pub fn given_test_case(
            command: Vec<&str>,
            stdin: &str,
            timeout: u64,
            status_matchers: Vec<Box<dyn Matcher<i32>>>,
            stdout_matchers: Vec<Box<dyn Matcher<OsString>>>,
            stderr_matchers: Vec<Box<dyn Matcher<OsString>>>,
        ) -> TestCase {
            TestCase {
                filename: DEFAULT_FILENAME.to_string(),
                path: DEFAULT_PATH.to_string(),
                command: command.iter().map(|x| x.to_string()).collect(),
                stdin: stdin.to_string(),
                env: vec![("MESSAGE".to_string(), "hello".to_string())],
                timeout: Duration::from_secs(timeout),
                tee_stdout: false,
                tee_stderr: false,
                status_matchers,
                stdout_matchers,
                stderr_matchers,
            }
        }

        #[rstest]
        #[case("command is exit, no matchers",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![], vec![], vec![] ),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![], stderr: vec![] } ))]
        #[case("command is exit, status matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_success(Value::from(true))], vec![], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![], stderr: vec![] }))]
        #[case("command is exit, status matchers are failed",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec![TestMatcher::failure_message(0)], stdout: vec![], stderr: vec![] }))]
        #[case("command is exit, stdout matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_success(Value::from(true))], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![], stderr: vec![] }))]
        #[case("command is exit, stdout matchers are failed",
            given_test_case(vec!["echo", "-n", "hello"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![TestMatcher::failure_message("hello")], stderr: vec![] }))]
        #[case("command is exit, stdout matchers are failed, stdin is given",
            given_test_case(vec!["cat"], "hello world", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![TestMatcher::failure_message("hello world")], stderr: vec![] }))]
        #[case("command is exit, stdout matchers are failed, env is given",
            given_test_case(vec!["printenv", "MESSAGE"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![TestMatcher::failure_message("hello\n")], stderr: vec![] }))]
        #[case("command is exit, stderr matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT,  vec![], vec![], vec![TestMatcher::new_success(Value::from(true))]),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![], stderr: vec![] }))]
        #[case("command is exit, stderr matchers are failed",
            given_test_case(vec!["bash", "-c", "echo -n hi >&2"], "", DEFAULT_TIMEOUT, vec![], vec![], vec![TestMatcher::new_failure(Value::from(1))]),
            TestResult::Asserted(AssertionResult{ status: vec![], stdout: vec![], stderr: vec![TestMatcher::failure_message("hi")] }))]
        #[case("command is signaled",
            given_test_case(vec!["bash", "-c", "kill -TERM $$"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec!["signaled with 15".to_string()], stdout: vec![], stderr: vec![] }))]
        #[case("command is timed out",
            given_test_case(vec!["sleep", "1"], "", 0, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult::Asserted(AssertionResult{ status: vec!["timed out".to_string()], stdout: vec![], stderr: vec![] }))]
        fn when_exec_succeeded(
            #[case] title: &str,
            #[case] given: TestCase,
            #[case] expeceted: TestResult,
        ) {
            let actual = given.run();
            assert_eq!(actual, expeceted, "{}", title);
        }

        #[test]
        fn when_exec_failed() {
            let given = given_test_case(
                vec!["_unknown"],
                "",
                DEFAULT_TIMEOUT,
                vec![],
                vec![],
                vec![],
            );

            let actual = given.run();

            assert!(matches!(actual, TestResult::ExecError(_)));
        }
    }
}
