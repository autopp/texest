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
    pub status_matcher: Option<Box<dyn Matcher<i32>>>,
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

        if let Some(status_matcher) = &self.status_matcher {
            other
                .status_matcher
                .as_ref()
                .is_some_and(|other_status_matcher| {
                    status_matcher.eq(other_status_matcher.as_any())
                })
        } else {
            other.status_matcher.is_none()
        }
    }
}
