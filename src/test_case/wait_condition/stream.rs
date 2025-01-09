use duration_str::HumanFormat;
use std::time::Duration;

use regex::Regex;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Child,
};

use crate::{ast::Map, validator::Validator};

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
pub struct StdoutCondition {
    pub pattern: Regex,
    pub timeout: Duration,
}

#[cfg(test)]
impl PartialEq for StdoutCondition {
    fn eq(&self, other: &Self) -> bool {
        self.timeout == other.timeout && self.pattern.as_str() == other.pattern.as_str()
    }
}

impl StdoutCondition {
    pub async fn wait(&self, cmd: &mut Child) -> Result<(), String> {
        let stdout = cmd.stdout.as_mut().unwrap();
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let result = tokio::time::timeout(self.timeout, async {
            while let Some(line) = lines.next_line().await.map_err(|err| err.to_string())? {
                if self.pattern.is_match(&line) {
                    return Ok(());
                }
            }

            Err(format!("stdout never output \"{}\"", self.pattern.as_str()))
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(err) => err,
            Err(_) => Err(format!(
                "stdout did not output \"{}\" in {}",
                self.pattern.as_str(),
                self.timeout.human_format()
            )),
        }
    }

    pub fn parse(v: &mut Validator, params: &Map) -> Option<Self> {
        let pattern = v.must_have_string(params, "pattern").and_then(|pattern| {
            Regex::new(&pattern)
                .inspect_err(|_| {
                    v.in_field("pattern", |v| {
                        v.add_violation("should be valid regular expression pattern")
                    });
                })
                .ok()
        });

        let timeout = {
            let err_count = v.violations.len();
            v.may_have_duration(params, "timeout").or_else(|| {
                if err_count == v.violations.len() {
                    Some(Duration::from_secs(3))
                } else {
                    None
                }
            })
        };

        match (pattern, timeout) {
            (Some(pattern), Some(timeout)) => Some(Self { pattern, timeout }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod stdout_condition {
        use indexmap::indexmap;
        use once_cell::sync::Lazy;
        use pretty_assertions::assert_eq;
        use rstest::rstest;
        use saphyr::Yaml;

        use super::*;

        #[rstest]
        #[tokio::test]
        #[case("when matched, returns Ok", Duration::from_secs(3), "echo hello; echo world; echo goodbye", Ok(()))]
        #[tokio::test]
        #[case("when timeout, returns Err", Duration::from_millis(10), "yes", Err("stdout did not output \"wo.ld\" in 10ms".to_string()))]
        #[tokio::test]
        #[case("when never matched, returns Err", Duration::from_secs(3), "true", Err("stdout never output \"wo.ld\"".to_string()))]
        async fn wait(
            #[case] title: &'static str,
            #[case] timeout: Duration,
            #[case] command: &'static str,
            #[case] expected: Result<(), String>,
        ) {
            let given = StdoutCondition {
                pattern: Regex::new("wo.ld").unwrap(),
                timeout,
            };

            let mut cmd = tokio::process::Command::new("bash")
                .arg("-c")
                .arg(command)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();

            let actual = given.wait(&mut cmd).await;

            assert_eq!(actual, expected, "{}", title);
        }

        static VALID_PATTERN: Lazy<Yaml> = Lazy::new(|| Yaml::String("wo.ld".to_string()));
        static INVALID_PATTERN: Lazy<Yaml> = Lazy::new(|| Yaml::String("(wo.ld".to_string()));
        static VALID_DURATION: Lazy<Yaml> = Lazy::new(|| Yaml::String("10s".to_string()));
        static INVALID_DURATION: Lazy<Yaml> = Lazy::new(|| Yaml::Boolean(true));

        #[rstest]
        #[case("with valid params", indexmap! { "pattern" => &*VALID_PATTERN }, Some(StdoutCondition { pattern: Regex::new("wo.ld").unwrap(), timeout: Duration::from_secs(3) }), vec![])]
        #[case("with valid full params", indexmap! { "pattern" => &*VALID_PATTERN, "timeout" => &*VALID_DURATION }, Some(StdoutCondition { pattern: Regex::new("wo.ld").unwrap(), timeout: Duration::from_secs(10) }), vec![])]
        #[case("without pattern", indexmap! {}, None, vec![("", "should have .pattern as string")])]
        #[case("with invalid pattern", indexmap! { "pattern" => &*INVALID_PATTERN }, None, vec![(".pattern", "should be valid regular expression pattern")])]
        #[case("with invalid timeout", indexmap! { "pattern" => &*VALID_PATTERN, "timeout" => &*INVALID_DURATION }, None, vec![(".timeout", "should be duration, but is bool")])]
        fn parse(
            #[case] title: &'static str,
            #[case] params: Map,
            #[case] expected_value: Option<StdoutCondition>,
            #[case] expected_violation: Vec<(&str, &str)>,
        ) {
            let (mut v, violation) = crate::validator::testutil::new_validator();

            let actual = StdoutCondition::parse(&mut v, &params);

            assert_eq!(expected_value, actual, "{}", title);
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
