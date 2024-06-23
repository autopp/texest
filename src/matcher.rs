mod registry;
pub mod status;
mod stream;

use std::fmt::Debug;

use crate::validator::Validator;

pub trait MatcherOld<T>: Debug {
    fn matches_old(&self, actual: &T) -> Result<(bool, String), String>;
    fn serialize(&self) -> (&str, &str, serde_yaml::Value);
}

impl<T> PartialEq for dyn MatcherOld<T> {
    fn eq(&self, other: &Self) -> bool {
        self.serialize() == other.serialize()
    }
}

pub type MatcherParser<T> =
    fn(v: &mut Validator, x: &serde_yaml::Value) -> Option<Box<dyn MatcherOld<T>>>;

pub use registry::{new_stream_matcher_registry, MatcherRegistry, StreamMatcherRegistry};

pub use status::StatusMatcher;

#[cfg(test)]
pub mod testutil {
    use std::fmt::Debug;

    use serde_yaml::Value;

    use crate::validator::Validator;

    use super::{registry::MatcherRegistry, MatcherOld};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Kind {
        Success,
        Failure,
        Error,
    }

    pub const PARSE_ERROR_VIOLATION_MESSAGE: &str = "violation";

    #[derive(Debug, Clone, PartialEq)]
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

    #[derive(Debug, Clone, PartialEq)]
    pub struct TestMatcherOld {
        pub kind: Kind,
        pub param: Value,
    }

    impl<T: Debug> MatcherOld<T> for TestMatcherOld {
        fn matches_old(&self, actual: &T) -> Result<(bool, String), String> {
            match self.kind {
                Kind::Success => Ok((true, Self::success_message(actual))),
                Kind::Failure => Ok((false, Self::failure_message(actual))),
                Kind::Error => Err(Self::error_message(actual)),
            }
        }

        fn serialize(&self) -> (&str, &str, serde_yaml::Value) {
            match self.kind {
                Kind::Success => ("test", "success", self.param.clone()),
                Kind::Failure => ("test", "failure", self.param.clone()),
                Kind::Error => ("test", "error", self.param.clone()),
            }
        }
    }

    impl TestMatcherOld {
        pub fn new_success<T: Debug>(param: Value) -> Box<dyn MatcherOld<T>> {
            let b: Box<dyn MatcherOld<T>> = Box::new(TestMatcherOld {
                kind: Kind::Success,
                param,
            });
            b
        }

        pub fn new_failure<T: Debug>(param: Value) -> Box<dyn MatcherOld<T>> {
            let b: Box<dyn MatcherOld<T>> = Box::new(TestMatcherOld {
                kind: Kind::Failure,
                param,
            });
            b
        }

        pub fn new_error<T: Debug>(param: Value) -> Box<dyn MatcherOld<T>> {
            let b: Box<dyn MatcherOld<T>> = Box::new(TestMatcherOld {
                kind: Kind::Error,
                param,
            });
            b
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

    pub fn parse_success<T: Debug>(
        _v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn MatcherOld<T>>> {
        Some(TestMatcherOld::new_success(x.clone()))
    }

    pub fn parse_failure<T: Debug>(
        _v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn MatcherOld<T>>> {
        Some(TestMatcherOld::new_failure(x.clone()))
    }

    pub fn parse_error<T: Debug>(
        _v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn MatcherOld<T>>> {
        Some(TestMatcherOld::new_error(x.clone()))
    }

    pub fn error_parse<T: Debug>(
        v: &mut Validator,
        _x: &serde_yaml::Value,
    ) -> Option<Box<dyn MatcherOld<T>>> {
        v.add_violation(PARSE_ERROR_VIOLATION_MESSAGE);
        None
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

    pub fn new_test_matcher_registry<T: Debug>() -> MatcherRegistry<T> {
        let mut r = MatcherRegistry::new("test");
        r.register(TEST_SUCCESS_NAME, parse_success);
        r.register(TEST_FAILURE_NAME, parse_failure);
        r.register(TEST_ERROR_NAME, parse_error);
        r.register(TEST_PARSE_ERROR_NAME, error_parse);
        r
    }
}
