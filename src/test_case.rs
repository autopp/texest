use std::{fmt::Debug, time::Duration};

use crate::matcher::Matcher;

#[derive(Debug)]
pub struct TestCase {
    pub filename: String,
    pub path: String,
    pub command: Vec<String>,
    pub stdin: String,
    pub timeout: Duration,
    pub tee_stdout: bool,
    pub tee_stderr: bool,
    pub status_matchers: Vec<Box<dyn Matcher<i32>>>,
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
