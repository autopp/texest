use std::time::Duration;

#[derive(PartialEq, Debug)]
pub struct TestCase {
    pub filename: String,
    pub path: String,
    pub command: Vec<String>,
    pub stdin: String,
    pub timeout: Duration,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
}
