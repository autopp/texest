use std::time::Duration;

use crate::{ast::Map, validator::Validator};

#[derive(Debug, Clone, PartialEq)]
pub struct SleepCondition {
    pub duration: Duration,
}

impl SleepCondition {
    pub async fn wait(&self) -> Result<(), String> {
        tokio::time::sleep(self.duration).await;
        Ok(())
    }

    pub fn parse(v: &mut Validator, params: &Map) -> Option<Self> {
        v.must_have_duration(params, "duration")
            .map(|duration| Self { duration })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod sleep_condition {
        use crate::{ast::Map, validator::testutil};

        use super::*;
        use indexmap::indexmap;
        use once_cell::sync::Lazy;
        use pretty_assertions::assert_eq;
        use rstest::rstest;
        use serde_yaml::Value;

        #[tokio::test]
        async fn wait() {
            let given = SleepCondition {
                duration: Duration::from_millis(50),
            };

            let before = std::time::Instant::now();
            let result = given.wait().await;
            let after = std::time::Instant::now();

            let elapsed = after - before;

            assert_eq!(Ok(()), result);
            assert!(elapsed >= Duration::from_millis(50));
        }

        static VALID_DURATION: Lazy<Value> = Lazy::new(|| Value::from("2s"));
        static INVALID_DURATION: Lazy<Value> = Lazy::new(|| Value::from(true));

        #[rstest]
        #[case("with valid duration", indexmap! { "duration" => &*VALID_DURATION }, Some(SleepCondition { duration: Duration::from_secs(2) }), vec![])]
        #[case("with invalid duration", indexmap! { "duration" => &*INVALID_DURATION }, None, vec![(".duration", "should be duration, but is bool")])]
        #[case("with invalid duration", indexmap! {}, None, vec![("", "should have .duration as duration")])]
        fn parse(
            #[case] title: &str,
            #[case] params: Map,
            #[case] expected_value: Option<SleepCondition>,
            #[case] expected_violation: Vec<(&str, &str)>,
        ) {
            let (mut v, violation) = testutil::new_validator();

            assert_eq!(
                expected_value,
                SleepCondition::parse(&mut v, &params),
                "{}",
                title
            );
            assert_eq!(
                expected_violation
                    .into_iter()
                    .map(|(path, msg)| violation(path, msg))
                    .collect::<Vec<_>>(),
                v.violations,
                "{}",
                title
            );
        }
    }
}
