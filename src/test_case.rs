use std::{ffi::OsString, fmt::Debug, time::Duration};

use indexmap::{indexmap, IndexMap};

use crate::{
    exec::{execute_command, Status},
    matcher::Matcher,
};

#[derive(Debug)]
pub struct TestCase {
    pub name: String,
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
    pub failures: IndexMap<String, Vec<String>>,
}

impl AssertionResult {
    pub fn is_passed(&self) -> bool {
        self.failures
            .iter()
            .all(|(_, messages)| messages.is_empty())
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct TestResult {
    pub name: String,
    pub result: Result<AssertionResult, String>,
}

impl TestResult {
    pub fn is_passed(&self) -> bool {
        match &self.result {
            Ok(assertion_result) => assertion_result.is_passed(),
            _ => false,
        }
    }
}

pub struct TestResultSummary {
    pub results: Vec<TestResult>,
}

type ClassifiedResults<'a> = (
    Vec<(&'a String, &'a AssertionResult)>,
    Vec<(&'a String, &'a AssertionResult)>,
    Vec<(&'a String, &'a String)>,
);

impl TestResultSummary {
    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn classified_results(&self) -> ClassifiedResults {
        let mut passed = vec![];
        let mut failed = vec![];
        let mut errors = vec![];

        for tr in &self.results {
            match &tr.result {
                Ok(assertion_result) => {
                    if assertion_result.is_passed() {
                        passed.push((&tr.name, assertion_result));
                    } else {
                        failed.push((&tr.name, assertion_result));
                    }
                }
                Err(message) => {
                    errors.push((&tr.name, message));
                }
            }
        }

        (passed, failed, errors)
    }

