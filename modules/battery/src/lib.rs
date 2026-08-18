wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use crate::bslstatus::module::host;
use exports::bslstatus::module::guest::{Guest, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_PATH: &str = "/sys/class/power_supply/BAT0/capacity";
const DEFAULT_FORMAT: &str = "BAT {}%";

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
        format.replace("{}", raw_content.trim())
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

    fn on_click(_button: u8) {}
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
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
}
