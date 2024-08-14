pub mod setup_hook;
pub mod teardown_hook;
pub mod wait_condition;

use std::{fmt::Debug, ops::ControlFlow, os::unix::ffi::OsStrExt, time::Duration};

use futures::future::join_all;
use indexmap::{indexmap, IndexMap};
use setup_hook::SetupHook;
use teardown_hook::TeardownHook;

use crate::{
    exec::{execute_background_command, execute_command, BackgroundExec, Output, Status},
    matcher::{StatusMatcher, StreamMatcher},
};

pub use self::wait_condition::WaitCondition;

#[derive(Debug, PartialEq, Clone, Default)]
pub struct BackgroundConfig {
    pub wait_condition: WaitCondition,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ProcessMode {
    Foreground,
    Background(BackgroundConfig),
}

#[derive(Debug, PartialEq)]
pub struct Process {
    pub command: Vec<String>,
    pub stdin: String,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub mode: ProcessMode,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
    pub status_matchers: Vec<StatusMatcher>,
    pub stdout_matchers: Vec<StreamMatcher>,
    pub stderr_matchers: Vec<StreamMatcher>,
}

#[derive(Debug, PartialEq)]
pub struct TestCase {
    pub name: String,
    pub filename: String,
    pub path: String,
    pub processes: IndexMap<String, Process>,
    pub files_matchers: IndexMap<String, Vec<StreamMatcher>>,
    pub setup_hooks: Vec<SetupHook>,
    pub teardown_hooks: Vec<TeardownHook>,
}

pub struct TestCaseFile<'a> {
    // TODO: remove allow (and use in report?)
    #[allow(dead_code)]
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

#[derive(Debug, PartialEq)]
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

impl TestCase {
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

        enum Execution {
            Foreground(Result<Output, String>),
            Background(Result<BackgroundExec, String>),
        }

        let exec_results = rt.block_on(async {
            let mut executions: Vec<Execution> = vec![];

            for (_, process) in self.processes.iter() {
                let execution = match &process.mode {
                    ProcessMode::Foreground => {
                        let exec_result = execute_command(
                            process.command.clone(),
                            process.stdin.clone(),
                            process.env.clone(),
                            process.timeout,
                        )
                        .await;

                        if let Ok(output) = &exec_result {
                            if process.tee_stdout {
                                println!("{}", output.stdout.to_string_lossy());
                            }
                            if process.tee_stderr {
                                println!("{}", output.stderr.to_string_lossy());
                            }
                        }

                        Execution::Foreground(exec_result)
                    }
                    ProcessMode::Background(cfg) => {
                        let background_exec = execute_background_command(
                            process.command.clone(),
                            process.stdin.clone(),
                            process.env.clone(),
                            process.timeout,
                            &cfg.wait_condition,
                        )
                        .await;

                        Execution::Background(background_exec)
                    }
                };

                executions.push(execution);
            }

            async fn collect_exec_result(execution: Execution) -> Result<Output, String> {
                match execution {
                    Execution::Foreground(result) => result,
                    Execution::Background(Ok(bg)) => bg.terminate().await,
                    Execution::Background(Err(err)) => Err(err),
                }
            }

            join_all(executions.into_iter().map(collect_exec_result)).await
        });

        let mut failures = indexmap! {};
        self.processes.iter().zip(exec_results).for_each(
            |((process_name, process), exec_result)| match exec_result {
                Ok(output) => {
                    let status_messages = match output.status {
                        Status::Exit(code) => run_status_matchers(&process.status_matchers, code),
                        Status::Signal(signal) => vec![format!("signaled with {}", signal)],
                        Status::Timeout => {
                            vec![format!("timed out ({} sec)", process.timeout.as_secs())]
                        }
                    };

                    let stdout = output.stdout.as_bytes().to_vec();
                    let stdout_messages = run_stream_matchers(&process.stdout_matchers, &stdout);

                    let stderr = output.stderr.as_bytes().to_vec();
                    let stderr_messages = run_stream_matchers(&process.stderr_matchers, &stderr);

                    if !status_messages.is_empty() {
                        failures.insert(subject_of(process_name, "status"), status_messages);
                    }
                    if !stdout_messages.is_empty() {
                        failures.insert(subject_of(process_name, "stdout"), stdout_messages);
                    }
                    if !stderr_messages.is_empty() {
                        failures.insert(subject_of(process_name, "stderr"), stderr_messages);
                    }
                }
                Err(err) => {
                    failures.insert(subject_of(process_name, "exec"), vec![err]);
                }
            },
        );

