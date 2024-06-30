use std::time::Duration;

use reqwest::Client;

use crate::{ast::Map, validator::Validator};

#[derive(Debug, Clone, PartialEq)]
pub struct HttpCondition {
    pub port: u16,
    pub path: String,
    pub initial_delay: Duration,
    pub interval: Duration,
    pub max_retry: u64,
    pub timeout: Duration,
}

impl HttpCondition {
    pub async fn wait(&self) -> Result<(), String> {
        tokio::time::sleep(self.initial_delay).await;

        let client = Client::builder().timeout(self.timeout).build().unwrap();
        let url = format!("http://localhost:{}{}", self.port, self.path);

        let check = || async {
            client
                .get(&url)
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
        };

        if check().await {
            return Ok(());
        }

        for _ in 0..self.max_retry {
            tokio::time::sleep(self.interval).await;

            if check().await {
                return Ok(());
            }
        }

        Err(format!("HTTP endpoint {} is not ready", self.path))
    }

    pub fn parse(v: &mut Validator, params: &Map) -> Option<Self> {
        let prev_vioaions_count = v.violations.len();
        let port: u16 = v
            .must_have_uint(params, "port")
            .and_then(|port64| {
                v.in_field("port", |v| {
                    TryFrom::try_from(port64)
                        .map_err(|_| {
                            v.add_violation("should be in range of u16");
                        })
                        .ok()
                })
            })
            .unwrap_or_default();
        let path = v.must_have_string(params, "path").unwrap_or_default();
        let initial_delay = v
            .may_have_duration(params, "initial_delay")
            .unwrap_or(Duration::from_secs(0));
        let interval = v
            .may_have_duration(params, "interval")
            .unwrap_or(Duration::from_secs(0));
        let max_retry = v.may_have_uint(params, "max_retry").unwrap_or(3);
        let timeout = v
            .may_have_duration(params, "timeout")
            .unwrap_or(Duration::from_secs(1));

        if prev_vioaions_count == v.violations.len() {
            Some(HttpCondition {
                port,
                path,
                initial_delay,
                interval,
                max_retry,
                timeout,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod http_condition {
        use indexmap::{indexmap, IndexMap};
        use pretty_assertions::assert_eq;
        use rstest::rstest;
        use saphyr::Yaml;

        use crate::validator::testutil;

        use super::*;

        mod wait {
            use httptest::{matchers::*, responders::*, Expectation, ServerPool};
            use rstest::*;

            use super::*;
            use pretty_assertions::assert_eq;

            static SERVER_POOL: ServerPool = ServerPool::new(10);
            const PATH: &str = "/health";

            #[rstest]
            #[tokio::test]
            #[case(status_code(200), Ok(()))]
            #[tokio::test]
            #[case(status_code(500), Err("HTTP endpoint /health is not ready".to_string()))]
            #[tokio::test]
            #[case(cycle![
                status_code(500),
                status_code(500),
                status_code(500),
                status_code(200),
                ], Ok(()))]
            #[tokio::test]
            #[case(cycle![
                status_code(500),
                status_code(500),
                status_code(500),
                status_code(500),
                status_code(200),
                ], Err("HTTP endpoint /health is not ready".to_string()))]
            #[tokio::test]
            #[case(delay_and_then(Duration::from_secs(1), status_code(200)), Err("HTTP endpoint /health is not ready".to_string()))]
            async fn success_cases<R: Responder + 'static>(
                #[case] responder: R,
                #[case] expected: Result<(), String>,
            ) {
                let server = SERVER_POOL.get_server();
                server.expect(
                    Expectation::matching(request::method_path("GET", PATH))
                        .times(0..)
                        .respond_with(responder),
                );
                let port = server.addr().port();
                let condition = HttpCondition {
                    port,
                    path: PATH.to_string(),
                    initial_delay: Duration::from_secs(0),
                    interval: Duration::from_millis(50),
                    max_retry: 3,
                    timeout: Duration::from_millis(50),
                };

                assert_eq!(expected, condition.wait().await);
            }
        }

        #[rstest]
        #[case("with full valid params", indexmap! {
            "port" => Yaml::Integer(8080),
            "path" => Yaml::String("/health".to_string()),
            "initial_delay" => Yaml::String("2s".to_string()),
            "interval" => Yaml::String("3s".to_string()),
            "max_retry" => Yaml::Integer(5),
            "timeout" => Yaml::String("20s".to_string()),
        }, Some(HttpCondition {
            port: 8080,
            path: "/health".to_string(),
            initial_delay: Duration::from_secs(2),
            interval: Duration::from_secs(3),
            max_retry: 5,
            timeout: Duration::from_secs(20),
        }), vec![])]
        #[case("with minimum valid params", indexmap! {
            "port" => Yaml::Integer(8080),
            "path" => Yaml::String("/health".to_string()),
        }, Some(HttpCondition {
            port: 8080,
            path: "/health".to_string(),
            initial_delay: Duration::from_secs(0),
            interval: Duration::from_secs(0),
            max_retry: 3,
            timeout: Duration::from_secs(1),
        }), vec![])]
        #[case("with missing reqired params", indexmap! {}, None, vec![("", "should have .port as uint"), ("", "should have .path as string")])]
        #[case("with invalid params", indexmap! {
            "port" => Yaml::Integer(65536),
            "path" => Yaml::Boolean(true),
            "initial_delay" => Yaml::Boolean(true),
            "interval" => Yaml::Boolean(true),
            "max_retry" => Yaml::Boolean(true),
            "timeout" => Yaml::Boolean(true),
        }, None, vec![
            (".port", "should be in range of u16"),
            (".path", "should be string, but is bool"),
            (".initial_delay", "should be duration, but is bool"),
            (".interval", "should be duration, but is bool"),
            (".max_retry", "should be uint, but is bool"),
            (".timeout", "should be duration, but is bool"),
        ])]
        fn parse(
            #[case] title: &str,
            #[case] params: IndexMap<&str, Yaml>,
            #[case] expected_value: Option<HttpCondition>,
            #[case] expected_violation: Vec<(&str, &str)>,
        ) {
            let (mut v, violation) = testutil::new_validator();

            assert_eq!(
                expected_value,
                HttpCondition::parse(
                    &mut v,
                    &params
                        .iter()
                        .map(|(k, v)| (*k, v))
                        .collect::<IndexMap<_, _>>()
                ),
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
