mod contain;
mod eq;
mod eq_json;
mod include_json;
mod match_regex;
use contain::ContainMatcher;
use eq::EqMatcher;
use eq_json::EqJsonMatcher;
use include_json::IncludeJsonMatcher;
use match_regex::MatchRegexMatcher;
use saphyr::Yaml;

use crate::validator::Validator;

use super::parse_name;

#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum StreamMatcher {
    Eq(eq::EqMatcher),
    Contain(contain::ContainMatcher),
    EqJson(eq_json::EqJsonMatcher),
    IncludeJson(include_json::IncludeJsonMatcher),
    MatchRegex(match_regex::MatchRegexMatcher),
    #[cfg(test)]
    Test(super::testutil::TestMatcher),
}

impl StreamMatcher {
    pub fn matches(&self, actual: &[u8]) -> Result<(bool, String), String> {
        match self {
            StreamMatcher::Eq(m) => m.matches(actual),
            StreamMatcher::Contain(m) => m.matches(actual),
            StreamMatcher::EqJson(m) => m.matches(actual),
            StreamMatcher::IncludeJson(m) => m.matches(actual),
            StreamMatcher::MatchRegex(m) => m.matches(actual),
            #[cfg(test)]
            StreamMatcher::Test(m) => m.matches(actual),
        }
    }

    pub fn parse(v: &mut Validator, name: &str, param: &Yaml) -> Option<(Self, bool)> {
        let (name, expected_passed) = parse_name(name);

        #[cfg(test)]
        if let Some(m) = super::testutil::parse_test_matcher(v, name, param) {
            return m.map(|m| (StreamMatcher::Test(m), expected_passed));
        }

        match name {
            "eq" => v.in_field(name, |v| EqMatcher::parse(v, param).map(StreamMatcher::Eq)),
            "contain" => v.in_field(name, |v| {
                ContainMatcher::parse(v, param).map(StreamMatcher::Contain)
            }),
            "eq_json" => v.in_field(name, |v| {
                EqJsonMatcher::parse(v, param).map(StreamMatcher::EqJson)
            }),
            "include_json" => v.in_field(name, |v| {
                IncludeJsonMatcher::parse(v, param).map(StreamMatcher::IncludeJson)
            }),
            "match_regex" => v.in_field(name, |v| {
                MatchRegexMatcher::parse(v, param).map(StreamMatcher::MatchRegex)
            }),
            _ => {
                v.add_violation(format!("stream matcher \"{}\" is not defined", name));
                None
            }
        }
        .map(|m| (m, expected_passed))
    }
}

#[cfg(test)]
pub mod testutil {
    use crate::matcher::testutil::TestMatcher;

    use super::*;

    pub fn new_stream_test_success(param: Yaml) -> StreamMatcher {
        StreamMatcher::Test(TestMatcher::new_success(param))
    }

    pub fn new_stream_test_failure(param: Yaml) -> StreamMatcher {
        StreamMatcher::Test(TestMatcher::new_failure(param))
    }
}

#[cfg(test)]
mod tests {
    use crate::validator::testutil;

    use super::*;
    use match_regex::MatchRegexMatcher;
    use pretty_assertions::assert_eq;
    use regex::Regex;
    use rstest::rstest;

    #[rstest]
    #[case("with eq", "eq", Yaml::String("hello".to_string()), Some((StreamMatcher::Eq(EqMatcher { expected: "hello".into() }), true)), vec![])]
    #[case("with not.eq", "not.eq", Yaml::String("hello".to_string()), Some((StreamMatcher::Eq(EqMatcher { expected: "hello".into() }), false)), vec![])]
    #[case("with contain", "contain", Yaml::String("hello".to_string()), Some((StreamMatcher::Contain(ContainMatcher { expected: "hello".into() }), true)), vec![])]
    #[case("with eq_json",
        "eq_json",
        Yaml::String(r#"{"message": "hello"}"#.to_string()),
        {
            let mut m = serde_json::Map::new();
            m.insert("message".to_string(), serde_json::Value::String("hello".to_string()));
            Some((StreamMatcher::EqJson(EqJsonMatcher {
                expected: serde_json::Value::Object(m),
                original: r#"{"message": "hello"}"#.into(),
            }), true))
        },
        vec![])]
    #[case("with include_json",
        "include_json",
        Yaml::String(r#"{"message": "hello"}"#.to_string()),
        {
            let mut m = serde_json::Map::new();
            m.insert("message".to_string(), serde_json::Value::String("hello".to_string()));
            Some((StreamMatcher::IncludeJson(IncludeJsonMatcher {
                expected: serde_json::Value::Object(m),
                original: r#"{"message": "hello"}"#.into(),
            }), true))
        },
        vec![])]
    #[case("with match_regex",
        "match_regex",
        Yaml::String("hel*o".to_string()),
        Some((StreamMatcher::MatchRegex(MatchRegexMatcher {
            expected: Regex::new("hel*o").unwrap(),
        }), true)),
        vec![])]
    #[case("with unknown name", "unknown", Yaml::Boolean(true), None, vec![("", "stream matcher \"unknown\" is not defined")])]
    fn parse(
        #[case] title: &str,
        #[case] name: &str,
        #[case] param: Yaml,
        #[case] expected_value: Option<(StreamMatcher, bool)>,
        #[case] expected_violation: Vec<(&str, &str)>,
    ) {
        let (mut v, violation) = testutil::new_validator();
        let actual = StreamMatcher::parse(&mut v, name, &param);

        assert_eq!(expected_value, actual, "{}", title);
        assert_eq!(
            expected_violation
                .iter()
                .map(|(path, msg)| violation(path, msg))
                .collect::<Vec<_>>(),
            v.violations,
            "{}",
            title
        );
    }
}
