use regex::Regex;
use saphyr::Yaml;

use crate::validator::Validator;

#[derive(Debug)]
pub struct MatchRegexMatcher {
    pub(super) expected: Regex,
}

impl PartialEq for MatchRegexMatcher {
    fn eq(&self, other: &Self) -> bool {
        self.expected.as_str() == other.expected.as_str()
    }
}

impl MatchRegexMatcher {
    pub fn matches(&self, actual: &[u8]) -> Result<(bool, String), String> {
        match String::from_utf8(actual.to_vec()) {
            Ok(actual_str) => {
                if self.expected.is_match(&actual_str) {
                    Ok((
                        true,
                        format!("should not match to /{}/, but match to it", self.expected),
                    ))
                } else {
                    Ok((
                        false,
                        format!("should match to /{}/, but don't match to it", self.expected),
                    ))
                }
            }
            _ => Ok((false, "should be valid utf8 string".into())),
        }
    }

    pub fn parse(v: &mut Validator, x: &Yaml) -> Option<Self> {
        v.must_be_string(x).and_then(|original| {
            Regex::new(&original)
                .map(|expected| MatchRegexMatcher { expected })
                .map_err(|_| v.add_violation("should be valid regular expression pattern"))
                .ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case("hello world".as_bytes(), true, "should not match to /hel*o/, but match to it")]
    #[case(
        "goodbye world".as_bytes(),
        false,
        "should match to /hel*o/, but don't match to it"
    )]
    #[case(
        &[0xCA, 0xFE, 0xBA, 0xBE],
        false,
        "should be valid utf8 string"
    )]
    fn matches(
        #[case] given: &[u8],
        #[case] expected_matched: bool,
        #[case] expected_message: &str,
    ) {
        let m = MatchRegexMatcher {
            expected: Regex::new("hel*o").unwrap(),
        };
        assert_eq!(
            Ok((expected_matched, expected_message.to_string())),
            m.matches(given)
        );
    }

    mod parse {
        use crate::validator::{testutil::new_validator, Violation};

        use super::*;
        use pretty_assertions::assert_eq;

        #[rstest]
        fn success_case() {
            let (mut v, _) = new_validator();
            let x = Yaml::String("hel*o".to_string());
            let actual = MatchRegexMatcher::parse(&mut v, &x);

            let expected = MatchRegexMatcher {
                expected: Regex::new("hel*o").unwrap(),
            };
            assert_eq!(Some(expected), actual);
            assert_eq!(Vec::<Violation>::new(), v.violations)
        }

        #[rstest]
        #[case(
            "with not string",
            Yaml::Boolean(true),
            "should be string, but is bool"
        )]
        #[case(
            "with not valid regex pattern",
            Yaml::String("(hello".into()),
            "should be valid regular expression pattern"
        )]
        fn failure_cases(#[case] title: &str, #[case] given: Yaml, #[case] expected_message: &str) {
            let (mut v, violation) = new_validator();
            let actual = MatchRegexMatcher::parse(&mut v, &given);

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
