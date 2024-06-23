use assert_json_diff::{assert_json_matches_no_panic, Config};

use crate::validator::Validator;

#[derive(Debug, Clone, PartialEq)]
pub struct IncludeJsonMatcher {
    pub(super) expected: serde_json::Value,
    pub(super) original: String,
}

impl IncludeJsonMatcher {
    pub fn matches(&self, actual: &[u8]) -> Result<(bool, String), String> {
        let actual_str = String::from_utf8(actual.to_vec()).map_err(|_err| {
            format!(
                "should be valid JSON string, but got \"{}\"",
                String::from_utf8_lossy(actual)
            )
        })?;

        let parsed: Result<serde_json::Value, serde_json::Error> =
            serde_json::from_str(&actual_str);
        if parsed.is_err() {
            return Ok((
                false,
                format!("should be valid JSON string, but got \"{}\"", actual_str),
            ));
        }

        let actual_json = parsed.unwrap();

        match assert_json_matches_no_panic(
            &actual_json,
            &self.expected,
            Config::new(assert_json_diff::CompareMode::Inclusive),
        ) {
            Ok(_) => Ok((
                true,
                format!("should not include {} as JSON, but got it", self.original),
            )),
            Err(msg) => Ok((false, msg)),
        }
    }

    pub fn parse(v: &mut Validator, x: &serde_yaml::Value) -> Option<Self> {
        v.must_be_string(x)
            .and_then(|original| match serde_json::from_str(&original) {
                Ok(expected) => Some(Self { expected, original }),
                _ => {
                    v.add_violation(format!(
                        "should be valid JSON string, but got \"{}\"",
                        original
                    ));
                    None
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case(
        r#"{"nums": [1, 2], "message":"hello"}"#,
        true,
        r#"should not include {"message": "hello", "nums": [1, 2]} as JSON, but got it"#
    )]
    #[case(
        r#"{"nums": [1, 2, 3], "message":"hello", "passed": true}"#,
        true,
        r#"should not include {"message": "hello", "nums": [1, 2]} as JSON, but got it"#
    )]
    #[case(
        r#"{"message": "world", "nums": [1, 2]}"#,
        false,
        r#"json atoms at path ".message" are not equal:
    expected:
        "hello"
    actual:
        "world""#
    )]
    #[case(
        r#"{"message": "hello", "nums": [1, 2],"#,
        false,
        r#"should be valid JSON string, but got "{"message": "hello", "nums": [1, 2],""#
    )]
    fn matches(
        #[case] given: &str,
        #[case] expected_matched: bool,
        #[case] expected_message: &str,
    ) {
        let original = r#"{"message": "hello", "nums": [1, 2]}"#;
        let m = IncludeJsonMatcher {
            original: original.into(),
            expected: serde_json::from_str(original).unwrap(),
        };
        assert_eq!(
            Ok((expected_matched, expected_message.to_string())),
            m.matches(given.as_bytes()),
        );
    }

    mod parse {
        use serde_yaml::Value;

        use super::*;
        use crate::validator::testutil::new_validator;
        use pretty_assertions::assert_eq;

        #[test]
        fn success_case() {
            let (mut v, _) = new_validator();
            let original = r#"{"message": "hello"}"#;
            let x = serde_yaml::to_value(original).unwrap();
            let actual = IncludeJsonMatcher::parse(&mut v, &x).unwrap();

            let mut m = serde_json::Map::new();
            m.insert(
                "message".to_string(),
                serde_json::Value::String("hello".to_string()),
            );
            let expected = IncludeJsonMatcher {
                original: original.into(),
                expected: serde_json::Value::Object(m),
            };
            assert_eq!(expected, actual);
        }

        #[rstest]
        #[case("with not string", Value::from(true), "should be string, but is bool")]
        #[case(
            "with not valid JSON string",
            Value::from(r#"{"message":"#),
            r#"should be valid JSON string, but got "{"message":""#
        )]
        fn failure_cases(
            #[case] title: &str,
            #[case] given: Value,
            #[case] expected_message: &str,
        ) {
            let (mut v, violation) = new_validator();
            let actual = IncludeJsonMatcher::parse(&mut v, &given);

            assert!(actual.is_none(), "{}", title);
            assert_eq!(
                vec![violation("", expected_message)],
                v.violations,
                "{}",
                title
            );
        }
    }
}
