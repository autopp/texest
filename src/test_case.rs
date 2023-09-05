use std::time::Duration;

#[derive(PartialEq, Debug)]
pub struct TestCase {
    pub filename: String,
    pub path: String,
    pub command: Vec<String>,
    pub timeout: Duration,
}
