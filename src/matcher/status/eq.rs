use std::any::Any;

use crate::matcher::Matcher;

#[derive(Debug)]
pub struct EqMatcher {
    expected: i32,
}

impl PartialEq<dyn Any> for EqMatcher {
    fn eq(&self, other: &dyn Any) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.expected == other.expected)
    }
}

impl Matcher<i32> for EqMatcher {
    fn matches(&self, actual: i32) -> Result<(bool, String), String> {
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
