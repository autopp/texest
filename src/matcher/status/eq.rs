use crate::validator::Validator;

#[derive(Debug, Clone, PartialEq)]
pub struct EqMatcher {
    pub(super) expected: i32,
}

impl EqMatcher {
    pub fn matches(&self, actual: i32) -> Result<(bool, String), String> {
        let matched = self.expected == actual;

        Ok((
            matched,
            if matched {
                format!("should not be {}, but got it", actual)
            } else {
                format!("should be {}, but got {}", self.expected, actual)
            },
        ))
    }

    pub fn parse(v: &mut Validator, x: &serde_yaml::Value) -> Option<Self> {
        v.must_be_uint(x).and_then(|expected| {
            i32::try_from(expected)
                .map_err(|err| {
                    v.add_violation(format!("cannot treat {} as i32", expected));
                    err
                })
                .ok()
                .map(|expected| Self { expected })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case(0, true, "should not be 0, but got it")]
    #[case(1, false, "should be 0, but got 1")]
    fn matches(#[case] given: i32, #[case] expected_matched: bool, #[case] expected_message: &str) {
        let m = EqMatcher { expected: 0 };
        assert_eq!(
            m.matches(given),
            Ok((expected_matched, expected_message.to_string()))
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
            let x = serde_yaml::to_value(1).unwrap();
            let actual = EqMatcher::parse(&mut v, &x).unwrap();

            assert_eq!(EqMatcher { expected: 1 }, actual);
        }

        #[rstest]
        #[case("with negative number", Value::from(-1), "should be uint, but is int")]
        #[case("with over i32", Value::from(2_i64.pow(32)), "cannot treat 4294967296 as i32")]
        #[case("with not int", Value::from("hello"), "should be uint, but is string")]
        fn failure_cases(
            #[case] title: &str,
            #[case] given: Value,
            #[case] expected_message: &str,
        ) {
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
