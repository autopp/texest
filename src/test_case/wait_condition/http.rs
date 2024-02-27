use std::time::Duration;

use reqwest::Client;

#[derive(Debug, Clone, PartialEq)]
pub struct HttpCondition {
    pub port: u16,
    pub path: String,
    pub initial_delay: Duration,
    pub interval: Duration,
    pub max_retry: u32,
    pub timeout: Duration,
}

impl HttpCondition {
    pub async fn wait(&self) -> Result<(), String> {
        tokio::time::sleep(self.initial_delay).await;

        let client = Client::builder().timeout(self.timeout).build().unwrap();
        for _ in 0..self.max_retry {
            let url = format!("http://localhost:{}{}", self.port, self.path);
            let result = client.get(&url).send().await;

            if let Ok(response) = result {
                if response.status().is_success() {
                    return Ok(());
                }
            }
            tokio::time::sleep(self.interval).await;
        }

        Err(format!("HTTP endpoint {} is not ready", self.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod http_condition {
        use super::*;

        mod wait {
            use httptest::{matchers::*, responders::*, Expectation, ServerPool};
            use rstest::*;

            use super::*;

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
                status_code(200),
                ], Ok(()))]
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
    }
}
