use crate::{matcher::Matcher, validator::Validator};

use super::STREAM_MATCHER_TAG;

#[derive(Debug, Clone, PartialEq)]
pub struct ContainMatcher {
    expected: Vec<u8>,
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

    fn serialize(&self) -> (&str, &str, serde_yaml::Value) {
        (
            STREAM_MATCHER_TAG,
            "contain",
            serde_yaml::to_value(&self.expected).unwrap(),
        )
    }
}

pub fn parse_contain_matcher(
    v: &mut Validator,
    x: &serde_yaml::Value,
) -> Option<Box<dyn Matcher<Vec<u8>>>> {
    v.must_be_string(x).map(|expected| {
        let b: Box<dyn Matcher<Vec<u8>>> = Box::new(ContainMatcher {
            expected: expected.into(),
        });
        b
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
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
            Ok((expected_matched, expected_message.to_string())),
            m.matches(&given.as_bytes().to_vec())
        );
    }

    mod parser {
        use serde_yaml::Value;

        use super::*;
        use crate::validator::testutil::new_validator;
        use pretty_assertions::assert_eq;

        #[test]
        fn success_case() {
            let (mut v, _) = new_validator();
            let x = Value::from("hello");
            let actual = parse_contain_matcher(&mut v, &x).unwrap();

            let expected: Box<dyn Matcher<Vec<u8>>> = Box::new(ContainMatcher {
                expected: "hello".into(),
            });
            assert_eq!(&expected, &actual);
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
                vec![violation("", expected_message)],
                v.violations,
                "{}",
                title
            );
        }
    }
}
