use std::{fmt::Debug, ops::ControlFlow, os::unix::ffi::OsStrExt, time::Duration};

use indexmap::{indexmap, IndexMap};

use crate::{
    exec::{execute_command, Status},
    matcher::Matcher,
    tmp_dir::TmpDir,
};

pub trait LifeCycleHook: Debug {
    fn serialize(&self) -> (&str, serde_yaml::Value);
}

pub trait SetupHook: LifeCycleHook {
    fn setup(&self) -> Result<(), String>;
}

impl PartialEq for dyn SetupHook {
    fn eq(&self, other: &Self) -> bool {
        self.serialize() == other.serialize()
    }
}

pub trait TeardownHook: LifeCycleHook {
    fn teardown(&self) -> Result<(), String>;
}

impl PartialEq for dyn TeardownHook {
    fn eq(&self, other: &Self) -> bool {
        self.serialize() == other.serialize()
    }
}

#[derive(Debug)]
pub struct TestCase<T: TmpDir> {
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
    pub setup_hooks: Vec<Box<dyn SetupHook>>,
    pub teardown_hooks: Vec<Box<dyn TeardownHook>>,
    pub tmp_dir: Option<T>,
}

pub struct TestCaseFile<'a, T: TmpDir> {
    pub filename: String,
    pub test_cases: Vec<&'a TestCase<T>>,
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

impl<T: TmpDir> PartialEq for TestCase<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.name != other.name
            || self.filename != other.filename
            || self.path != other.path
            || self.command != other.command
            || self.stdin != other.stdin
            || self.timeout != other.timeout
            || self.tee_stdout != other.tee_stdout
            || self.tee_stderr != other.tee_stderr
            || self.status_matchers != other.status_matchers
            || self.stdout_matchers != other.stdout_matchers
            || self.stderr_matchers != other.stderr_matchers
            || self.setup_hooks != other.setup_hooks
            || self.teardown_hooks != other.teardown_hooks
        {
            return false;
        }

        match self.tmp_dir {
            Some(ref tmp_dir) => other
                .tmp_dir
                .as_ref()
                .map(|other_tmp_dir| tmp_dir.path() == other_tmp_dir.path())
                .unwrap_or(false),
            None => other.tmp_dir.is_none(),
        }
    }
}

impl<T: TmpDir> TestCase<T> {
    pub fn run(&self) -> TestResult {
        let rt = tokio::runtime::Runtime::new().unwrap();

        let mut setup_failures = vec![];
        self.setup_hooks.iter().try_for_each(|hook| {
            let r = hook.setup();
            if let Err(err) = r {
                setup_failures.push(err);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });

        if !setup_failures.is_empty() {
            return TestResult {
                name: self.name.clone(),
                failures: indexmap! { "setup".to_string() => setup_failures },
            };
        }

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

        let mut teardown_failures = vec![];
        self.teardown_hooks.iter().rev().for_each(|hook| {
            if let Err(err) = hook.teardown() {
                teardown_failures.push(err);
            }
        });

        TestResult {
            name: self.name.clone(),
            failures: indexmap! {
                "status".to_string() => status,
                "stdout".to_string() => stdout_messages,
                "stderr".to_string() => stderr_messages,
                "teardown".to_string() => teardown_failures
            },
        }
    }
}

#[cfg(test)]
pub mod testutil {
    use serde_yaml::Value;

    use crate::{matcher::Matcher, tmp_dir::TmpDir};
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use super::{LifeCycleHook, SetupHook, TeardownHook, TestCase};

    pub const DEFAULT_NAME: &str = "test";
    pub const DEFAULT_FILENAME: &str = "test.yaml";
    pub const DEFAULT_PATH: &str = "$.tests[0]";
    pub const DEFAULT_TIMEOUT: u64 = 10;

    #[derive(Debug, PartialEq)]
    pub enum HookType {
        Setup,
        Teardown,
    }

