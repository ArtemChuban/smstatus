pub mod protocol;

mod client;
mod registry;

pub(crate) use registry::ExtensionRegistry;

#[cfg(test)]
pub(crate) fn test_temp_dir(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "smstatus-extension-{label}-test-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
