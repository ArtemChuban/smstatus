wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{Guest, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_FORMAT: &str = "{used}/{total} used, {free} free";

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    format: Option<String>,
}

thread_local! {
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

    pub fn format_ram(format: &str, total_bytes: u64, used_bytes: u64, free_bytes: u64) -> String {
        format
            .replace("{total}", &human_bytes(total_bytes))
            .replace("{used}", &human_bytes(used_bytes))
            .replace("{free}", &human_bytes(free_bytes))
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
        let text = match host::read_mem_usage() {
            Ok(usage) => FORMAT.with(|f| {
                logic::format_ram(
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
            interval_ms: 5_000,
        }
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::logic::*;

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
}
