mod contain;
mod eq;
mod eq_json;
mod include_json;
use contain::ContainMatcher;
use eq::EqMatcher;
use eq_json::EqJsonMatcher;
use include_json::IncludeJsonMatcher;
use saphyr::Yaml;

use crate::validator::Validator;

#[derive(Debug, PartialEq)]
pub enum StreamMatcher {
    Eq(eq::EqMatcher),
    Contain(contain::ContainMatcher),
    EqJson(eq_json::EqJsonMatcher),
    IncludeJson(include_json::IncludeJsonMatcher),
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
            #[cfg(test)]
            StreamMatcher::Test(m) => m.matches(actual),
        }
    }

    pub fn parse(v: &mut Validator, name: &str, param: &Yaml) -> Option<Self> {
        #[cfg(test)]
        if let Some(m) = super::testutil::parse_test_matcher(v, name, param) {
            return m.map(StreamMatcher::Test);
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
            _ => {
                v.add_violation(format!("stream matcher \"{}\" is not defined", name));
                None
            }
        }
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
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case("with eq", "eq", Yaml::String("hello".to_string()), Some(StreamMatcher::Eq(EqMatcher { expected: "hello".into() })), vec![])]
    #[case("with contain", "contain", Yaml::String("hello".to_string()), Some(StreamMatcher::Contain(ContainMatcher { expected: "hello".into() })), vec![])]
    #[case("with eq_json",
        "eq_json",
        Yaml::String(r#"{"message": "hello"}"#.to_string()),
        {
            let mut m = serde_json::Map::new();
            m.insert("message".to_string(), serde_json::Value::String("hello".to_string()));
            Some(StreamMatcher::EqJson(EqJsonMatcher {
                expected: serde_json::Value::Object(m),
                original: r#"{"message": "hello"}"#.into(),
            }))
        },
        vec![])]
    #[case("with include_json",
        "include_json",
        Yaml::String(r#"{"message": "hello"}"#.to_string()),
        {
            let mut m = serde_json::Map::new();
            m.insert("message".to_string(), serde_json::Value::String("hello".to_string()));
            Some(StreamMatcher::IncludeJson(IncludeJsonMatcher {
                expected: serde_json::Value::Object(m),
                original: r#"{"message": "hello"}"#.into(),
            }))
        },
        vec![])]
    #[case("with unknown name", "unknown", Yaml::Boolean(true), None, vec![("", "stream matcher \"unknown\" is not defined")])]
    fn parse(
        #[case] title: &str,
        #[case] name: &str,
        #[case] param: Yaml,
        #[case] expected_value: Option<StreamMatcher>,
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