        self.files_matchers.iter().for_each(|(path, matchers)| {
            let subject = subject_of("file", path);

            match std::fs::metadata(path) {
                Ok(metadata) => {
                    if !metadata.is_file() {
                        failures.insert(subject, vec!["is not file".to_string()]);
                        return;
                    }

                    match std::fs::read(path) {
                        Ok(content) => {
                            let messages = run_stream_matchers(matchers, &content);
                            if !messages.is_empty() {
                                failures.insert(subject, messages);
                            }
                        }
                        Err(_) => {
                            failures.insert(subject, vec!["cannot read file".to_string()]);
                        }
                    };
                }
                Err(_) => {
                    failures.insert(subject, vec!["dose not exist".to_string()]);
                }
            };
        });

        let mut teardown_failures = vec![];
        self.teardown_hooks.iter().rev().for_each(|hook| {
            if let Err(err) = hook.teardown() {
                teardown_failures.push(err);
            }
        });

        if !teardown_failures.is_empty() {
            failures.insert("teardown".to_string(), teardown_failures);
        }

        TestResult {
            name: self.name.clone(),
            failures,
        }
    }
}

fn subject_of<S: AsRef<str>, T: AsRef<str>>(process_name: S, subject: T) -> String {
    format!("{}:{}", process_name.as_ref(), subject.as_ref())
}

fn run_status_matchers(matchers: &[StatusMatcher], status: i32) -> Vec<String> {
    matchers
        .iter()
        .filter_map(|matcher| {
            matcher
                .matches(status)
                .map(|(passed, message)| if passed { None } else { Some(message) })
                .unwrap_or_else(Some)
        })
        .collect()
}

fn run_stream_matchers(matchers: &[StreamMatcher], stream: &[u8]) -> Vec<String> {
    matchers
        .iter()
        .filter_map(|matcher| {
            matcher
                .matches(stream)
                .map(|(passed, message)| if passed { None } else { Some(message) })
                .unwrap_or_else(Some)
        })
        .collect()
}

#[cfg(test)]
pub mod testutil {
    use indexmap::{indexmap, IndexMap};

    use crate::matcher::{StatusMatcher, StreamMatcher};
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use super::{
        setup_hook::SetupHook, teardown_hook::TeardownHook, Process, ProcessMode, TestCase,
    };

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

    #[derive(Debug, PartialEq)]
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

        pub fn setup(&self) -> Result<(), String> {
            self.history.borrow_mut().push((HookType::Setup, self.name));
            self.to_result()
        }

