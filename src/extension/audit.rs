use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

const MAX_PREVIEW_LEN: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtensionCallOutcome {
    Ok,
    Err(String),
    Denied,
    // Future: CheckDenied for #60 (extension-side permission denial)
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ExtensionCallRecord {
    pub(crate) at: Instant,
    pub(crate) module_kind: Option<String>,
    pub(crate) extension: String,
    pub(crate) method: String,
    pub(crate) payload_preview: String,
    pub(crate) outcome: ExtensionCallOutcome,
}

pub(crate) struct ExtensionCallAudit {
    records: Mutex<VecDeque<ExtensionCallRecord>>,
}

impl ExtensionCallAudit {
    pub(crate) const MAX_RECORDS: usize = 64;

    pub(crate) fn new() -> Self {
        Self {
            records: Mutex::new(VecDeque::new()),
        }
    }

    pub(crate) fn push(&self, record: ExtensionCallRecord) {
        let mut records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        records.push_back(record);
        while records.len() > Self::MAX_RECORDS {
            records.pop_front();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn recent(&self, limit: usize) -> Vec<ExtensionCallRecord> {
        let records = self.records.lock().unwrap_or_else(PoisonError::into_inner);
        let start = records.len().saturating_sub(limit);
        records.range(start..).cloned().collect()
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization" | "cookie" | "token" | "secret" | "password"
    ) || lower.contains("token")
        || lower.contains("apikey")
        || lower.contains("api_key")
        || lower.contains("api-key")
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn redact_url(s: &str) -> String {
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let host_end = rest.find('/').unwrap_or(rest.len());
    let prefix_len = s.len() - rest.len() + host_end;
    format!("{}<redacted>", &s[..prefix_len])
}

fn looks_like_path(s: &str) -> bool {
    s.starts_with('/') || s.starts_with('~') || s.contains("/home/")
}

fn redact_path(s: &str) -> String {
    let trimmed = s.trim_start_matches('~');
    let trimmed = trimmed.trim_start_matches('/');
    let first_end = trimmed.find('/').map(|i| i + 1).unwrap_or(trimmed.len());
    let prefix_len = s.len() - trimmed.len() + first_end;
    if prefix_len >= s.len() {
        s.to_string()
    } else {
        format!("{}<redacted>", &s[..prefix_len])
    }
}

fn is_path_like_method(method: &str) -> bool {
    matches!(method, "read" | "write" | "list" | "stat" | "exists" | "fs")
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn redact_string_value(s: &str, method: &str) -> String {
    if looks_like_url(s) {
        redact_url(s)
    } else if is_path_like_method(method) || looks_like_path(s) {
        redact_path(s)
    } else {
        s.to_string()
    }
}

fn redact_json_value(value: &mut serde_json::Value, method: &str) {
    match value {
        serde_json::Value::Object(obj) => {
            for (key, val) in obj.iter_mut() {
                if is_sensitive_key(key) {
                    *val = serde_json::Value::String("<redacted>".to_string());
                } else {
                    redact_json_value(val, method);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_json_value(item, method);
            }
        }
        serde_json::Value::String(s) => {
            *s = redact_string_value(s, method);
        }
        _ => {}
    }
}

pub(crate) fn redact_payload(method: &str, payload: &str) -> String {
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(payload) {
        redact_json_value(&mut value, method);
        return truncate(&value.to_string(), MAX_PREVIEW_LEN);
    }

    if looks_like_url(payload) {
        return truncate(&redact_url(payload), MAX_PREVIEW_LEN);
    }

    if is_path_like_method(method) || looks_like_path(payload) {
        return truncate(&redact_path(payload), MAX_PREVIEW_LEN);
    }

    truncate(payload, MAX_PREVIEW_LEN)
}

pub(crate) fn redact_error_message(message: &str) -> String {
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(message) {
        redact_json_value(&mut value, "");
        return truncate(&value.to_string(), MAX_PREVIEW_LEN);
    }

    if looks_like_url(message) {
        return truncate(&redact_url(message), MAX_PREVIEW_LEN);
    }

    if looks_like_path(message) {
        return truncate(&redact_path(message), MAX_PREVIEW_LEN);
    }

    if let Some(index) = message.find("/home/") {
        let prefix = redact_path(&message[index..]);
        return truncate(&format!("{}{prefix}", &message[..index]), MAX_PREVIEW_LEN);
    }

    if let Some(index) = message.find("http://").or_else(|| message.find("https://")) {
        let tail = redact_url(&message[index..]);
        return truncate(&format!("{}{tail}", &message[..index]), MAX_PREVIEW_LEN);
    }

    truncate(message, MAX_PREVIEW_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(extension: &str) -> ExtensionCallRecord {
        ExtensionCallRecord {
            at: Instant::now(),
            module_kind: None,
            extension: extension.to_string(),
            method: "ping".to_string(),
            payload_preview: String::new(),
            outcome: ExtensionCallOutcome::Ok,
        }
    }

    #[test]
    fn ring_evicts_oldest_when_full() {
        let audit = ExtensionCallAudit::new();
        for i in 0..ExtensionCallAudit::MAX_RECORDS {
            let mut r = record("echo");
            r.extension = format!("ext-{i}");
            audit.push(r);
        }
        assert_eq!(audit.recent(ExtensionCallAudit::MAX_RECORDS).len(), 64);

        let mut newest = record("echo");
        newest.extension = "ext-new".to_string();
        audit.push(newest);

        let recent = audit.recent(ExtensionCallAudit::MAX_RECORDS);
        assert_eq!(recent.len(), 64);
        assert!(!recent.iter().any(|r| r.extension == "ext-0"));
        assert!(recent.iter().any(|r| r.extension == "ext-new"));
    }

    #[test]
    fn recent_respects_limit() {
        let audit = ExtensionCallAudit::new();
        for i in 0..10 {
            let mut r = record("echo");
            r.extension = format!("ext-{i}");
            audit.push(r);
        }
        assert_eq!(audit.recent(3).len(), 3);
        let last_three = audit.recent(3);
        assert_eq!(last_three[0].extension, "ext-7");
        assert_eq!(last_three[2].extension, "ext-9");
    }

    #[test]
    fn redact_json_sensitive_headers() {
        let payload = r#"{"Authorization":"Bearer secret123","Cookie":"session=abc"}"#;
        let redacted = redact_payload("get", payload);
        assert!(!redacted.contains("secret123"));
        assert!(!redacted.contains("session=abc"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn redact_json_sensitive_headers_in_array() {
        let payload = r#"[{"Authorization":"Bearer secret123"}]"#;
        let redacted = redact_payload("get", payload);
        assert!(!redacted.contains("secret123"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn redact_json_path_field_value() {
        let payload = r#"{"path":"/home/user/secrets/key.pem"}"#;
        let redacted = redact_payload("get", payload);
        assert!(!redacted.contains("secrets"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn redact_json_access_token_field() {
        let payload = r#"{"access_token":"abc123","refresh_token":"xyz789"}"#;
        let redacted = redact_payload("get", payload);
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("xyz789"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn redact_long_home_path() {
        let payload = "/home/user/secret/projects/smstatus/config.toml";
        let redacted = redact_payload("read", payload);
        assert!(!redacted.contains("secret/projects"));
        assert!(redacted.contains("<redacted>"));
        assert!(redacted.starts_with("/home/"));
    }

    #[test]
    fn redact_url_keeps_scheme_and_host() {
        let payload = "https://api.example.com/v1/secret/data?token=abc";
        let redacted = redact_payload("get", payload);
        assert!(redacted.starts_with("https://api.example.com"));
        assert!(!redacted.contains("secret"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn redact_error_message_strips_embedded_home_path() {
        let message = "extension call failed: open /home/user/secrets/key.pem: denied";
        let redacted = redact_error_message(message);
        assert!(!redacted.contains("secrets"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn truncate_plain_text() {
        let payload = "a".repeat(100);
        let redacted = redact_payload("ping", &payload);
        assert!(redacted.ends_with("..."));
        assert!(redacted.len() <= MAX_PREVIEW_LEN);
    }
}
