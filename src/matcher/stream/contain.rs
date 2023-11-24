use std::{any::Any, ffi::OsString, os::unix::prelude::OsStringExt};

use crate::{matcher::Matcher, validator::Validator};

#[derive(Debug, Clone, PartialEq)]
pub struct ContainMatcher {
    expected: OsString,
}

impl PartialEq<dyn Any> for ContainMatcher {
    fn eq(&self, other: &dyn Any) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.expected == other.expected)
    }
}

impl Matcher<OsString> for ContainMatcher {
    fn matches(&self, actual: OsString) -> Result<(bool, String), String> {
        let expected_bytes = self.expected.clone().into_vec();
        let matched = actual
            .clone()
            .into_vec()
            .windows(expected_bytes.len())
            .any(|w| w == expected_bytes);

        Ok((
            matched,
            if matched {
                format!(
                    "should not contain \"{}\", but contain it",
                    self.expected.to_string_lossy()
                )
            } else {
                format!(
                    "should contain \"{}\", but got \"{}\"",
                    self.expected.to_string_lossy(),
                    actual.to_string_lossy()
                )
            },
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn parse_contain_matcher(
    v: &mut Validator,
    x: &serde_yaml::Value,
) -> Option<Box<dyn Matcher<OsString> + 'static>> {
    v.must_be_string(x).map(|expected| {
        let b: Box<dyn Matcher<OsString> + 'static> = Box::new(ContainMatcher {
            expected: expected.into(),
        });
        b
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("hello world", true, "should not contain \"hello\", but contain it")]
    #[case(
        "goodbye world",
        false,
        "should contain \"hello\", but got \"goodbye world\""
    )]
    fn matches(
        #[case] given: &str,
        #[case] expected_matched: bool,
        #[case] expected_message: &str,
    ) {
        let given: OsString = given.into();
        let m = ContainMatcher {
            expected: "hello".into(),
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
            let x = Value::from("hello");
            let actual = parse_contain_matcher(&mut v, &x).unwrap();

            let casted: Option<&ContainMatcher> = actual.as_any().downcast_ref();

            assert_eq!(
                casted,
                Some(&ContainMatcher {
                    expected: OsString::from("hello")
                })
            );
        }

        #[rstest]
        #[case("with not string", Value::from(true), "should be string, but is bool")]
        fn failure_cases(
            #[case] title: &str,
            #[case] given: Value,
            #[case] expected_message: &str,
        ) {
            let (mut v, violation) = new_validator();
            let actual = parse_contain_matcher(&mut v, &given);

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