        pub fn teardown(&self) -> Result<(), String> {
            self.history
                .borrow_mut()
                .push((HookType::Teardown, self.name));
            self.to_result()
        }
    }

    pub struct ProcessTemplate {
        pub command: Vec<&'static str>,
        pub stdin: &'static str,
        pub env: Vec<(&'static str, &'static str)>,
        pub timeout: u64,
        pub mode: ProcessMode,
        pub tee_stdout: bool,
        pub tee_stderr: bool,
        pub status_matchers: Vec<StatusMatcher>,
        pub stdout_matchers: Vec<StreamMatcher>,
        pub stderr_matchers: Vec<StreamMatcher>,
    }

    impl Default for ProcessTemplate {
        fn default() -> Self {
            ProcessTemplate {
                command: vec!["echo", "hello"],
                stdin: "",
                env: vec![],
                timeout: DEFAULT_TIMEOUT,
                tee_stdout: false,
                tee_stderr: false,
                mode: ProcessMode::Foreground,
                status_matchers: vec![],
                stdout_matchers: vec![],
                stderr_matchers: vec![],
            }
        }
    }

    impl ProcessTemplate {
        pub fn build(self) -> Process {
            Process {
                command: self.command.iter().map(|x| x.to_string()).collect(),
                stdin: self.stdin.to_string(),
                env: self
                    .env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                timeout: Duration::from_secs(self.timeout),
                mode: self.mode,
                tee_stdout: self.tee_stdout,
                tee_stderr: self.tee_stderr,
                status_matchers: self.status_matchers,
                stdout_matchers: self.stdout_matchers,
                stderr_matchers: self.stderr_matchers,
            }
        }
    }

    type FilesMatchers = IndexMap<&'static str, Vec<StreamMatcher>>;
    pub struct TestCaseTemplate {
        pub name: &'static str,
        pub filename: &'static str,
        pub path: &'static str,
        pub processes: IndexMap<&'static str, ProcessTemplate>,
        pub files_matchers: FilesMatchers,
        pub setup_hooks: Vec<SetupHook>,
        pub teardown_hooks: Vec<TeardownHook>,
    }

    impl TestCaseTemplate {
        pub fn build(self) -> TestCase {
            TestCase {
                name: self.name.to_string(),
                filename: self.filename.to_string(),
                path: self.path.to_string(),
                processes: self
                    .processes
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v.build()))
                    .collect(),
                files_matchers: self
                    .files_matchers
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                setup_hooks: self.setup_hooks,
                teardown_hooks: self.teardown_hooks,
            }
        }
    }

    impl Default for TestCaseTemplate {
        fn default() -> Self {
            TestCaseTemplate {
                name: DEFAULT_NAME,
                filename: DEFAULT_FILENAME,
                path: DEFAULT_PATH,
                processes: indexmap! { "main" => ProcessTemplate::default() },
                files_matchers: indexmap! {},
                setup_hooks: vec![],
                teardown_hooks: vec![],
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

            use crate::matcher::testutil::new_stream_test_success;
            use crate::matcher::testutil::{
                new_status_test_failure, new_status_test_success, new_stream_test_failure,
                TestMatcher,
            };
            use crate::test_case::testutil::HookType::{Setup, Teardown};
            use crate::test_case::testutil::{
                HookHistory, ProcessTemplate, TestCaseTemplate, DEFAULT_NAME,
            };
            use crate::test_case::wait_condition::SleepCondition;

            use self::tests::testutil::{DEFAULT_FILENAME, DEFAULT_PATH};

            use super::*;
            use pretty_assertions::assert_eq;
            use rstest::rstest;
            use saphyr::Yaml;
            use setup_hook::testutil::new_test_setup_hook;
            use teardown_hook::testutil::new_test_teardown_hook;

            #[rstest]
            #[case("command is exit, no matchers",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["true"], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, status matchers are succeeded",
                TestCaseTemplate{ processes: indexmap! { "main" => ProcessTemplate { command: vec!["true"], status_matchers: vec![new_status_test_success(Yaml::Boolean(true))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, status matchers are failed",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["true"], status_matchers: vec![new_status_test_failure(Yaml::Integer(1))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{format!("main:{}", *STATUS_STRING) => vec![TestMatcher::failure_message(0)]} })]
            #[case("command is exit, stdout matchers are succeeded",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["true"], stdout_matchers: vec![new_stream_test_success(Yaml::Boolean(true))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, stdout matchers are failed",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["echo", "-n", "hello"], stdout_matchers: vec![new_stream_test_failure(Yaml::Integer(1))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{format!("main:{}", *STDOUT_STRING) => vec![TestMatcher::failure_message("hello".as_bytes())]} })]
            #[case("command is exit, stdout matchers are failed, stdin is given",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["cat"], stdin: "hello world", stdout_matchers: vec![new_stream_test_failure(Yaml::Integer(1))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{format!("main:{}", *STDOUT_STRING) => vec![TestMatcher::failure_message("hello world".as_bytes())]} })]
            #[case("command is exit, stdout matchers are failed, env is given",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["printenv", "MESSAGE"], env: vec![("MESSAGE", "hello")], stdout_matchers: vec![new_stream_test_failure(Yaml::Integer(1))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{format!("main:{}", *STDOUT_STRING) => vec![TestMatcher::failure_message("hello\n".as_bytes())]} })]
            #[case("command is exit, stderr matchers are succeeded",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["true"], stderr_matchers: vec![new_stream_test_success(Yaml::Boolean(true))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{} })]
            #[case("command is exit, stderr matchers are failed",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["bash", "-c", "echo -n hi >&2"], stderr_matchers: vec![new_stream_test_failure(Yaml::Integer(1))], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{format!("main:{}", *STDERR_STRING) => vec![TestMatcher::failure_message("hi".as_bytes())]} })]
            #[case("command is signaled",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["bash", "-c", "kill -TERM $$"], ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{format!("main:{}", *STATUS_STRING) => vec!["signaled with 15".to_string()]} })]
            #[case("command is timed out",
                TestCaseTemplate { processes: indexmap! { "main" => ProcessTemplate { command: vec!["sleep", "1"], timeout: 0, ..Default::default() } }, ..Default::default() },
                TestResult { name: DEFAULT_NAME.to_string(), failures: indexmap!{format!("main:{}", *STATUS_STRING) => vec!["timed out (0 sec)".to_string()]} })]
            #[case("with background process",
                TestCaseTemplate {
                    processes: indexmap! {
                        "bg" => ProcessTemplate {
                            command: vec!["bash", "-c", r#"
                                trap 'echo goodbye >&2; exit 2' TERM
                                echo hello
                                while true; do true; done
                            "#
                            ],
                            mode: ProcessMode::Background(BackgroundConfig {
                                wait_condition: WaitCondition::Sleep(SleepCondition { duration: Duration::from_millis(50) })
                            }),
                            status_matchers: vec![new_status_test_failure(Yaml::Boolean(true))],
                            stdout_matchers: vec![new_stream_test_failure(Yaml::Boolean(true))],
                            stderr_matchers: vec![new_stream_test_failure(Yaml::Boolean(true))],
                            ..Default::default()
                        },
                        "main" => ProcessTemplate {
                            command: vec!["false"],
                            status_matchers: vec![new_status_test_failure(Yaml::Boolean(true))],
                            ..Default::default()
                        }
                    },
                    ..Default::default()
                },
                TestResult {
                    name: DEFAULT_NAME.to_string(),
                    failures: indexmap! {
                        format!("bg:{}", *STATUS_STRING) => vec![TestMatcher::failure_message(2)],
                        format!("bg:{}", *STDOUT_STRING) => vec![TestMatcher::failure_message("hello\n".as_bytes())],
                        format!("bg:{}", *STDERR_STRING) => vec![TestMatcher::failure_message("goodbye\n".as_bytes())],
                        format!("main:{}", *STATUS_STRING) => vec![TestMatcher::failure_message(1)]
                    }
                })]
            fn when_exec_succeeded(
                #[case] title: &str,
                #[case] given: TestCaseTemplate,
                #[case] expected: TestResult,
            ) {
                let actual = given.build().run();

                assert_eq!(expected, actual, "{}", title);
            }

            #[rstest]
            #[case("file exists, matcher is succeeded",
                "echo -n hello >{}",
                vec![new_stream_test_success(Yaml::Boolean(true))],
                None)]
            #[case("file exists, matcher is failed",
                "echo -n hello >{}",
                vec![new_stream_test_failure(Yaml::Boolean(true))],
                Some(vec![TestMatcher::failure_message("hello".as_bytes())]))]
            #[case("file dose not exist",
                "rm -f {}",
                vec![new_stream_test_failure(Yaml::Boolean(true))],
                Some(vec!["dose not exist".to_string()]))]
            #[case("given is not file",
                "mkdir -p {}",
                vec![new_stream_test_failure(Yaml::Boolean(true))],
                Some(vec!["is not file".to_string()]))]
            #[case("cannot read file",
                "echo -n hello >{}; chmod 0200 {}",
                vec![new_stream_test_failure(Yaml::Boolean(true))],
                Some(vec!["cannot read file".to_string()]))]
            fn when_exec_succeeded_with_files_matcher(
                #[case] title: &str,
                #[case] command: &str,
                #[case] matchers: Vec<StreamMatcher>,
                #[case] expected_messages: Option<Vec<String>>,
            ) {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("test.txt").to_str().unwrap().to_string();
                let command_with_path = command.replace("{}", &path);

                let given = TestCase {
                    name: DEFAULT_NAME.to_string(),
                    filename: DEFAULT_FILENAME.to_string(),
                    path: DEFAULT_PATH.to_string(),
                    processes: indexmap! {
                        "main".to_string() => Process {
                            command: vec!["bash".to_string(), "-c".to_string(), command_with_path],
                            env: vec![],
                            stdin: "".to_string(),
                            timeout: Duration::from_secs(10),
                            mode: ProcessMode::Foreground,
                            tee_stdout: false,
                            tee_stderr: false,
                            status_matchers: vec![],
                            stdout_matchers: vec![],
                            stderr_matchers: vec![],
                        }
                    },
                    files_matchers: indexmap! { path.clone() => matchers },
                    setup_hooks: vec![],
                    teardown_hooks: vec![],
                };

                let expected = TestResult {
                    name: DEFAULT_NAME.to_string(),
                    failures: expected_messages
                        .map(|messages| indexmap! { format!("file:{}", path) => messages.clone() })
                        .unwrap_or_default(),
                };

                assert_eq!(expected, given.run(), "{}", title);
            }

            #[rstest]
            #[case("all hooks and assertions are succeeded",
                new_status_test_success(Yaml::Boolean(true)),
                vec![("setup1", None), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", None)],
                indexmap!{},
                vec![(Setup, "setup1"), (Setup, "setup2"), (Teardown, "teardown2"), (Teardown, "teardown1")])]
            #[case("when assertion is failed, remaining hooks are executed",
                new_status_test_failure(Yaml::Boolean(true)),
                vec![("setup1", None), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", None)],
                indexmap!{ format!("main:{}", *STATUS_STRING) => vec![TestMatcher::failure_message(42)] },
                vec![(Setup, "setup1"), (Setup, "setup2"), (Teardown, "teardown2"), (Teardown, "teardown1")])]
            #[case("when first setup hook is failed, target command and remaning hooks are not executed",
                new_status_test_failure(Yaml::Boolean(true)),
                vec![("setup1", Some("setup1 failed")), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", None)],
                indexmap!{ SETUP_STRING.clone() => vec!["setup1 failed".to_string()] },
                vec![(Setup, "setup1")])]
            #[case("when first teardown hook is failed, remaining hooks are executed",
                new_status_test_failure(Yaml::Boolean(true)),
                vec![("setup1", None), ("setup2", None)],
                vec![("teardown1", None), ("teardown2", Some("teardown2 failed"))],
                indexmap!{ format!("main:{}", *STATUS_STRING) => vec![TestMatcher::failure_message(42)], TEARDOWN_STRING.clone() => vec!["teardown2 failed".to_string()] },
                vec![(Setup, "setup1"), (Setup, "setup2"), (Teardown, "teardown2"), (Teardown, "teardown1")])]
            fn when_hooks_given(
                #[case] title: &str,
                #[case] status_matcher: StatusMatcher,
                #[case] setup_hooks: Vec<(&'static str, Option<&'static str>)>,
                #[case] teardown_hooks: Vec<(&'static str, Option<&'static str>)>,
                #[case] expected_failures: IndexMap<String, Vec<String>>,
                #[case] expected_history: HookHistory,
            ) {
                let history = Rc::new(RefCell::new(vec![]));

                let given = TestCaseTemplate {
                    processes: indexmap! {
                        "main" => ProcessTemplate {
                            command: vec!["bash", "-c", "exit 42"],
                            status_matchers: vec![status_matcher],
                            ..Default::default()
                        }
                    },
                    setup_hooks: setup_hooks
                        .iter()
                        .map(|(name, err)| new_test_setup_hook(name, *err, Rc::clone(&history)))
                        .collect(),
                    teardown_hooks: teardown_hooks
                        .iter()
                        .map(|(name, err)| new_test_teardown_hook(name, *err, Rc::clone(&history)))
                        .collect(),
                    ..Default::default()
                }
                .build();

                let result = given.run();
                assert_eq!(
                    TestResult {
                        name: DEFAULT_NAME.into(),
                        failures: expected_failures
                            .iter()
                            .map(|(subject, messages)| (subject.to_string(), messages.clone()))
                            .collect()
                    },
                    result,
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

            #[rstest]
            fn when_exec_failed() {
                let given = TestCaseTemplate {
                    processes: indexmap! { "main" => ProcessTemplate { command: vec!["_unknown"], ..Default::default() } },
                    ..Default::default()
                }
                .build();

                let actual = given.run();

                assert_eq!(DEFAULT_NAME, actual.name);
                assert_eq!(1, actual.failures.len());
                assert_eq!(1, actual.failures.get("main:exec").unwrap().len());
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
