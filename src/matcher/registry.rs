use std::collections::HashMap;

use super::{status::parse_eq_matcher, MatcherParser};

pub struct MatcherRegistry<T> {
    target: String,
    matchers: HashMap<String, MatcherParser<T>>,
}

impl<T> MatcherRegistry<T> {
    pub fn new(target: String) -> Self {
        Self {
            target,
            matchers: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: String, parser: MatcherParser<T>) {
        if self.matchers.insert(name.clone(), parser).is_some() {
            panic!("matcher {} is already registered", name);
        }
    }

    pub fn parse(
        &self,
        name: String,
        v: &mut super::Validator,
        x: &serde_yaml::Value,
    ) -> Option<Box<dyn super::Matcher<T>>> {
        self.matchers.get(&name).and_then(|parser| parser(v, x))
    }
}

pub type StatusMatcherRegistry = MatcherRegistry<i32>;

pub fn new_status_matcher_registry() -> StatusMatcherRegistry {
    let mut r = StatusMatcherRegistry::new("status".to_string());
    r.register("eq".to_string(), parse_eq_matcher);
    r
}
