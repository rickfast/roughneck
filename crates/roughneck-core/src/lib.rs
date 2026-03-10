mod backend;
mod config;
mod error;
mod types;

pub use backend::{FileSystemBackend, MemoryBackend};
pub use config::*;
pub use error::{Result, RoughneckError};
pub use types::*;

use std::time::{SystemTime, UNIX_EPOCH};

/// Returns the current UNIX timestamp in milliseconds.
#[must_use]
pub fn now_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}
