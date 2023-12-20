use std::{any::Any, ffi::OsString};

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

impl Matcher<OsString> for EqJsonMatcher {
    fn matches(&self, actual: OsString) -> Result<(bool, String), String> {
        let actual_str = actual.to_str().ok_or_else(|| {
            format!(
                "should be valid JSON string, but got \"{}\"",
                actual.to_string_lossy()
            )
        })?;

        let parsed: Result<serde_json::Value, serde_json::Error> = serde_json::from_str(actual_str);
        if parsed.is_err() {
            return Ok((
                false,
                format!("should be valid JSON string, but got \"{}\"", actual_str),
            ));
        }

        let actual_json = parsed.unwrap();
        let matched = self.expected == actual_json;

        Ok((
            matched,
            if matched {
                format!("should not be {} as JSON, but got it", self.original)
            } else {
                format!(
                    "should be {} as JSON, but got {}",
                    self.original, actual_str
                )
            },
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn parse_eq_json_matcher(
    v: &mut Validator,
    x: &serde_yaml::Value,
) -> Option<Box<dyn Matcher<OsString> + 'static>> {
    v.must_be_string(x)
        .and_then(|original| match serde_json::from_str(&original) {
            Ok(expected) => {
                let b: Box<dyn Matcher<OsString> + 'static> =
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
        r#"should be {"message": "hello", "nums": [1, 2]} as JSON, but got {"message": "world", "nums": [1, 2]}"#
    )]
    #[case(
        r#"{"message": "hello", "nums": [1, 2, 3]}"#,
        false,
        r#"should be {"message": "hello", "nums": [1, 2]} as JSON, but got {"message": "hello", "nums": [1, 2, 3]}"#
    )]
    #[case(
        r#"{"message": "hello", "nums": [1, 2], "passed": true}"#,
        false,
        r#"should be {"message": "hello", "nums": [1, 2]} as JSON, but got {"message": "hello", "nums": [1, 2], "passed": true}"#
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
        let given: OsString = given.into();

        let original = r#"{"message": "hello", "nums": [1, 2]}"#;
        let m = EqJsonMatcher {
            original: original.into(),
            expected: serde_json::from_str(original).unwrap(),
        };
        assert_eq!(
            m.matches(given),
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
