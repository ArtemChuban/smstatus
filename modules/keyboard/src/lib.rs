wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, HostApiVersion, Metadata, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_FORMAT: &str = "{}";
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

    const INVALID_PREFIXES: [&str; 4] = ["evdev", "inet", "pc", "base"];

    #[derive(Deserialize)]
    struct XkbStateJson {
        active_group: u8,
        symbols: String,
    }

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn parse_xkb_state_json(json: &str) -> Result<(u8, String), String> {
        serde_json::from_str::<XkbStateJson>(json)
            .map(|s| (s.active_group, s.symbols))
            .map_err(|e| format!("malformed xkb state: {e}"))
    }

    fn is_layout_token(tok: &str) -> bool {
        if INVALID_PREFIXES
            .iter()
            .any(|prefix| tok.starts_with(prefix))
        {
            return false;
        }
        let mut chars = tok.chars();
        !matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_digit())
    }

    pub fn extract_layout(symbols: &str, active_group: u8) -> Result<String, String> {
        symbols
            .split(['+', ':'])
            .filter(|tok| !tok.is_empty())
            .filter(|tok| is_layout_token(tok))
            .nth(active_group as usize)
            .map(str::to_string)
            .ok_or_else(|| "no layout found for active group".to_string())
    }

    pub fn format_layout(format: &str, layout: &str) -> String {
        fmt_common::format_template(format, &[("", layout)])
    }

    pub fn format_error(err: &str) -> String {
        format!("KBD error: {err}")
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
        let text = match host::call_extension("xkb", "state", "") {
            Ok(json) => match logic::parse_xkb_state_json(&json) {
                Ok((active_group, symbols)) => {
                    match logic::extract_layout(&symbols, active_group) {
                        Ok(layout) => FORMAT.with(|f| logic::format_layout(&f.borrow(), &layout)),
                        Err(err) => logic::format_error(&err),
                    }
                }
                Err(err) => logic::format_error(&err),
            },
            Err(err) => logic::format_error(&err),
        };
        Output {
            text,
            interval_ms: 300,
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
            display_name: "Keyboard".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "ArtemChuban".to_string(),
        }
    }

    fn required_extensions() -> Vec<String> {
        vec!["xkb".to_string()]
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::Guest;
    use super::logic::*;

    #[test]
    fn parses_valid_config_with_format() {
        assert_eq!(
            parse_config(r#"{"format":"[{}]"}"#),
            Some(Config {
                format: Some("[{}]".to_string()),
            })
        );
    }

    #[test]
    fn missing_format_field_yields_none() {
        assert_eq!(parse_config("{}"), Some(Config { format: None }));
    }

    #[test]
    fn null_format_field_yields_none() {
        assert_eq!(
            parse_config(r#"{"format":null}"#),
            Some(Config { format: None })
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
        assert_eq!(parse_config(r#"{"format":123}"#), None);
    }

    #[test]
    fn ignores_unknown_extra_fields() {
        assert_eq!(
            parse_config(r#"{"format":"[{}]","extra":"ignored"}"#),
            Some(Config {
                format: Some("[{}]".to_string()),
            })
        );
    }

    #[test]
    fn extracts_first_group_layout() {
        assert_eq!(
            extract_layout("pc+us+ru:2+inet(evdev)", 0),
            Ok("us".to_string())
        );
    }

    #[test]
    fn extracts_second_group_layout() {
        assert_eq!(
            extract_layout("pc+us+ru:2+inet(evdev)", 1),
            Ok("ru".to_string())
        );
    }

    #[test]
    fn filters_pc_prefix_token() {
        assert_eq!(extract_layout("pc+us", 0), Ok("us".to_string()));
    }

    #[test]
    fn filters_inet_option_token() {
        assert_eq!(
            extract_layout("pc+us+ru:2+inet(evdev)", 1),
            Ok("ru".to_string())
        );
    }

    #[test]
    fn filters_base_prefix_token() {
        assert_eq!(extract_layout("base+us", 0), Ok("us".to_string()));
    }

    #[test]
    fn filters_bare_digit_group_marker() {
        assert_eq!(extract_layout("us+ru:2", 1), Ok("ru".to_string()));
    }

    #[test]
    fn does_not_filter_multi_digit_tokens() {
        assert_eq!(extract_layout("us+12", 1), Ok("12".to_string()));
    }

    #[test]
    fn errors_on_out_of_range_group() {
        assert_eq!(
            extract_layout("pc+us", 1),
            Err("no layout found for active group".to_string())
        );
    }

    #[test]
    fn errors_on_empty_symbols_string() {
        assert_eq!(
            extract_layout("", 0),
            Err("no layout found for active group".to_string())
        );
    }

    #[test]
    fn errors_when_only_junk_tokens_present() {
        assert_eq!(
            extract_layout("pc+base+evdev", 0),
            Err("no layout found for active group".to_string())
        );
    }

    #[test]
    fn format_layout_substitutes_placeholder() {
        assert_eq!(format_layout("[{}]", "en"), "[en]");
    }

    #[test]
    fn format_layout_no_placeholder_returns_format_unchanged() {
        assert_eq!(format_layout("static text", "en"), "static text");
    }

    #[test]
    fn format_layout_multiple_placeholders_all_replaced() {
        assert_eq!(format_layout("{} {}", "en"), "en en");
    }

    #[test]
    fn format_layout_empty_format_string_returns_empty_string() {
        assert_eq!(format_layout("", "en"), "");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(
            format_error("no layout found for active group"),
            "KBD error: no layout found for active group"
        );
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "KBD error: ");
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
                display_name: "Keyboard".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: "ArtemChuban".to_string(),
            }
        );
    }

    #[test]
    fn required_extensions_is_xkb() {
        assert_eq!(
            super::Component::required_extensions(),
            vec!["xkb".to_string()]
        );
    }

    #[test]
    fn parse_xkb_state_json_success() {
        assert_eq!(
            parse_xkb_state_json(r#"{"active_group":1,"symbols":"pc+us+ru:2"}"#),
            Ok((1, "pc+us+ru:2".to_string()))
        );
    }

    #[test]
    fn parse_xkb_state_json_missing_fields() {
        assert!(parse_xkb_state_json(r#"{"active_group":1}"#).is_err());
    }

    #[test]
    fn parse_xkb_state_json_garbage() {
        assert!(parse_xkb_state_json("not json").is_err());
    }
}