    pub type HookHistory = Vec<(HookType, &'static str)>;

    #[derive(Debug)]
    pub struct TestHook {
        pub name: &'static str,
        pub err: Option<&'static str>,
        pub history: Rc<RefCell<HookHistory>>,
    }

    impl TestHook {
        pub fn new(
            name: &'static str,
            err: Option<&'static str>,
            history: Rc<RefCell<HookHistory>>,
        ) -> Self {
            TestHook { name, err, history }
        }

        fn to_result(&self) -> Result<(), String> {
            self.err.map(|err| Err(err.into())).unwrap_or(Ok(()))
        }
    }

    impl LifeCycleHook for TestHook {
        fn serialize(&self) -> (&str, Value) {
            ("test", Value::from(self.name))
        }
    }

    impl SetupHook for TestHook {
        fn setup(&self) -> Result<(), String> {
            self.history.borrow_mut().push((HookType::Setup, self.name));
            self.to_result()
        }
    }

    impl TeardownHook for TestHook {
        fn teardown(&self) -> Result<(), String> {
            self.history
                .borrow_mut()
                .push((HookType::Teardown, self.name));
            self.to_result()
        }
    }

    pub struct TestCaseTemplate<T: TmpDir> {
        pub name: &'static str,
        pub filename: &'static str,
        pub path: &'static str,
        pub command: Vec<&'static str>,
        pub stdin: &'static str,
        pub env: Vec<(&'static str, &'static str)>,
        pub timeout: u64,
        pub tee_stdout: bool,
        pub tee_stderr: bool,
        pub status_matchers: Vec<Box<dyn Matcher<i32>>>,
        pub stdout_matchers: Vec<Box<dyn Matcher<Vec<u8>>>>,
        pub stderr_matchers: Vec<Box<dyn Matcher<Vec<u8>>>>,
        pub setup_hooks: Vec<Box<dyn SetupHook>>,
        pub teardown_hooks: Vec<Box<dyn TeardownHook>>,
        pub tmp_dir: Option<T>,
    }

    impl<T: TmpDir> TestCaseTemplate<T> {
        pub fn build(self) -> TestCase<T> {
            TestCase {
                name: self.name.to_string(),
                filename: self.filename.to_string(),
                path: self.path.to_string(),
                command: self.command.iter().map(|x| x.to_string()).collect(),
                stdin: self.stdin.to_string(),
                env: self
                    .env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                timeout: Duration::from_secs(self.timeout),
                tee_stdout: self.tee_stdout,
                tee_stderr: self.tee_stderr,
                status_matchers: self.status_matchers,
                stdout_matchers: self.stdout_matchers,
                stderr_matchers: self.stderr_matchers,
                setup_hooks: self.setup_hooks,
                teardown_hooks: self.teardown_hooks,
                tmp_dir: self.tmp_dir,
            }
        }
    }

    impl<T: TmpDir> Default for TestCaseTemplate<T> {
        fn default() -> Self {
            TestCaseTemplate {
                name: DEFAULT_NAME,
                filename: DEFAULT_FILENAME,
                path: DEFAULT_PATH,
                command: vec!["echo", "hello"],
                stdin: "",
                env: vec![],
                timeout: DEFAULT_TIMEOUT,
                tee_stdout: false,
                tee_stderr: false,
                status_matchers: vec![],
                stdout_matchers: vec![],
                stderr_matchers: vec![],
                setup_hooks: vec![],
                teardown_hooks: vec![],
                tmp_dir: None,
            }
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
    static SETUP_STRING: Lazy<String> = Lazy::new(|| "setup".to_string());
    static TEARDOWN_STRING: Lazy<String> = Lazy::new(|| "teardown".to_string());

    mod test_case {
        use super::*;

        mod run {
            use std::{cell::RefCell, rc::Rc};

            use crate::matcher::testutil::TestMatcher;
            use crate::test_case::testutil::HookType::{Setup, Teardown};
            use crate::test_case::testutil::{
                HookHistory, TestCaseTemplate, TestHook, DEFAULT_NAME,
            };
            use crate::tmp_dir::testutil::StubTmpDir;

            use super::*;
            use pretty_assertions::assert_eq;
            use rstest::rstest;
            use serde_yaml::Value;

            fn complete_failures(test_result: &TestResult) -> TestResult {
                TestResult {
                    name: test_result.name.clone(),
                    failures: indexmap! {
                        STATUS_STRING.clone() => test_result.failures.get(&*STATUS_STRING).map(Clone::clone).unwrap_or(vec![]),
                        STDOUT_STRING.clone() => test_result.failures.get(&*STDOUT_STRING).map(Clone::clone).unwrap_or(vec![]),
                        STDERR_STRING.clone() => test_result.failures.get(&*STDERR_STRING).map(Clone::clone).unwrap_or(vec![]),
                        SETUP_STRING.clone() => test_result.failures.get(&*SETUP_STRING).map(Clone::clone).unwrap_or(vec![]),
                        TEARDOWN_STRING.clone() => test_result.failures.get(&*TEARDOWN_STRING).map(Clone::clone).unwrap_or(vec![]),
                    },
                }
            }

            #[rstest]
            #[case("command is exit, no matchers",
                TestCaseTemplate { command: vec!["true"], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, status matchers are succeeded",
                TestCaseTemplate{ command: vec!["true"], status_matchers: vec![TestMatcher::new_success(Value::from(true))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, status matchers are failed",
                TestCaseTemplate { command: vec!["true"], status_matchers: vec![TestMatcher::new_failure(Value::from(1))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec![TestMatcher::failure_message(0)]} })]
            #[case("command is exit, stdout matchers are succeeded",
                TestCaseTemplate { command: vec!["true"], stdout_matchers: vec![TestMatcher::new_success(Value::from(true))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, stdout matchers are failed",
                TestCaseTemplate { command: vec!["echo", "-n", "hello"], stdout_matchers: vec![TestMatcher::new_failure(Value::from(1))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello".as_bytes())]} })]
            #[case("command is exit, stdout matchers are failed, stdin is given",
                TestCaseTemplate { command: vec!["cat"], stdin: "hello world", stdout_matchers: vec![TestMatcher::new_failure(Value::from(1))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello world".as_bytes())]} })]
            #[case("command is exit, stdout matchers are failed, env is given",
                TestCaseTemplate { command: vec!["printenv", "MESSAGE"], env: vec![("MESSAGE", "hello")], stdout_matchers: vec![TestMatcher::new_failure(Value::from(1))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{STDOUT_STRING.clone() => vec![TestMatcher::failure_message("hello\n".as_bytes())]} })]
            #[case("command is exit, stderr matchers are succeeded",
                TestCaseTemplate { command: vec!["true"], stderr_matchers: vec![TestMatcher::new_success(Value::from(true))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, stderr matchers are failed",
                TestCaseTemplate { command: vec!["bash", "-c", "echo -n hi >&2"], stderr_matchers: vec![TestMatcher::new_failure(Value::from(1))], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{STDERR_STRING.clone() => vec![TestMatcher::failure_message("hi".as_bytes())]} })]
            #[case("command is signaled",
                TestCaseTemplate { command: vec!["bash", "-c", "kill -TERM $$"], ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec!["signaled with 15".to_string()]} })]
            #[case("command is timed out",
                TestCaseTemplate { command: vec!["sleep", "1"], timeout: 0, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{STATUS_STRING.clone() => vec!["timed out (0 sec)".to_string()]} })]
            fn when_exec_succeeded(
                #[case] title: &str,
                #[case] given: TestCaseTemplate<StubTmpDir>,
                #[case] expected: TestResult,
            ) {
                let actual = given.build().run();
                assert_eq!(
                    complete_failures(&expected),
                    complete_failures(&actual),
                    "{}",
                    title
                );
            }

            #[rstest]
            #[case("all hooks and assertions are succeeded",
                TestMatcher::new_success(Value::from(true)),
                vec![("setup1", None), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", None)],
                indexmap!{},
                vec![(Setup, "setup1"), (Setup, "setup2"), (Teardown, "teardown2"), (Teardown, "teardown1")])]
            #[case("when assertion is failed, remaining hooks are executed",
                TestMatcher::new_failure(Value::from(true)),
                vec![("setup1", None), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", None)],
                indexmap!{ STATUS_STRING.clone() => vec![TestMatcher::failure_message(42)] },
                vec![(Setup, "setup1"), (Setup, "setup2"), (Teardown, "teardown2"), (Teardown, "teardown1")])]
            #[case("when first setup hook is failed, target command and remaning hooks are not executed",
                TestMatcher::new_failure(Value::from(true)),
                vec![("setup1", Some("setup1 failed")), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", None)],
                indexmap!{ SETUP_STRING.clone() => vec!["setup1 failed".to_string()] },
                vec![(Setup, "setup1")])]
            #[case("when first teardown hook is failed, remaining hooks are executed",
                TestMatcher::new_failure(Value::from(true)),
                vec![("setup1", None), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", Some("teardown2 failed"))],
                indexmap!{ STATUS_STRING.clone() => vec![TestMatcher::failure_message(42)], TEARDOWN_STRING.clone() => vec!["teardown2 failed".to_string()] },
                vec![(Setup, "setup1"), (Setup, "setup2"), (Teardown, "teardown2"), (Teardown, "teardown1")])]
            fn when_hooks_given(
                #[case] title: &str,
                #[case] status_matcher: Box<dyn Matcher<i32>>,
                #[case] setup_hooks: Vec<(&'static str, Option<&'static str>)>,
                #[case] teardown_hooks: Vec<(&'static str, Option<&'static str>)>,
                #[case] expected_failures: IndexMap<String, Vec<String>>,
                #[case] expected_history: HookHistory,
            ) {
                let history = Rc::new(RefCell::new(vec![]));

                let given = TestCaseTemplate::<StubTmpDir> {
                    command: vec!["bash", "-c", "exit 42"],
                    status_matchers: vec![status_matcher],
                    setup_hooks: setup_hooks
                        .iter()
                        .map(|(name, err)| -> Box<dyn SetupHook> {
                            Box::new(TestHook::new(name, *err, Rc::clone(&history)))
                        })
                        .collect(),
                    teardown_hooks: teardown_hooks
                        .iter()
                        .map(|(name, err)| -> Box<dyn TeardownHook> {
                            Box::new(TestHook::new(name, *err, Rc::clone(&history)))
                        })
                        .collect(),
                    ..Default::default()
                }
                .build();

                let result = given.run();
                assert_eq!(
                    complete_failures(&TestResult {
                        name: DEFAULT_NAME.into(),
                        failures: expected_failures
                            .iter()
                            .map(|(subject, messages)| (subject.to_string(), messages.clone()))
                            .collect()
                    }),
                    complete_failures(&result),
                    "{}: result",
                    title
                );

                assert_eq!(
                    expected_history,
                    *history.borrow(),
                    "{}: hook history",
                    title
                );
            }

            #[test]
            fn when_exec_failed() {
                let given = TestCaseTemplate::<StubTmpDir> {
                    command: vec!["_unknown"],
                    ..Default::default()
                }
                .build();

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
