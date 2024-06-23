use similar::TextDiff;

use crate::{matcher::MatcherOld, validator::Validator};

use super::STREAM_MATCHER_TAG;

#[derive(Debug, Clone, PartialEq)]
pub struct EqMatcher {
    expected: Vec<u8>,
}

impl MatcherOld<Vec<u8>> for EqMatcher {
    fn matches_old(&self, actual: &Vec<u8>) -> Result<(bool, String), String> {
        if actual == &self.expected {
            Ok((
                true,
                format!(
                    "should not be \"{}\", but got it",
                    String::from_utf8_lossy(actual)
                ),
            ))
        } else {
            let diff_message = TextDiff::from_lines(&self.expected, actual)
                .iter_all_changes()
                .map(|change| {
                    let tag = match change.tag() {
                        similar::ChangeTag::Delete => "-",
                        similar::ChangeTag::Insert => "+",
                        similar::ChangeTag::Equal => " ",
                    };
                    format!("{}{}", tag, change)
                })
                .collect::<Vec<_>>()
                .join("");

            Ok((false, format!("not equals:\n\n{}", diff_message)))
        }
    }

    fn serialize(&self) -> (&str, &str, serde_yaml::Value) {
        (
            STREAM_MATCHER_TAG,
            "eq",
            serde_yaml::to_value(&self.expected).unwrap(),
        )
    }
}

pub fn parse_eq_matcher(
    v: &mut Validator,
    x: &serde_yaml::Value,
) -> Option<Box<dyn MatcherOld<Vec<u8>>>> {
    v.must_be_string(x).map(|expected| {
        let b: Box<dyn MatcherOld<Vec<u8>>> = Box::new(EqMatcher {
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
    #[case("hello", true, "should not be \"hello\", but got it")]
    #[case("goodbye", false, "not equals:\n\n-hello\n+goodbye\n")]
    fn matches(
        #[case] given: &str,
        #[case] expected_matched: bool,
        #[case] expected_message: &str,
    ) {
        let m = EqMatcher {
            expected: "hello".into(),
        };
        assert_eq!(
            Ok((expected_matched, expected_message.to_string())),
            m.matches_old(&given.as_bytes().to_vec()),
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
            let x = serde_yaml::to_value("hello").unwrap();
            let actual = parse_eq_matcher(&mut v, &x).unwrap();
            let expected: Box<dyn MatcherOld<Vec<u8>>> = Box::new(EqMatcher {
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
            let actual = parse_eq_matcher(&mut v, &given);

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
