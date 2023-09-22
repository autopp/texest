mod registry;
mod status;

use std::{any::Any, fmt::Debug};

use crate::validator::Validator;

pub trait Matcher<T>: Debug + PartialEq<dyn Any> {
    fn matches(&self, actual: T) -> Result<(bool, String), String>;
    fn as_any(&self) -> &dyn std::any::Any;
}

pub type MatcherParser<T> =
    fn(v: &mut Validator, x: &serde_yaml::Value) -> Option<Box<dyn Matcher<T>>>;

pub use registry::{new_status_matcher_registry, StatusMatcherRegistry};

#[cfg(test)]
mod testutil {
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

    pub fn success_matcher<T: Debug>(
        v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn Matcher<T>>> {
        let b: Box<dyn Matcher<T> + 'static> = Box::new(TestMatcher {
            kind: Kind::Success,
            param: x.clone(),
        });
        Some(b)
    }

    pub fn failure_matcher<T: Debug>(
        v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn Matcher<T>>> {
        let b: Box<dyn Matcher<T> + 'static> = Box::new(TestMatcher {
            kind: Kind::Failure,
            param: x.clone(),
        });
        Some(b)
    }

    pub fn error_matcher<T: Debug>(
        v: &mut Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn Matcher<T>>> {
        let b: Box<dyn Matcher<T> + 'static> = Box::new(TestMatcher {
            kind: Kind::Error,
            param: x.clone(),
        });
        Some(b)
    }

    pub fn error_parse<T: Debug>(
        v: &mut Validator,
        x: &serde_yaml::Value,
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
        r.register(SUCCESS_MATCHER.to_string(), success_matcher);
        r.register(FAILURE_MATCHER.to_string(), failure_matcher);
        r.register(ERROR_MATCHER.to_string(), error_matcher);
        r.register(PARSE_ERROR_MATCHER.to_string(), error_parse);
        r
    }
}
