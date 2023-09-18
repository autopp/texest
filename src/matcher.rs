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
