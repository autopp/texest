use std::any::Any;

use crate::{matcher::Matcher, validator::Validator};

#[derive(Debug, Clone, PartialEq)]
pub struct ContainMatcher {
    expected: Vec<u8>,
}

impl PartialEq<dyn Any> for ContainMatcher {
    fn eq(&self, other: &dyn Any) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.expected == other.expected)
    }
}

impl Matcher<Vec<u8>> for ContainMatcher {
    fn matches(&self, actual: &Vec<u8>) -> Result<(bool, String), String> {
        let matched = actual
            .windows(self.expected.len())
            .any(|w| w == self.expected);

        Ok((
            matched,
            if matched {
                format!(
                    "should not contain \"{}\", but contain it",
                    String::from_utf8_lossy(&self.expected)
                )
            } else {
                format!(
                    "should contain \"{}\", but don't contain it",
                    String::from_utf8_lossy(&self.expected)
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
) -> Option<Box<dyn Matcher<Vec<u8>> + 'static>> {
    v.must_be_string(x).map(|expected| {
        let b: Box<dyn Matcher<Vec<u8>> + 'static> = Box::new(ContainMatcher {
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
        "should contain \"hello\", but don't contain it"
    )]
    fn matches(
        #[case] given: &str,
        #[case] expected_matched: bool,
        #[case] expected_message: &str,
    ) {
        let m = ContainMatcher {
            expected: "hello".into(),
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
            let x = Value::from("hello");
            let actual = parse_contain_matcher(&mut v, &x).unwrap();

            let casted: Option<&ContainMatcher> = actual.as_any().downcast_ref();

            assert_eq!(
                casted,
                Some(&ContainMatcher {
                    expected: "hello".into()
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
