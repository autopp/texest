use std::collections::HashMap;

use super::{status, stream, MatcherParser};

pub struct MatcherRegistry<T> {
    target: String,
    matchers: HashMap<String, MatcherParser<T>>,
}

impl<T> MatcherRegistry<T> {
    pub fn new(target: &str) -> Self {
        Self {
            target: target.to_string(),
            matchers: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, parser: MatcherParser<T>) {
        if self.matchers.insert(name.to_string(), parser).is_some() {
            panic!("matcher {} is already registered", name);
        }
    }

    pub fn parse(
        &self,
        name: &str,
        v: &mut super::Validator,
        param: &serde_yaml::Value,
    ) -> Option<Box<dyn super::Matcher<T>>> {
        match self.matchers.get(name) {
            Some(parser) => v.in_field(name, |v| parser(v, param)),
            None => {
                v.add_violation(format!("{} matcher {} is not defined", self.target, name));
                None
            }
        }
    }
}

pub type StatusMatcherRegistry = MatcherRegistry<i32>;

pub fn new_status_matcher_registry() -> StatusMatcherRegistry {
    let mut r = StatusMatcherRegistry::new("status");
    r.register("eq", status::parse_eq_matcher);
    r
}

pub type StreamMatcherRegistry = MatcherRegistry<Vec<u8>>;

pub fn new_stream_matcher_registry() -> StreamMatcherRegistry {
    let mut r = StreamMatcherRegistry::new("stream");
    r.register("eq", stream::parse_eq_matcher);
    r.register("contain", stream::parse_contain_matcher);
    r.register("eq_json", stream::parse_eq_json_matcher);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    mod matcher_registry {
        use super::*;

        mod parse {
            use std::vec;

            use crate::{
                matcher::testutil::{error_parse, parse_success, TestMatcher, VIOLATION_MESSAGE},
                validator::{Validator, Violation},
            };

            use super::*;
            use pretty_assertions::assert_eq;
            use serde_yaml::Value;

            const NAME: &str = "some";

            #[test]
            fn success_case() {
                let mut r = MatcherRegistry::<i32>::new("test");
                r.register(NAME, parse_success);

                let mut v = Validator::new("test.yaml");
                let param = Value::from(true);

                let actual = r.parse(NAME, &mut v, &param);
                let expected = TestMatcher::new_success::<i32>(param);

                assert_eq!(&expected, &actual.unwrap())
            }

            #[test]
            fn failure_case_undefined() {
                let r = MatcherRegistry::<i32>::new("test");

                let mut v = Validator::new("test.yaml");
                let param = Value::from(true);

                let actual = r.parse(NAME, &mut v, &param);

                assert!(actual.is_none());
                assert_eq!(
                    vec![Violation {
                        filename: "test.yaml".to_string(),
                        path: "$".to_string(),
                        message: format!("test matcher {} is not defined", NAME)
                    }],
                    v.violations,
                )
            }

            #[test]
            fn failure_case_parse_error() {
                let mut r = MatcherRegistry::<i32>::new("test");
                r.register(NAME, error_parse);

                let mut v = Validator::new("test.yaml");
                let param = Value::from(true);

                let actual = r.parse(NAME, &mut v, &param);

                assert!(actual.is_none());
                assert_eq!(
                    vec![Violation {
                        filename: "test.yaml".to_string(),
                        path: format!("$.{}", NAME),
                        message: VIOLATION_MESSAGE.to_string()
                    }],
                    v.violations,
                )
            }
        }
    }
}
