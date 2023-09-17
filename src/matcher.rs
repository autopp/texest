mod status;

use std::{any::Any, fmt::Debug};

pub trait Matcher<T>: Debug + PartialEq<dyn Any> {
    fn matches(&self, actual: T) -> Result<(bool, String), String>;
    fn as_any(&self) -> &dyn std::any::Any;
}

pub trait StatusMatcher: Matcher<i32> {}
