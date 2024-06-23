use crate::validator::Validator;

#[derive(Debug, PartialEq)]
pub struct ContainMatcher {
    pub(super) expected: Vec<u8>,
}

impl ContainMatcher {
    pub fn matches(&self, actual: &[u8]) -> Result<(bool, String), String> {
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

    pub fn parse(v: &mut Validator, x: &serde_yaml::Value) -> Option<Self> {
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
            m.matches(given.as_bytes())
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
            let x = Value::from("hello");
            let actual = ContainMatcher::parse(&mut v, &x).unwrap();

            let expected = ContainMatcher {
                expected: "hello".into(),
            };
            assert_eq!(expected, actual);
        }

        #[rstest]
        #[case("with not string", Value::from(true), "should be string, but is bool")]
        fn failure_cases(
            #[case] title: &str,
            #[case] given: Value,
            #[case] expected_message: &str,
        ) {
            let (mut v, violation) = new_validator();
            let actual = ContainMatcher::parse(&mut v, &given);

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
