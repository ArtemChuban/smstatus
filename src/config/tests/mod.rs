use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

mod discover;
mod params;
mod reads;
mod section;
mod write_modules;
mod write_separator;

static TEST_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn unique_temp_path(purpose: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEST_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smstatus-config-test-{purpose}-{}-{nanos}-{counter}.toml",
        std::process::id()
    ))
}
