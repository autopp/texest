mod http;
mod sleep;

use std::time::Duration;

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
}

impl Default for WaitCondition {
    fn default() -> Self {
        WaitCondition::Sleep(SleepCondition {
            duration: Duration::from_secs(0),
        })
    }
}