    pub fn is_all_passed(&self) -> bool {
        self.results.iter().all(|result| result.is_passed())
    }
}

impl PartialEq for TestCase {
    fn eq(&self, other: &Self) -> bool {
        if self.name != other.name
            || self.filename != other.filename
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
            return TestResult {
                name: self.name.clone(),
                result: Err(err),
            };
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

        TestResult {
            name: self.name.clone(),
            result: Ok(AssertionResult {
                failures: indexmap! {
                    "status".to_string() => status,
                    "stdout".to_string() => stdout,
                    "stderr".to_string() => stderr,
                },
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::indexmap;
    use once_cell::sync::Lazy;

    static STATUS_STRING: Lazy<String> = Lazy::new(|| "status".to_string());
    static STDOUT_STRING: Lazy<String> = Lazy::new(|| "stdout".to_string());
    static STDERR_STRING: Lazy<String> = Lazy::new(|| "stderr".to_string());

    mod test_case {
        use super::*;
        use crate::matcher::testutil::*;

        mod run {
            use super::*;
            use rstest::rstest;
            use serde_yaml::Value;

            const DEFAULT_NAME: &str = "test";
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
                    name: DEFAULT_NAME.to_string(),
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
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} } ) })]
            #[case("command is exit, status matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_success(Value::from(true))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} } ) })]
            #[case("command is exit, status matchers are failed",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![TestMatcher::failure_message(0)], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })})]
            #[case("command is exit, stdout matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_success(Value::from(true))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} }) })]
            #[case("command is exit, stdout matchers are failed",
            given_test_case(vec!["echo", "-n", "hello"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello")], STDERR_STRING.clone() => vec![]} }) })]
            #[case("command is exit, stdout matchers are failed, stdin is given",
            given_test_case(vec!["cat"], "hello world", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello world")], STDERR_STRING.clone() => vec![]} }) })]
            #[case("command is exit, stdout matchers are failed, env is given",
            given_test_case(vec!["printenv", "MESSAGE"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello\n")], STDERR_STRING.clone() => vec![]} }) })]
            #[case("command is exit, stderr matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT,  vec![], vec![], vec![TestMatcher::new_success(Value::from(true))]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} }) })]
            #[case("command is exit, stderr matchers are failed",
            given_test_case(vec!["bash", "-c", "echo -n hi >&2"], "", DEFAULT_TIMEOUT, vec![], vec![], vec![TestMatcher::new_failure(Value::from(1))]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![TestMatcher::failure_message("hi")]} }) })]
            #[case("command is signaled",
            given_test_case(vec!["bash", "-c", "kill -TERM $$"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec!["signaled with 15".to_string()], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} }) })]
            #[case("command is timed out",
            given_test_case(vec!["sleep", "1"], "", 0, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), result: Ok(AssertionResult{ failures: indexmap!{STATUS_STRING.clone() => vec!["timed out".to_string()], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} }) })]
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

                assert!(matches!(
                    actual,
                    TestResult {
                        name: _,
                        result: Err(_)
                    }
                ));
            }
        }
    }

    mod test_result_summary {
        use super::*;
        use indexmap::indexmap;
        use once_cell::sync::Lazy;
        use rstest::rstest;

        #[rstest]
        #[case(vec![], 0)]
        #[case(vec![TestResult{name: "test".to_string(), result: Ok(AssertionResult { failures:
            indexmap!{
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            }
        })}], 1)]
        #[case(vec![TestResult{ name: "test".to_string(), result: Err("".to_string()) }], 1)]
        #[case(vec![
            TestResult{ name: "test".to_string(), result: Ok(AssertionResult {
                failures: indexmap!{
                    STATUS_STRING.clone() => vec![],
                    STDOUT_STRING.clone() => vec![],
                    STDERR_STRING.clone() => vec![],
                }
            })},
            TestResult{ name: "test2".to_string(), result: Err("".to_string()) },
        ], 2)]
        fn len(#[case] results: Vec<TestResult>, #[case] expected: usize) {
            let summary = TestResultSummary { results };

            assert_eq!(summary.len(), expected);
        }

        static PASSED1_NAME: Lazy<String> = Lazy::new(|| "passed1".to_string());
        static PASSED2_NAME: Lazy<String> = Lazy::new(|| "passed2".to_string());
        static FAILURE1_NAME: Lazy<String> = Lazy::new(|| "failure1".to_string());
        static FAILURE2_NAME: Lazy<String> = Lazy::new(|| "failure2".to_string());
        static ERROR1_NAME: Lazy<String> = Lazy::new(|| "error1".to_string());
        static ERROR2_NAME: Lazy<String> = Lazy::new(|| "error2".to_string());

        static PASSED1: Lazy<AssertionResult> = Lazy::new(|| AssertionResult {
            failures: indexmap! {
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            },
        });
        static PASSED2: Lazy<AssertionResult> = Lazy::new(|| AssertionResult {
            failures: indexmap! {
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            },
        });
        static FAILURE1: Lazy<AssertionResult> = Lazy::new(|| AssertionResult {
            failures: indexmap! {
                STATUS_STRING.clone() => vec!["status failure".to_string()],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            },
        });
        static FAILURE2: Lazy<AssertionResult> = Lazy::new(|| AssertionResult {
            failures: indexmap! {
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec!["stdout failure".to_string()],
                STDERR_STRING.clone() => vec![],
            },
        });
        static ERROR1: Lazy<String> = Lazy::new(|| "error1".to_string());
        static ERROR2: Lazy<String> = Lazy::new(|| "error2".to_string());

        #[rstest]
        #[case(
            vec![],
            (vec![], vec![], vec![]),
        )]
        #[case(
            vec![TestResult{ name: "passed1".to_string(), result: Ok(PASSED1.clone())}, TestResult{ name: "failure1".to_string(), result: Ok(FAILURE1.clone()) }, TestResult{ name: "error1".to_string(), result: Err(ERROR1.clone()) }, TestResult{ name: "passed2".to_string(), result: Ok(PASSED2.clone()) }, TestResult{ name: "failure2".to_string(), result: Ok(FAILURE2.clone()) }, TestResult{ name: "error2".to_string(), result: Err(ERROR2.clone()) }],
            (vec![(&*PASSED1_NAME, &*PASSED1), (&*PASSED2_NAME, &*PASSED2)], vec![(&*FAILURE1_NAME, &*FAILURE1), (&*FAILURE2_NAME, &*FAILURE2)], vec![(&*ERROR1_NAME, &*ERROR1), (&*ERROR2_NAME, &*ERROR2)]),
        )]
        fn classified_results(
            #[case] results: Vec<TestResult>,
            #[case] expected: ClassifiedResults,
        ) {
            let summary = TestResultSummary { results };
            let actual = summary.classified_results();

            assert_eq!(actual, expected);
        }

        #[rstest]
        #[case(vec![], true)]
        #[case(vec![TestResult{name: "test1".to_string(), result: Ok(PASSED1.clone())}, TestResult{ name: "test2".to_string(), result: Ok(PASSED2.clone())}], true)]
        #[case(vec![TestResult{ name: "test1".to_string(), result: Ok(PASSED1.clone()) }, TestResult{ name: "test2".to_string(), result: Ok(PASSED2.clone()) }, TestResult{ name: "test3".to_string(), result: Ok(FAILURE1.clone()) }], false)]
        #[case(vec![TestResult{ name: "test1".to_string(), result: Ok(PASSED1.clone()) }, TestResult{ name: "test2".to_string(), result: Ok(PASSED2.clone()) }, TestResult{ name: "test3".to_string(), result: Err(ERROR1.clone()) }], false)]
        fn is_all_passed(#[case] results: Vec<TestResult>, #[case] expected: bool) {
            let summary = TestResultSummary { results };

            assert_eq!(summary.is_all_passed(), expected);
        }
    }
}
