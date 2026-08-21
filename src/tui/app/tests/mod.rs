use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

static TEST_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn unique_temp_path(purpose: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEST_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smstatus-tui-app-test-{purpose}-{}-{nanos}-{counter}.toml",
        std::process::id()
    ))
}

mod daemon;
mod help;
mod modules;
mod params;
mod reload;
mod separator;
mod text;
