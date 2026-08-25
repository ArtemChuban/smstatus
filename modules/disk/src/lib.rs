wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, HostApiVersion, Metadata, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_DEVICE: &str = "/dev/sda1";
const DEFAULT_FORMAT: &str = "{used}/{total} used, {free} free";
const REQUIRED_HOST_API: (u32, u32, u32) = (1, 0, 0);

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    device: Option<String>,
    format: Option<String>,
}

thread_local! {
    static DEVICE: RefCell<String> = RefCell::new(DEFAULT_DEVICE.to_string());
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
}

mod logic {
    use super::Config;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct DiskUsageJson {
        total_bytes: u64,
        used_bytes: u64,
        free_bytes: u64,
    }

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn parse_disk_usage_json(json: &str) -> Result<(u64, u64, u64), String> {
        serde_json::from_str::<DiskUsageJson>(json)
            .map(|u| (u.total_bytes, u.used_bytes, u.free_bytes))
            .map_err(|e| format!("malformed disk usage: {e}"))
    }

    pub fn format_disk(format: &str, total_bytes: u64, used_bytes: u64, free_bytes: u64) -> String {
        fmt_common::format_usage(format, total_bytes, used_bytes, free_bytes)
    }

    pub fn format_error(err: &str) -> String {
        format!("disk error: {err}")
    }
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Some(parsed) = logic::parse_config(&config) {
            let device = parsed.device.unwrap_or_else(|| DEFAULT_DEVICE.to_string());
            let format = parsed.format.unwrap_or_else(|| DEFAULT_FORMAT.to_string());
            DEVICE.with(|d| *d.borrow_mut() = device);
            FORMAT.with(|f| *f.borrow_mut() = format);
        }
    }

    fn update() -> Output {
        let device = DEVICE.with(|d| d.borrow().clone());
        let text = match host::call_extension("disk", "usage", &device) {
            Ok(json) => match logic::parse_disk_usage_json(&json) {
                Ok((total_bytes, used_bytes, free_bytes)) => FORMAT
                    .with(|f| logic::format_disk(&f.borrow(), total_bytes, used_bytes, free_bytes)),
                Err(err) => logic::format_error(&err),
            },
            Err(err) => logic::format_error(&err),
        };
        Output {
            text,
            interval_ms: 30_000,
        }
    }

    fn required_host_api_version() -> HostApiVersion {
        let (major, minor, patch) = REQUIRED_HOST_API;
        HostApiVersion {
            major,
            minor,
            patch,
        }
    }

    fn config_schema() -> Vec<ConfigParam> {
        fmt_common::config_schema![
            ConfigParam,
            ("device", DEFAULT_DEVICE),
            ("format", DEFAULT_FORMAT),
        ]
    }

    fn get_metadata() -> Metadata {
        Metadata {
            display_name: "Disk".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "ArtemChuban".to_string(),
        }
    }

    fn required_extensions() -> Vec<String> {
        vec!["disk".to_string()]
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::Guest;
    use super::logic::*;
    use fmt_common::human_bytes;

    #[test]
    fn parses_valid_config_with_both_fields() {
        assert_eq!(
            parse_config(r#"{"device":"/dev/sda1","format":"{used}/{total}"}"#),
            Some(Config {
                device: Some("/dev/sda1".to_string()),
                format: Some("{used}/{total}".to_string()),
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_device() {
        assert_eq!(
            parse_config(r#"{"device":"/dev/sda1"}"#),
            Some(Config {
                device: Some("/dev/sda1".to_string()),
                format: None,
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_format() {
        assert_eq!(
            parse_config(r#"{"format":"{free}"}"#),
            Some(Config {
                device: None,
                format: Some("{free}".to_string()),
            })
        );
    }

    #[test]
    fn empty_json_object_yields_both_fields_none() {
        assert_eq!(
            parse_config("{}"),
            Some(Config {
                device: None,
                format: None,
            })
        );
    }

    #[test]
    fn returns_none_for_invalid_json() {
        assert_eq!(parse_config("not json"), None);
    }

    #[test]
    fn returns_none_for_empty_string() {
        assert_eq!(parse_config(""), None);
    }

    #[test]
    fn human_bytes_zero() {
        assert_eq!(human_bytes(0), "0B");
    }

    #[test]
    fn human_bytes_below_1k() {
        assert_eq!(human_bytes(512), "512B");
    }

    #[test]
    fn human_bytes_exact_1k_boundary() {
        assert_eq!(human_bytes(1024), "1.0K");
    }

    #[test]
    fn human_bytes_exact_1g_boundary() {
        assert_eq!(human_bytes(1024 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn human_bytes_terabyte_range() {
        assert_eq!(human_bytes(2 * 1024u64.pow(4)), "2.0T");
    }

    #[test]
    fn human_bytes_caps_at_terabyte_unit() {
        assert_eq!(human_bytes(1024u64.pow(5)), "1024.0T");
    }

    #[test]
    fn format_disk_substitutes_all_placeholders() {
        assert_eq!(
            format_disk(
                "{used}/{total} used, {free} free",
                1024 * 1024 * 1024 * 10,
                1024 * 1024 * 1024 * 4,
                1024 * 1024 * 1024 * 6,
            ),
            "4.0G/10.0G used, 6.0G free"
        );
    }

    #[test]
    fn format_disk_no_placeholders_returns_unchanged() {
        assert_eq!(format_disk("static text", 100, 50, 50), "static text");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(
            format_error("device `/dev/sda9` not found in /proc/mounts"),
            "disk error: device `/dev/sda9` not found in /proc/mounts"
        );
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "disk error: ");
    }

    #[test]
    fn config_schema_declares_device_and_format() {
        assert_eq!(
            super::Component::config_schema(),
            vec![
                super::ConfigParam {
                    name: "device".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_DEVICE.to_string(),
                },
                super::ConfigParam {
                    name: "format".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_FORMAT.to_string(),
                },
            ]
        );
    }

    #[test]
    fn get_metadata_reports_display_name_version_and_author() {
        assert_eq!(
            super::Component::get_metadata(),
            super::Metadata {
                display_name: "Disk".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: "ArtemChuban".to_string(),
            }
        );
    }

    #[test]
    fn required_extensions_is_disk() {
        assert_eq!(
            super::Component::required_extensions(),
            vec!["disk".to_string()]
        );
    }

    #[test]
    fn parse_disk_usage_json_success() {
        assert_eq!(
            parse_disk_usage_json(r#"{"total_bytes":1000,"used_bytes":400,"free_bytes":600}"#),
            Ok((1000, 400, 600))
        );
    }

    #[test]
    fn parse_disk_usage_json_missing_fields() {
        assert!(parse_disk_usage_json(r#"{"total_bytes":1000}"#).is_err());
    }

    #[test]
    fn parse_disk_usage_json_garbage() {
        assert!(parse_disk_usage_json("not json").is_err());
    }
}
