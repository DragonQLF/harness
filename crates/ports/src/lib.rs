use std::fmt;

use harness_domain::Event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: u64,
    pub event: Event,
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Serde(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "store io error: {e}"),
            StoreError::Serde(msg) => write!(f, "store serialization error: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub trait StorePort: Send + Sync {
    fn append_event(&self, e: &Event) -> Result<StoredEvent, StoreError>;
    fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError>;
}

pub trait ClockPort: Send + Sync {
    fn now_millis(&self) -> u64;
}
