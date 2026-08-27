use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{
    ExtensionCallAudit, ExtensionCallOutcome, ExtensionCallRecord, ExtensionLiveState,
    ExtensionRegistry, redact_error_message,
};
use crate::config::BarConfig;
use crate::install;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ExtensionStatusSnapshot {
    pub daemon_pid: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
    pub extensions: Vec<ExtensionStatusRow>,
    pub recent_calls: Vec<SerializableCallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ExtensionStatusRow {
    pub name: String,
    pub installed: bool,
    pub required_by: Vec<String>,
    pub live: ExtensionLiveState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SerializableCallRecord {
    pub at_unix_ms: i64,
    pub module_kind: Option<String>,
    pub extension: String,
    pub method: String,
    pub payload_preview: String,
    pub outcome: SerializableCallOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SerializableCallOutcome {
    Ok,
    Err { message: String },
    Denied,
}

pub(crate) fn collect_required_extensions(
    modules_dir: &Path,
    module_entries: &[String],
) -> HashMap<String, Vec<String>> {
    let mut by_extension: HashMap<String, Vec<String>> = HashMap::new();
    for entry in module_entries {
        let (kind, _) = BarConfig::split_module_entry(entry);
        let manifest = match crate::manifest::read_module_manifest(modules_dir, kind) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        for req in manifest.required_extensions {
            by_extension
                .entry(req.name)
                .or_default()
                .push(kind.to_string());
        }
    }
    for kinds in by_extension.values_mut() {
        kinds.sort_unstable();
        kinds.dedup();
    }
    by_extension
}

fn system_time_to_unix_ms(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn serialize_outcome(outcome: &ExtensionCallOutcome) -> SerializableCallOutcome {
    match outcome {
        ExtensionCallOutcome::Ok => SerializableCallOutcome::Ok,
        ExtensionCallOutcome::Err(message) => SerializableCallOutcome::Err {
            message: redact_error_message(message),
        },
        ExtensionCallOutcome::Denied => SerializableCallOutcome::Denied,
    }
}

fn serialize_call(record: &ExtensionCallRecord) -> SerializableCallRecord {
    SerializableCallRecord {
        at_unix_ms: system_time_to_unix_ms(record.at),
        module_kind: record.module_kind.clone(),
        extension: record.extension.clone(),
        method: record.method.clone(),
        payload_preview: record.payload_preview.clone(),
        outcome: serialize_outcome(&record.outcome),
    }
}

pub(crate) fn build_snapshot(
    registry: &ExtensionRegistry,
    audit: &ExtensionCallAudit,
    extensions_dir: &Path,
    modules_dir: &Path,
    config: &BarConfig,
    daemon_pid: Option<i32>,
    limit: usize,
) -> ExtensionStatusSnapshot {
    let module_entries = config.module_names().unwrap_or_default();
    let required = collect_required_extensions(modules_dir, &module_entries);
    let installed = install::list_extensions_in(extensions_dir).unwrap_or_default();

    let mut names = HashSet::new();
    names.extend(installed.iter().cloned());
    names.extend(required.keys().cloned());
    let mut sorted_names: Vec<_> = names.into_iter().collect();
    sorted_names.sort_unstable();

    let extensions = sorted_names
        .into_iter()
        .map(|name| ExtensionStatusRow {
            installed: registry.is_installed(&name),
            required_by: required.get(&name).cloned().unwrap_or_default(),
            live: registry.live_state(&name),
            name,
        })
        .collect();

    let recent_calls = audit
        .recent(limit.min(ExtensionCallAudit::MAX_RECORDS))
        .iter()
        .map(serialize_call)
        .collect();

    ExtensionStatusSnapshot {
        daemon_pid,
        err: None,
        extensions,
        recent_calls,
    }
}

pub(crate) fn encode_status_snapshot(
    registry: &ExtensionRegistry,
    audit: &ExtensionCallAudit,
    extensions_dir: &Path,
    modules_dir: &Path,
    config: &BarConfig,
    daemon_pid: Option<i32>,
    limit: usize,
) -> String {
    let snapshot = build_snapshot(
        registry,
        audit,
        extensions_dir,
        modules_dir,
        config,
        daemon_pid,
        limit,
    );
    serde_json::to_string(&snapshot).unwrap_or_else(|err| {
        serde_json::to_string(&ExtensionStatusSnapshot {
            daemon_pid,
            err: Some(format!("failed to encode status snapshot: {err}")),
            extensions: Vec::new(),
            recent_calls: Vec::new(),
        })
        .unwrap_or_else(|_| "{\"err\":\"failed to encode status snapshot\"}".to_string())
    })
}

fn format_live_state(live: &ExtensionLiveState) -> String {
    match live {
        ExtensionLiveState::Idle => "idle".to_string(),
        ExtensionLiveState::Live => "live".to_string(),
        ExtensionLiveState::BackingOff { retry_in_secs } => {
            format!("backing off ({retry_in_secs}s)")
        }
    }
}

pub(crate) fn format_snapshot_lines(
    snapshot: &ExtensionStatusSnapshot,
    limit: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(err) = &snapshot.err {
        lines.push(format!("error: {err}"));
        return lines;
    }

    match snapshot.daemon_pid {
        Some(pid) => lines.push(format!("daemon running (pid {pid})")),
        None => lines.push("daemon stopped".to_string()),
    }

    lines.push(String::new());
    lines.push("extensions:".to_string());
    if snapshot.extensions.is_empty() {
        lines.push("  (none)".to_string());
    } else {
        for row in &snapshot.extensions {
            let install_state = if row.installed {
                "installed"
            } else {
                "not installed"
            };
            let mut line = format!(
                "  {} ({}, {})",
                row.name,
                install_state,
                format_live_state(&row.live)
            );
            if !row.required_by.is_empty() {
                line.push_str(&format!("; required by: {}", row.required_by.join(", ")));
            }
            lines.push(line);
        }
    }

    lines.push(String::new());
    if snapshot.daemon_pid.is_none() {
        lines.push("recent calls: unavailable while daemon is stopped".to_string());
        return lines;
    }

    lines.push("recent calls:".to_string());
    if snapshot.recent_calls.is_empty() {
        lines.push("  (none)".to_string());
    } else {
        let start = snapshot.recent_calls.len().saturating_sub(limit);
        for call in &snapshot.recent_calls[start..] {
            let module = call.module_kind.as_deref().unwrap_or("-");
            let outcome = match &call.outcome {
                SerializableCallOutcome::Ok => "ok".to_string(),
                SerializableCallOutcome::Err { message } => {
                    format!("err: {}", redact_error_message(message))
                }
                SerializableCallOutcome::Denied => "denied".to_string(),
            };
            lines.push(format!(
                "  [{module}] {}::{} {} preview={} at={}",
                call.extension, call.method, outcome, call.payload_preview, call.at_unix_ms
            ));
        }
    }

    lines
}

pub(crate) fn cmd_extension_status(limit: usize) -> crate::error::Result<Vec<String>> {
    use crate::config::{BarConfig, active_config_path, default_config_dir};
    use crate::daemon::{self, DaemonStatus};
    use crate::lock;

    let config_dir = default_config_dir()?;
    let config_path = active_config_path(&config_dir)?;
    let config = BarConfig::load(&config_path)?;
    let modules_dir = config_dir.join("modules");
    let extensions_dir = config_dir.join("extensions");

    let snapshot = match daemon::status()? {
        DaemonStatus::Stopped => {
            let registry = ExtensionRegistry::new(
                extensions_dir.clone(),
                lock::lock_dir()?.join("extensions"),
            );
            let audit = ExtensionCallAudit::new();
            build_snapshot(
                &registry,
                &audit,
                &extensions_dir,
                &modules_dir,
                &config,
                None,
                limit,
            )
        }
        DaemonStatus::Running { .. } | DaemonStatus::RunningPidUnknown => {
            let json = crate::control::query_extension_status()?;
            serde_json::from_str(&json)
                .map_err(|err| format!("failed to parse extension status from daemon: {err}"))?
        }
    };

    Ok(format_snapshot_lines(&snapshot, limit))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::extension::ExtensionRegistry;
    use crate::extension::{ExtensionCallAudit, ExtensionCallOutcome, ExtensionCallRecord};

    fn echo_extension_path() -> PathBuf {
        std::env::var("CARGO_BIN_EXE_echo")
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .unwrap_or_else(|| {
                let mut dir = std::env::current_exe().unwrap();
                dir.pop();
                if dir.ends_with("deps") {
                    dir.pop();
                }
                dir.join("echo")
            })
    }

    fn install_echo(extensions_dir: &Path) {
        let pkg = extensions_dir.join("echo");
        std::fs::create_dir_all(&pkg).unwrap();
        symlink(echo_extension_path(), pkg.join("extension")).unwrap();
        std::fs::write(
            pkg.join("manifest.toml"),
            "name = \"echo\"\nversion = \"0.1.0\"\nauthor = \"test\"\nextensions-api = { major = 0, minor = 1 }\n",
        )
        .unwrap();
    }

    #[test]
    fn build_snapshot_round_trips_through_json() {
        let base = crate::extension::test_temp_dir("status-snapshot");
        let extensions_dir = base.join("extensions");
        let modules_dir = base.join("modules");
        let socket_dir = base.join("sockets");
        std::fs::create_dir_all(&modules_dir).unwrap();
        install_echo(&extensions_dir);

        let cpu_pkg = modules_dir.join("cpu");
        std::fs::create_dir_all(&cpu_pkg).unwrap();
        std::fs::write(cpu_pkg.join("module.wasm"), b"\0asm").unwrap();
        std::fs::write(
            cpu_pkg.join("manifest.toml"),
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 0, minor = 1 }\nrequired_extensions = [{ name = \"echo\", version = { major = 0, minor = 1 } }]\n",
        )
        .unwrap();

        let config_path = base.join("preset.toml");
        std::fs::write(&config_path, "modules = [\"cpu\"]\n").unwrap();
        let config = BarConfig::load(&config_path).unwrap();

        let registry = ExtensionRegistry::new(extensions_dir, socket_dir);
        registry.call("echo", "ping", "hello").unwrap();

        let audit = ExtensionCallAudit::new();
        audit.push(ExtensionCallRecord {
            at: SystemTime::now(),
            module_kind: Some("cpu".to_string()),
            extension: "echo".to_string(),
            method: "ping".to_string(),
            payload_preview: "hello".to_string(),
            outcome: ExtensionCallOutcome::Ok,
        });

        let snapshot = build_snapshot(
            &registry,
            &audit,
            &base.join("extensions"),
            &modules_dir,
            &config,
            Some(4242),
            20,
        );
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: ExtensionStatusSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.daemon_pid, Some(4242));
        assert!(
            decoded
                .extensions
                .iter()
                .any(|row| row.name == "echo" && row.live == ExtensionLiveState::Live)
        );
        assert_eq!(decoded.recent_calls.len(), 1);
        assert_eq!(decoded.recent_calls[0].module_kind.as_deref(), Some("cpu"));
    }

    #[test]
    fn collect_required_extensions_inverts_module_manifests() {
        let base = crate::extension::test_temp_dir("status-required");
        let modules_dir = base.join("modules");
        let cpu_pkg = modules_dir.join("cpu");
        std::fs::create_dir_all(&cpu_pkg).unwrap();
        std::fs::write(cpu_pkg.join("module.wasm"), b"\0asm").unwrap();
        std::fs::write(
            cpu_pkg.join("manifest.toml"),
            "name = \"cpu\"\ndisplay_name = \"CPU\"\nversion = \"0.1.0\"\nauthor = \"test\"\nmodules-api = { major = 0, minor = 1 }\nrequired_extensions = [{ name = \"fs\", version = { major = 0, minor = 1 } }]\n",
        )
        .unwrap();

        let required = collect_required_extensions(&modules_dir, &["cpu".to_string()]);
        assert_eq!(required.get("fs"), Some(&vec!["cpu".to_string()]));
    }

    #[test]
    fn format_snapshot_lines_renders_offline_and_live_sections() {
        let snapshot = ExtensionStatusSnapshot {
            daemon_pid: None,
            err: None,
            extensions: vec![ExtensionStatusRow {
                name: "fs".to_string(),
                installed: true,
                required_by: vec!["cpu".to_string()],
                live: ExtensionLiveState::Idle,
            }],
            recent_calls: Vec::new(),
        };
        let offline = format_snapshot_lines(&snapshot, 20);
        assert!(offline.iter().any(|line| line.contains("daemon stopped")));
        assert!(
            offline
                .iter()
                .any(|line| line.contains("unavailable while daemon is stopped"))
        );

        let live = ExtensionStatusSnapshot {
            daemon_pid: Some(42),
            err: None,
            extensions: vec![ExtensionStatusRow {
                name: "echo".to_string(),
                installed: true,
                required_by: Vec::new(),
                live: ExtensionLiveState::Live,
            }],
            recent_calls: vec![
                SerializableCallRecord {
                    at_unix_ms: 1_700_000_000_000,
                    module_kind: Some("cpu".to_string()),
                    extension: "echo".to_string(),
                    method: "ping".to_string(),
                    payload_preview: "hello".to_string(),
                    outcome: SerializableCallOutcome::Ok,
                },
                SerializableCallRecord {
                    at_unix_ms: 1_700_000_000_001,
                    module_kind: None,
                    extension: "fs".to_string(),
                    method: "read".to_string(),
                    payload_preview: "/home/<redacted>".to_string(),
                    outcome: SerializableCallOutcome::Denied,
                },
            ],
        };
        let lines = format_snapshot_lines(&live, 1);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("daemon running (pid 42)"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("echo") && line.contains("live"))
        );
        let recent_lines: Vec<_> = lines
            .iter()
            .filter(|line| line.starts_with("  [") && line.contains("::"))
            .collect();
        assert_eq!(recent_lines.len(), 1);
        assert!(recent_lines[0].contains("fs::read"));
    }
}
