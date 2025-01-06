mod eq;

use eq::EqMatcher;
use saphyr::Yaml;

use super::parse_name;

#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum StatusMatcher {
    Eq(eq::EqMatcher),
    #[cfg(test)]
    Test(super::testutil::TestMatcher),
}

impl StatusMatcher {
    pub fn matches(&self, actual: i32) -> Result<(bool, String), String> {
        match self {
            StatusMatcher::Eq(m) => m.matches(actual),
            #[cfg(test)]
            StatusMatcher::Test(m) => m.matches(actual),
        }
    }

    pub fn parse(v: &mut super::Validator, name: &str, param: &Yaml) -> Option<(Self, bool)> {
        let (name, expected_passed) = parse_name(name);

        #[cfg(test)]
        if let Some(m) = super::testutil::parse_test_matcher(v, name, param) {
            return m.map(|m| (StatusMatcher::Test(m), expected_passed));
        }

        match name {
            "eq" => v.in_field(name, |v| EqMatcher::parse(v, param).map(StatusMatcher::Eq)),
            _ => {
                v.add_violation(format!("status matcher \"{}\" is not defined", name));
                None
            }
        }
        .map(|m| (m, expected_passed))
    }
}

#[cfg(test)]
pub mod testutil {
    use crate::matcher::testutil::TestMatcher;

    use super::*;

    pub fn new_status_test_success(param: Yaml) -> StatusMatcher {
        StatusMatcher::Test(TestMatcher::new_success(param))
    }

    pub fn new_status_test_failure(param: Yaml) -> StatusMatcher {
        StatusMatcher::Test(TestMatcher::new_failure(param))
    }
}

#[cfg(test)]
mod tests {
    use crate::validator::testutil;

    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use saphyr::Yaml;

    #[rstest]
    #[case("with eq", "eq", Yaml::Integer(1), Some((StatusMatcher::Eq(eq::EqMatcher { expected: 1 }), true)), vec![])]
    #[case("with not.eq", "not.eq", Yaml::Integer(1), Some((StatusMatcher::Eq(eq::EqMatcher { expected: 1 }), false)), vec![])]
    #[case("with unknown name", "unknown", Yaml::Boolean(true), None, vec![("", "status matcher \"unknown\" is not defined")])]
    fn parse(
        #[case] title: &str,
        #[case] name: &str,
        #[case] param: Yaml,
        #[case] expected_value: Option<(StatusMatcher, bool)>,
        #[case] expected_violation: Vec<(&str, &str)>,
    ) {
        let (mut v, violation) = testutil::new_validator();
        let actual = StatusMatcher::parse(&mut v, name, &param);

        assert_eq!(expected_value, actual, "{}", title);
        assert_eq!(
            expected_violation
                .iter()
                .map(|(path, msg)| violation(path, msg))
                .collect::<Vec<_>>(),
            v.violations,
            "{}",
            title
        );
    }
}
