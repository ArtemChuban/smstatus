wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, HostApiVersion, Metadata, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_PATH: &str = "/sys/class/power_supply/BAT0/capacity";
const DEFAULT_FORMAT: &str = "BAT {:3}%";
const REQUIRED_HOST_API: (u32, u32, u32) = (1, 0, 0);

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    path: Option<String>,
    format: Option<String>,
}

thread_local! {
    static PATH: RefCell<String> = RefCell::new(DEFAULT_PATH.to_string());
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
}

mod logic {
    use super::Config;

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn format_battery(format: &str, raw_content: &str) -> String {
        fmt_common::format_template(format, &[("", raw_content.trim())])
    }

    pub fn format_error(err: &str) -> String {
        format!("BAT error: {err}")
    }
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Some(parsed) = logic::parse_config(&config) {
            let path = parsed.path.unwrap_or_else(|| DEFAULT_PATH.to_string());
            let format = parsed.format.unwrap_or_else(|| DEFAULT_FORMAT.to_string());
            PATH.with(|p| *p.borrow_mut() = path);
            FORMAT.with(|f| *f.borrow_mut() = format);
        }
    }

    fn update() -> Output {
        let path = PATH.with(|p| p.borrow().clone());
        let text = match host::read_sysfs(&path) {
            Ok(content) => FORMAT.with(|f| logic::format_battery(&f.borrow(), &content)),
            Err(err) => logic::format_error(&err),
        };
        Output {
            text,
            interval_ms: 5000,
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
            ("path", DEFAULT_PATH),
            ("format", DEFAULT_FORMAT),
        ]
    }

    fn get_metadata() -> Metadata {
        Metadata {
            display_name: "Battery".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "ArtemChuban".to_string(),
        }
    }

    fn required_extensions() -> Vec<String> {
        vec![]
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::Guest;
    use super::logic::*;

    #[test]
    fn parses_valid_config_with_both_fields() {
        assert_eq!(
            parse_config(r#"{"path":"/p","format":"BAT {}%"}"#),
            Some(Config {
                path: Some("/p".to_string()),
                format: Some("BAT {}%".to_string()),
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_path() {
        assert_eq!(
            parse_config(r#"{"path":"/p"}"#),
            Some(Config {
                path: Some("/p".to_string()),
                format: None,
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_format() {
        assert_eq!(
            parse_config(r#"{"format":"BAT {}%"}"#),
            Some(Config {
                path: None,
                format: Some("BAT {}%".to_string()),
            })
        );
    }

    #[test]
    fn empty_json_object_yields_both_fields_none() {
        assert_eq!(
            parse_config("{}"),
            Some(Config {
                path: None,
                format: None,
            })
        );
    }

    #[test]
    fn null_fields_yield_none() {
        assert_eq!(
            parse_config(r#"{"path":null,"format":null}"#),
            Some(Config {
                path: None,
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
    fn returns_none_for_wrong_field_type() {
        assert_eq!(parse_config(r#"{"path":123}"#), None);
    }

    #[test]
    fn ignores_unknown_extra_fields() {
        assert_eq!(
            parse_config(r#"{"path":"/p","extra":"ignored"}"#),
            Some(Config {
                path: Some("/p".to_string()),
                format: None,
            })
        );
    }

    #[test]
    fn substitutes_and_trims_trailing_newline() {
        assert_eq!(format_battery("BAT {}%", "87\n"), "BAT 87%");
    }

    #[test]
    fn no_placeholder_returns_format_unchanged() {
        assert_eq!(format_battery("static text", "87"), "static text");
    }

    #[test]
    fn zero_pads_capacity() {
        assert_eq!(format_battery("BAT {:03}%", "9"), "BAT 009%");
    }

    #[test]
    fn space_pads_capacity() {
        assert_eq!(format_battery("BAT {:3}%", "9"), "BAT   9%");
    }

    #[test]
    fn multiple_placeholders_all_replaced() {
        assert_eq!(format_battery("{} {}", "50"), "50 50");
    }

    #[test]
    fn empty_content_after_trim_replaces_with_empty_string() {
        assert_eq!(format_battery("BAT {}%", "   \n"), "BAT %");
    }

    #[test]
    fn trims_leading_and_trailing_whitespace() {
        assert_eq!(format_battery("{}", "  87  \n"), "87");
    }

    #[test]
    fn empty_format_string_returns_empty_string() {
        assert_eq!(format_battery("", "87"), "");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(
            format_error("read failed: no such file"),
            "BAT error: read failed: no such file"
        );
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "BAT error: ");
    }

    #[test]
    fn passes_through_special_characters() {
        assert_eq!(
            format_error("err\nwith\nnewlines"),
            "BAT error: err\nwith\nnewlines"
        );
    }

    #[test]
    fn config_schema_declares_path_and_format() {
        assert_eq!(
            super::Component::config_schema(),
            vec![
                super::ConfigParam {
                    name: "path".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_PATH.to_string(),
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
                display_name: "Battery".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: "ArtemChuban".to_string(),
            }
        );
    }

    #[test]
    fn required_extensions_is_empty() {
        assert_eq!(
            super::Component::required_extensions(),
            Vec::<String>::new()
        );
    }
}
