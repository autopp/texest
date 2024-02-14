use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum WaitCondition {
    Sleep(Duration),
}

impl WaitCondition {
    pub async fn wait(&self) -> Result<(), String> {
        match self {
            WaitCondition::Sleep(duration) => {
                tokio::time::sleep(*duration).await;
                Ok(())
            }
        }
    }
}

impl Default for WaitCondition {
    fn default() -> Self {
        WaitCondition::Sleep(Duration::from_secs(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod wait_condition {
        use super::*;

        mod sleep {
            use super::*;
            use pretty_assertions::assert_eq;

            #[tokio::test]
            async fn wait() {
                let given = WaitCondition::Sleep(Duration::from_millis(50));

                let before = std::time::Instant::now();
                let result = given.wait().await;
                let after = std::time::Instant::now();

                let elapsed = after - before;

                assert_eq!(Ok(()), result);
                assert!(elapsed >= Duration::from_millis(50));
            }
        }
    }
}
