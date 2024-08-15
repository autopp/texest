use saphyr::Yaml;
use similar::TextDiff;

use crate::validator::Validator;

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct EqMatcher {
    pub(super) expected: Vec<u8>,
}

impl EqMatcher {
    pub fn matches(&self, actual: &[u8]) -> Result<(bool, String), String> {
        if actual == self.expected {
            Ok((
                true,
                format!(
                    "should not be \"{}\", but got it",
                    String::from_utf8_lossy(actual)
                ),
            ))
        } else {
            let diff_message = TextDiff::from_lines(&self.expected, &actual.to_vec())
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

    pub fn parse(v: &mut Validator, x: &Yaml) -> Option<Self> {
        v.must_be_string(x).map(|expected| Self {
            expected: expected.into(),
        })
    }
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
            m.matches(given.as_bytes()),
        );
    }

    mod parse {
        use super::*;
        use crate::validator::testutil::new_validator;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn success_case() {
            let (mut v, _) = new_validator();
            let x = Yaml::String("hello".to_string());
            let actual = EqMatcher::parse(&mut v, &x).unwrap();
            let expected = EqMatcher {
                expected: "hello".into(),
            };

            assert_eq!(expected, actual);
        }

        #[rstest]
        #[case(
            "with not string",
            Yaml::Boolean(true),
            "should be string, but is bool"
        )]
        fn failure_cases(#[case] title: &str, #[case] given: Yaml, #[case] expected_message: &str) {
            let (mut v, violation) = new_validator();
            let actual = EqMatcher::parse(&mut v, &given);

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
