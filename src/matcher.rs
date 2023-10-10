mod registry;
mod status;
mod stream;

use std::{any::Any, fmt::Debug};

use crate::validator::Validator;

pub trait Matcher<T>: Debug + PartialEq<dyn Any> {
    fn matches(&self, actual: T) -> Result<(bool, String), String>;
    fn as_any(&self) -> &dyn std::any::Any;
}

pub type MatcherParser<T> =
    fn(v: &mut Validator, x: &serde_yaml::Value) -> Option<Box<dyn Matcher<T>>>;

pub use registry::{
    new_status_matcher_registry, new_stream_matcher_registry, StatusMatcherRegistry,
    StreamMatcherRegistry,
};

#[cfg(test)]
pub mod testutil {
    use std::{any::Any, fmt::Debug};

    use serde_yaml::Value;

    use crate::validator::Validator;

    use super::{registry::MatcherRegistry, Matcher};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Kind {
        Success,
        Failure,
        Error,
    }

    pub const SUCCESS_MESSAGE: &str = "success";
    pub const FAILURE_MESSAGE: &str = "failure";
    pub const ERROR_MESSAGE: &str = "error";
    pub const VIOLATION_MESSAGE: &str = "violation";

    #[derive(Debug, Clone, PartialEq)]
    pub struct TestMatcher {
        pub kind: Kind,
        pub param: Value,
    }

    impl PartialEq<dyn Any> for TestMatcher {
        fn eq(&self, other: &dyn Any) -> bool {
            other
                .downcast_ref::<Self>()
                .is_some_and(|other| self.kind == other.kind && self.param == other.param)
        }
    }

    impl<T: Debug> Matcher<T> for TestMatcher {
        fn matches(&self, _actual: T) -> Result<(bool, String), String> {
            match self.kind {
                Kind::Success => Ok((true, SUCCESS_MESSAGE.to_string())),
                Kind::Failure => Ok((false, FAILURE_MESSAGE.to_string())),
                Kind::Error => Err(ERROR_MESSAGE.to_string()),
            }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl TestMatcher {
        pub fn new_success<T: Debug>(param: Value) -> Box<dyn Matcher<T>> {
            let b: Box<dyn Matcher<T> + 'static> = Box::new(TestMatcher {
                kind: Kind::Success,
                param,
            });
            b
        }

        pub fn new_failure<T: Debug>(param: Value) -> Box<dyn Matcher<T>> {
            let b: Box<dyn Matcher<T> + 'static> = Box::new(TestMatcher {
                kind: Kind::Failure,
                param,
            });
            b
        }

        pub fn new_error<T: Debug>(param: Value) -> Box<dyn Matcher<T>> {
            let b: Box<dyn Matcher<T> + 'static> = Box::new(TestMatcher {
                kind: Kind::Error,
                param,
            });
            b
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
        let mut r = MatcherRegistry::new("test".to_string());
        r.register(SUCCESS_MATCHER.to_string(), parse_success);
        r.register(FAILURE_MATCHER.to_string(), parse_failure);
        r.register(ERROR_MATCHER.to_string(), parse_error);
        r.register(PARSE_ERROR_MATCHER.to_string(), error_parse);
        r
    }
}
