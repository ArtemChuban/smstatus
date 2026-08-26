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

pub(super) fn install_test_log() {
    let path = unique_temp_path("action-log").with_extension("log");
    crate::logging::set_path_for_test(path);
}

pub(super) fn action_log() -> Vec<String> {
    crate::logging::logged_messages()
}

pub(super) fn action_log_with_daemon_notify(messages: &[&str]) -> Vec<String> {
    std::iter::once("smstatus is not running; config saved but bar not updated")
        .chain(messages.iter().copied())
        .map(str::to_string)
        .collect()
}

pub(super) fn clear_action_log() {
    crate::logging::clear_for_test();
}

mod daemon;
mod help;
mod install;
mod logs;
mod modules;
mod params;
mod presets;
mod reload;
mod separator;
mod text;
