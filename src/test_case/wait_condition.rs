mod http;
mod sleep;

use std::time::Duration;

use crate::ast::Map;
use crate::validator::Validator;

pub use self::http::HttpCondition;
pub use self::sleep::SleepCondition;

#[derive(Debug, Clone, PartialEq)]
pub enum WaitCondition {
    Sleep(SleepCondition),
    Http(HttpCondition),
}

impl WaitCondition {
    pub async fn wait(&self) -> Result<(), String> {
        match self {
            WaitCondition::Sleep(sleep_condition) => sleep_condition.wait().await,
            WaitCondition::Http(http_condition) => http_condition.wait().await,
        }
    }

    pub fn parse(v: &mut Validator, name: &str, params: &Map) -> Option<Self> {
        match name {
            "sleep" => SleepCondition::parse(v, params).map(WaitCondition::Sleep),
            "http" => HttpCondition::parse(v, params).map(WaitCondition::Http),
            _ => {
                v.in_field("type", |v| {
                    v.add_violation(format!("\"{}\" is not valid wait condition type", name))
                });
                None
            }
        }
    }
}

impl Default for WaitCondition {
    fn default() -> Self {
        WaitCondition::Sleep(SleepCondition {
            duration: Duration::from_secs(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::validator::testutil;

    use super::*;
    use indexmap::{indexmap, IndexMap};
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use serde_yaml::Value;

    #[rstest]
    #[case("with sleep", "sleep", indexmap! { "duration" => Value::from("1s") }, Some(WaitCondition::Sleep(SleepCondition { duration: Duration::from_secs(1) })), vec![])]
    #[case("with http", "http", indexmap! {
            "port" => Value::from(8080),
            "path" => Value::from("/health"),
        }, Some(WaitCondition::Http(HttpCondition {
            port: 8080,
            path: "/health".to_string(),
            initial_delay: Duration::from_secs(0),
            interval: Duration::from_secs(0),
            max_retry: 3,
            timeout: Duration::from_secs(1),
        })), vec![])]
    #[case("with unknown wait condition", "unknown", indexmap! {}, None, vec![(".type", "\"unknown\" is not valid wait condition type")])]
    fn parse(
        #[case] title: &str,
        #[case] name: &str,
        #[case] params: IndexMap<&str, Value>,
        #[case] expected_value: Option<WaitCondition>,
        #[case] expected_violation: Vec<(&str, &str)>,
    ) {
        let (mut v, violation) = testutil::new_validator();

        assert_eq!(
            expected_value,
            WaitCondition::parse(&mut v, name, &params.iter().map(|(k, v)| (*k, v)).collect()),
            "{}",
            title
        );

        assert_eq!(
            expected_violation
                .iter()
                .map(|(path, msg)| violation(path, msg))
                .collect::<Vec<_>>(),
            v.violations,
            "{}",
            title
        )
    }
}
