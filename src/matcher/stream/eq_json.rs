use std::any::Any;

use assert_json_diff::{assert_json_matches_no_panic, Config};

use crate::{matcher::Matcher, validator::Validator};

#[derive(Debug, Clone, PartialEq)]
pub struct EqJsonMatcher {
    expected: serde_json::Value,
    original: String,
}

impl PartialEq<dyn Any> for EqJsonMatcher {
    fn eq(&self, other: &dyn Any) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.expected == other.expected)
    }
}

impl Matcher<Vec<u8>> for EqJsonMatcher {
    fn matches(&self, actual: &Vec<u8>) -> Result<(bool, String), String> {
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
            Config::new(assert_json_diff::CompareMode::Strict),
        ) {
            Ok(_) => Ok((
                true,
                format!("should not be {} as JSON, but got it", self.original),
            )),
            Err(msg) => Ok((false, msg)),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn parse_eq_json_matcher(
    v: &mut Validator,
    x: &serde_yaml::Value,
) -> Option<Box<dyn Matcher<Vec<u8>> + 'static>> {
    v.must_be_string(x)
        .and_then(|original| match serde_json::from_str(&original) {
            Ok(expected) => {
                let b: Box<dyn Matcher<Vec<u8>> + 'static> =
                    Box::new(EqJsonMatcher { expected, original });
                Some(b)
            }
            _ => {
                v.add_violation(format!(
                    "should be valid JSON string, but got \"{}\"",
                    original
                ));
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(
        r#"{"nums": [1, 2], "message":"hello"}"#,
        true,
        r#"should not be {"message": "hello", "nums": [1, 2]} as JSON, but got it"#
    )]
    #[case(
        r#"{"message": "world", "nums": [1, 2]}"#,
        false,
        r#"json atoms at path ".message" are not equal:
    lhs:
        "world"
    rhs:
        "hello""#
    )]
    #[case(
        r#"{"message": "hello", "nums": [1, 2, 3]}"#,
        false,
        r#"json atom at path ".nums[2]" is missing from rhs"#
    )]
    #[case(
        r#"{"message": "hello", "nums": [1, 2], "passed": true}"#,
        false,
        r#"json atom at path ".passed" is missing from rhs"#
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
        let m = EqJsonMatcher {
            original: original.into(),
            expected: serde_json::from_str(original).unwrap(),
        };
        assert_eq!(
            m.matches(&given.as_bytes().to_vec()),
            Ok((expected_matched, expected_message.to_string()))
        );
    }

    mod parser {
        use serde_yaml::Value;

        use super::*;
        use crate::validator::testutil::new_validator;

        #[test]
        fn success_case() {
            let (mut v, _) = new_validator();
            let original = r#"{"message": "hello"}"#;
            let x = serde_yaml::to_value(original).unwrap();
            let actual = parse_eq_json_matcher(&mut v, &x).unwrap();

            let casted: Option<&EqJsonMatcher> = actual.as_any().downcast_ref();

            let mut m = serde_json::Map::new();
            m.insert(
                "message".to_string(),
                serde_json::Value::String("hello".to_string()),
            );
            assert_eq!(
                casted,
                Some(&EqJsonMatcher {
                    original: original.into(),
                    expected: serde_json::Value::Object(m)
                })
            );
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
            let actual = parse_eq_json_matcher(&mut v, &given);

            assert!(actual.is_none(), "{}", title);
            assert_eq!(
                v.violations,
                vec![violation("", expected_message)],
                "{}",
                title
            );
        }
    }
}
