wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, HostApiVersion, Metadata, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_FORMAT: &str = "{used}/{total} used, {free} free";
const REQUIRED_HOST_API: (u32, u32, u32) = (2, 0, 0);

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    format: Option<String>,
}

thread_local! {
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
}

mod logic {
    use super::Config;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct MemUsageJson {
        total_bytes: u64,
        used_bytes: u64,
        free_bytes: u64,
    }

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn parse_mem_usage_json(json: &str) -> Result<(u64, u64, u64), String> {
        serde_json::from_str::<MemUsageJson>(json)
            .map(|s| (s.total_bytes, s.used_bytes, s.free_bytes))
            .map_err(|e| format!("malformed mem usage: {e}"))
    }

    pub fn format_ram(format: &str, total_bytes: u64, used_bytes: u64, free_bytes: u64) -> String {
        fmt_common::format_usage(format, total_bytes, used_bytes, free_bytes)
    }

    pub fn format_error(err: &str) -> String {
        format!("ram error: {err}")
    }
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Some(parsed) = logic::parse_config(&config) {
            let format = parsed.format.unwrap_or_else(|| DEFAULT_FORMAT.to_string());
            FORMAT.with(|f| *f.borrow_mut() = format);
        }
    }

    fn update() -> Output {
        let text = match host::call_extension("mem", "usage", "") {
            Ok(json) => match logic::parse_mem_usage_json(&json) {
                Ok((total_bytes, used_bytes, free_bytes)) => FORMAT
                    .with(|f| logic::format_ram(&f.borrow(), total_bytes, used_bytes, free_bytes)),
                Err(err) => logic::format_error(&err),
            },
            Err(err) => logic::format_error(&err),
        };
        Output {
            text,
            interval_ms: 5_000,
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
        fmt_common::config_schema![ConfigParam, ("format", DEFAULT_FORMAT),]
    }

    fn get_metadata() -> Metadata {
        Metadata {
            display_name: "RAM".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "ArtemChuban".to_string(),
        }
    }

    fn required_extensions() -> Vec<String> {
        vec!["mem".to_string()]
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
    fn parses_valid_config_with_format() {
        assert_eq!(
            parse_config(r#"{"format":"{used}/{total}"}"#),
            Some(Config {
                format: Some("{used}/{total}".to_string()),
            })
        );
    }

    #[test]
    fn empty_json_object_yields_format_none() {
        assert_eq!(parse_config("{}"), Some(Config { format: None }));
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
    fn format_ram_substitutes_all_placeholders() {
        assert_eq!(
            format_ram(
                "{used}/{total} used, {free} free",
                1024 * 1024 * 1024 * 16,
                1024 * 1024 * 1024 * 6,
                1024 * 1024 * 1024 * 10,
            ),
            "6.0G/16.0G used, 10.0G free"
        );
    }

    #[test]
    fn format_ram_no_placeholders_returns_unchanged() {
        assert_eq!(format_ram("static text", 100, 50, 50), "static text");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(
            format_error("MemTotal not found in /proc/meminfo"),
            "ram error: MemTotal not found in /proc/meminfo"
        );
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "ram error: ");
    }

    #[test]
    fn config_schema_declares_format() {
        assert_eq!(
            super::Component::config_schema(),
            vec![super::ConfigParam {
                name: "format".to_string(),
                param_type: "string".to_string(),
                default: super::DEFAULT_FORMAT.to_string(),
            }]
        );
    }

    #[test]
    fn get_metadata_reports_display_name_version_and_author() {
        assert_eq!(
            super::Component::get_metadata(),
            super::Metadata {
                display_name: "RAM".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: "ArtemChuban".to_string(),
            }
        );
    }

    #[test]
    fn required_extensions_is_mem() {
        assert_eq!(
            super::Component::required_extensions(),
            vec!["mem".to_string()]
        );
    }

    #[test]
    fn parse_mem_usage_json_success() {
        assert_eq!(
            parse_mem_usage_json(
                r#"{"total_bytes":1024000,"used_bytes":614400,"free_bytes":409600}"#
            ),
            Ok((1_024_000, 614_400, 409_600))
        );
    }

    #[test]
    fn parse_mem_usage_json_missing_fields() {
        assert!(parse_mem_usage_json(r#"{"total_bytes":1024000}"#).is_err());
    }

    #[test]
    fn parse_mem_usage_json_garbage() {
        assert!(parse_mem_usage_json("not json").is_err());
    }
}
