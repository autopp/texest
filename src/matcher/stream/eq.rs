use std::any::Any;

use crate::{matcher::Matcher, validator::Validator};

#[derive(Debug, Clone, PartialEq)]
pub struct EqMatcher {
    expected: Vec<u8>,
}

impl PartialEq<dyn Any> for EqMatcher {
    fn eq(&self, other: &dyn Any) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.expected == other.expected)
    }
}

impl Matcher<Vec<u8>> for EqMatcher {
    fn matches(&self, actual: &Vec<u8>) -> Result<(bool, String), String> {
        let matched = self.expected == *actual;

        Ok((
            matched,
            if matched {
                format!(
                    "should not be \"{}\", but got it",
                    String::from_utf8_lossy(actual)
                )
            } else {
                format!(
                    "should be \"{}\", but got \"{}\"",
                    String::from_utf8_lossy(&self.expected),
                    String::from_utf8_lossy(actual)
                )
            },
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn parse_eq_matcher(
    v: &mut Validator,
    x: &serde_yaml::Value,
) -> Option<Box<dyn Matcher<Vec<u8>> + 'static>> {
    v.must_be_string(x).map(|expected| {
        let b: Box<dyn Matcher<Vec<u8>> + 'static> = Box::new(EqMatcher {
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
    #[case("hello", true, "should not be \"hello\", but got it")]
    #[case("goodbye", false, "should be \"hello\", but got \"goodbye\"")]
    fn matches(
        #[case] given: &str,
        #[case] expected_matched: bool,
        #[case] expected_message: &str,
    ) {
        let m = EqMatcher {
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
            let x = serde_yaml::to_value("hello").unwrap();
            let actual = parse_eq_matcher(&mut v, &x).unwrap();

            let casted: Option<&EqMatcher> = actual.as_any().downcast_ref();

            assert_eq!(
                casted,
                Some(&EqMatcher {
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
            let actual = parse_eq_matcher(&mut v, &given);

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
