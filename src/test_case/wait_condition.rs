mod sleep;

use std::time::Duration;

pub use self::sleep::SleepCondition;

#[derive(Debug, Clone, PartialEq)]
pub enum WaitCondition {
    Sleep(SleepCondition),
}

impl WaitCondition {
    pub async fn wait(&self) -> Result<(), String> {
        match self {
            WaitCondition::Sleep(sleep_condition) => sleep_condition.wait().await,
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
