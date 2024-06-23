mod status;
mod stream;

use crate::validator::Validator;

pub use status::StatusMatcher;
pub use stream::StreamMatcher;

#[cfg(test)]
pub mod testutil {
    use std::fmt::Debug;

    use serde_yaml::Value;

    use crate::validator::Validator;

    pub use super::status::testutil::{new_status_test_failure, new_status_test_success};
    pub use super::stream::testutil::{new_stream_test_failure, new_stream_test_success};

    #[derive(Debug, PartialEq)]
    pub enum Kind {
        Success,
        Failure,
        Error,
    }

    pub const PARSE_ERROR_VIOLATION_MESSAGE: &str = "violation";

    #[derive(Debug, PartialEq)]
    pub struct TestMatcher {
        pub kind: Kind,
        pub param: Value,
    }

    impl TestMatcher {
        pub fn matches<T: Debug + Copy>(&self, actual: T) -> Result<(bool, String), String> {
            match self.kind {
                Kind::Success => Ok((true, Self::success_message(actual))),
                Kind::Failure => Ok((false, Self::failure_message(actual))),
                Kind::Error => Err(Self::error_message(actual)),
            }
        }

        pub fn new_success(param: Value) -> Self {
            Self {
                kind: Kind::Success,
                param,
            }
        }

        pub fn new_failure(param: Value) -> Self {
            Self {
                kind: Kind::Failure,
                param,
            }
        }

        pub fn new_error(param: Value) -> Self {
            Self {
                kind: Kind::Error,
                param,
            }
        }

        pub fn success_message(value: impl Debug) -> String {
            format!("success: {:?}", value)
        }

        pub fn failure_message(value: impl Debug) -> String {
            format!("failure: {:?}", value)
        }

        pub fn error_message(value: impl Debug) -> String {
            format!("error: {:?}", value)
        }
    }

    pub const TEST_SUCCESS_NAME: &str = "test_success";
    pub const TEST_FAILURE_NAME: &str = "test_failure";
    pub const TEST_ERROR_NAME: &str = "test_error";
    pub const TEST_PARSE_ERROR_NAME: &str = "test_parse_error";

    pub fn parse_test_matcher(
        v: &mut Validator,
        name: &str,
        param: &serde_yaml::Value,
    ) -> Option<Option<TestMatcher>> {
        v.in_field(name, |v| match name {
            TEST_SUCCESS_NAME => Some(Some(TestMatcher::new_success(param.clone()))),
            TEST_FAILURE_NAME => Some(Some(TestMatcher::new_failure(param.clone()))),
            TEST_ERROR_NAME => Some(Some(TestMatcher::new_error(param.clone()))),
            TEST_PARSE_ERROR_NAME => {
                v.add_violation(PARSE_ERROR_VIOLATION_MESSAGE);
                Some(None)
            }
            _ => None,
        })
    }
}
