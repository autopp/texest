mod registry;
mod status;
mod stream;

use std::fmt::Debug;

use crate::validator::Validator;

pub trait Matcher<T>: Debug {
    fn matches(&self, actual: &T) -> Result<(bool, String), String>;
    fn serialize(&self) -> (&str, &str, serde_yaml::Value);
}

impl<T> PartialEq for dyn Matcher<T> {
    fn eq(&self, other: &Self) -> bool {
        self.serialize() == other.serialize()
    }
}

pub type MatcherParser<T> =
    fn(v: &mut Validator, x: &serde_yaml::Value) -> Option<Box<dyn Matcher<T>>>;

pub use registry::{
    new_status_matcher_registry, new_stream_matcher_registry, StatusMatcherRegistry,
    StreamMatcherRegistry,
};

#[cfg(test)]
pub mod testutil {
    use std::fmt::Debug;

    use serde_yaml::Value;

    use crate::validator::Validator;

    use super::{registry::MatcherRegistry, Matcher};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Kind {
        Success,
        Failure,
        Error,
    }

    pub const VIOLATION_MESSAGE: &str = "violation";

    #[derive(Debug, Clone, PartialEq)]
    pub struct TestMatcher {
        pub kind: Kind,
        pub param: Value,
    }

    impl<T: Debug> Matcher<T> for TestMatcher {
        fn matches(&self, actual: &T) -> Result<(bool, String), String> {
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

    impl TestMatcher {
        pub fn new_success<T: Debug>(param: Value) -> Box<dyn Matcher<T>> {
            let b: Box<dyn Matcher<T>> = Box::new(TestMatcher {
                kind: Kind::Success,
                param,
            });
            b
        }

        pub fn new_failure<T: Debug>(param: Value) -> Box<dyn Matcher<T>> {
            let b: Box<dyn Matcher<T>> = Box::new(TestMatcher {
                kind: Kind::Failure,
                param,
            });
            b
        }

        pub fn new_error<T: Debug>(param: Value) -> Box<dyn Matcher<T>> {
            let b: Box<dyn Matcher<T>> = Box::new(TestMatcher {
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
    ) -> Option<Box<dyn Matcher<T>>> {
        Some(TestMatcher::new_success(x.clone()))
    }

    pub fn parse_failure<T: Debug>(
        _v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn Matcher<T>>> {
        Some(TestMatcher::new_failure(x.clone()))
    }

    pub fn parse_error<T: Debug>(
        _v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn Matcher<T>>> {
        Some(TestMatcher::new_error(x.clone()))
    }

    pub fn error_parse<T: Debug>(
        v: &mut Validator,
        _x: &serde_yaml::Value,
    ) -> Option<Box<dyn Matcher<T>>> {
        v.add_violation(VIOLATION_MESSAGE);
        None
    }

    pub const SUCCESS_MATCHER: &str = "success";
    pub const FAILURE_MATCHER: &str = "failure";
    pub const ERROR_MATCHER: &str = "error";
    pub const PARSE_ERROR_MATCHER: &str = "parse_error";

    pub fn new_test_matcher_registry<T: Debug>() -> MatcherRegistry<T> {
        let mut r = MatcherRegistry::new("test");
        r.register(SUCCESS_MATCHER, parse_success);
        r.register(FAILURE_MATCHER, parse_failure);
        r.register(ERROR_MATCHER, parse_error);
        r.register(PARSE_ERROR_MATCHER, error_parse);
        r
    }
}
