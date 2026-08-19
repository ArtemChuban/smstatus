wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{Guest, HostApiVersion, Output};
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

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn human_bytes(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
        let mut value = bytes as f64;
        let mut unit = 0;
        while value >= 1024.0 && unit < UNITS.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            format!("{value:.0}{}", UNITS[unit])
        } else {
            format!("{value:.1}{}", UNITS[unit])
        }
    }

    pub fn format_disk(format: &str, total_bytes: u64, used_bytes: u64, free_bytes: u64) -> String {
        format
            .replace("{total}", &human_bytes(total_bytes))
            .replace("{used}", &human_bytes(used_bytes))
            .replace("{free}", &human_bytes(free_bytes))
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
        let text = match host::read_disk_usage(&device) {
            Ok(usage) => FORMAT.with(|f| {
                logic::format_disk(
                    &f.borrow(),
                    usage.total_bytes,
                    usage.used_bytes,
                    usage.free_bytes,
                )
            }),
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
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::logic::*;

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
}
