use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct SleepCondition {
    pub duration: Duration,
}

impl SleepCondition {
    pub async fn wait(&self) -> Result<(), String> {
        tokio::time::sleep(self.duration).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod sleep_condition {
        use super::*;
        use pretty_assertions::assert_eq;

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
    }
}
