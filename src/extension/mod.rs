mod audit;
mod bus;
mod client;
mod registry;
mod status;

pub(crate) use audit::{
    ExtensionCallAudit, ExtensionCallOutcome, ExtensionCallRecord, redact_error_message,
    redact_payload,
};
pub(crate) use bus::ExtensionEventBus;
pub(crate) use registry::{ExtensionLiveState, ExtensionRegistry, is_safe_extension_name};
pub(crate) use status::{cmd_extension_status, encode_status_snapshot};

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
