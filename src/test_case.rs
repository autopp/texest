use std::{fmt::Debug, os::unix::ffi::OsStrExt, time::Duration};

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
    pub stdout_matchers: Vec<Box<dyn Matcher<Vec<u8>>>>,
    pub stderr_matchers: Vec<Box<dyn Matcher<Vec<u8>>>>,
}

pub struct TestCaseFile<'a> {
    pub filename: String,
    pub test_cases: Vec<&'a TestCase>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct TestResult {
    pub name: String,
    pub failures: IndexMap<String, Vec<String>>,
}

impl TestResult {
    pub fn is_passed(&self) -> bool {
        self.failures
            .iter()
            .all(|(_, messages)| messages.is_empty())
    }
}

pub struct TestResultSummary {
    pub results: Vec<TestResult>,
}

impl TestResultSummary {
    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn classified_results(&self) -> (Vec<&TestResult>, Vec<&TestResult>) {
        let mut passed = vec![];
        let mut failed = vec![];

        for tr in &self.results {
            if tr.is_passed() {
                passed.push(tr);
            } else {
                failed.push(tr);
            }
        }

        (passed, failed)
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
                self_status_matcher == other_status_matcher
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
                failures: indexmap! { "exec".to_string() => vec![err] },
            };
        }

        let output = exec_result.unwrap();

        let status = match output.status {
            Status::Exit(code) => self
                .status_matchers
                .iter()
                .filter_map(|matcher| {
                    matcher
                        .matches(&code)
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
            Status::Timeout => vec![format!("timed out ({} sec)", self.timeout.as_secs())],
        };

        let stdout = output.stdout.as_bytes().to_vec();
        let stdout_messages = self
            .stdout_matchers
            .iter()
            .filter_map(|matcher| {
                matcher
                    .matches(&stdout)
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

        let stderr = output.stderr.as_bytes().to_vec();
        let stderr_messages = self
            .stderr_matchers
            .iter()
            .filter_map(|matcher| {
                matcher
                    .matches(&stderr)
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
            failures: indexmap! {
                "status".to_string() => status,
                "stdout".to_string() => stdout_messages,
                "stderr".to_string() => stderr_messages,
            },
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
            use pretty_assertions::assert_eq;
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
                stdout_matchers: Vec<Box<dyn Matcher<Vec<u8>>>>,
                stderr_matchers: Vec<Box<dyn Matcher<Vec<u8>>>>,
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
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, status matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_success(Value::from(true))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, status matchers are failed",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![TestMatcher::failure_message(0)], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, stdout matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_success(Value::from(true))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, stdout matchers are failed",
            given_test_case(vec!["echo", "-n", "hello"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello".as_bytes())], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, stdout matchers are failed, stdin is given",
            given_test_case(vec!["cat"], "hello world", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello world".as_bytes())], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, stdout matchers are failed, env is given",
            given_test_case(vec!["printenv", "MESSAGE"], "", DEFAULT_TIMEOUT, vec![], vec![TestMatcher::new_failure(Value::from(1))], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello\n".as_bytes())], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, stderr matchers are succeeded",
            given_test_case(vec!["true"], "", DEFAULT_TIMEOUT,  vec![], vec![], vec![TestMatcher::new_success(Value::from(true))]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })]
            #[case("command is exit, stderr matchers are failed",
            given_test_case(vec!["bash", "-c", "echo -n hi >&2"], "", DEFAULT_TIMEOUT, vec![], vec![], vec![TestMatcher::new_failure(Value::from(1))]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![TestMatcher::failure_message("hi".as_bytes())]} })]
            #[case("command is signaled",
            given_test_case(vec!["bash", "-c", "kill -TERM $$"], "", DEFAULT_TIMEOUT, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec!["signaled with 15".to_string()], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })]
            #[case("command is timed out",
            given_test_case(vec!["sleep", "1"], "", 0, vec![TestMatcher::new_failure(Value::from(1))], vec![], vec![]),
            TestResult{ name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec!["timed out (0 sec)".to_string()], STDOUT_STRING.clone() => vec![], STDERR_STRING.clone() => vec![]} })]
            fn when_exec_succeeded(
                #[case] title: &str,
                #[case] given: TestCase,
                #[case] expected: TestResult,
            ) {
                let actual = given.run();
                assert_eq!(expected, actual, "{}", title);
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

                assert_eq!(DEFAULT_NAME.clone(), actual.name);
                assert_eq!(1, actual.failures.len());
                assert_eq!(1, actual.failures.get("exec").unwrap().len());
            }
        }
    }

    mod test_result_summary {
        use super::*;
        use indexmap::indexmap;
        use once_cell::sync::Lazy;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[rstest]
        #[case(vec![], 0)]
        #[case(vec![TestResult{name: "test".to_string(), failures:
            indexmap!{
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            }
        }], 1)]
        #[case(vec![
            TestResult{ name: "test".to_string(),
                failures: indexmap!{
                    STATUS_STRING.clone() => vec![],
                    STDOUT_STRING.clone() => vec![],
                    STDERR_STRING.clone() => vec![],
                }
            },
            TestResult{ name: "test2".to_string(), failures: indexmap!{} },
        ], 2)]
        fn len(#[case] results: Vec<TestResult>, #[case] expected: usize) {
            let summary = TestResultSummary { results };

            assert_eq!(expected, summary.len());
        }

        static PASSED1: Lazy<TestResult> = Lazy::new(|| TestResult {
            name: "passed1".to_string(),
            failures: indexmap! {
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            },
        });
        static PASSED2: Lazy<TestResult> = Lazy::new(|| TestResult {
            name: "passed2".to_string(),
            failures: indexmap! {
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            },
        });
        static FAILURE1: Lazy<TestResult> = Lazy::new(|| TestResult {
            name: "failure1".to_string(),
            failures: indexmap! {
                STATUS_STRING.clone() => vec!["status failure".to_string()],
                STDOUT_STRING.clone() => vec![],
                STDERR_STRING.clone() => vec![],
            },
        });
        static FAILURE2: Lazy<TestResult> = Lazy::new(|| TestResult {
            name: "failure2".to_string(),
            failures: indexmap! {
                STATUS_STRING.clone() => vec![],
                STDOUT_STRING.clone() => vec!["stdout failure".to_string()],
                STDERR_STRING.clone() => vec![],
            },
        });

        #[rstest]
        #[case(
            vec![],
            (vec![], vec![]),
        )]
        #[case(
            vec![PASSED1.clone(), FAILURE1.clone(), PASSED2.clone(), FAILURE2.clone()],
            (vec![&*PASSED1, &*PASSED2], vec![&*FAILURE1, &*FAILURE2]),
        )]
        fn classified_results(
            #[case] results: Vec<TestResult>,
            #[case] expected: (Vec<&TestResult>, Vec<&TestResult>),
        ) {
            let summary = TestResultSummary { results };
            let actual = summary.classified_results();

            assert_eq!(expected, actual);
        }

        #[rstest]
        #[case(vec![], true)]
        #[case(vec![PASSED1.clone(), PASSED2.clone()], true)]
        #[case(vec![PASSED1.clone(), PASSED2.clone(), FAILURE1.clone()], false)]
        fn is_all_passed(#[case] results: Vec<TestResult>, #[case] expected: bool) {
            let summary = TestResultSummary { results };

            assert_eq!(expected, summary.is_all_passed());
        }
    }
}
