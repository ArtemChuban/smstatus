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
const DEFAULT_ACTIVE_LABEL: &str = "on";
const DEFAULT_INACTIVE_LABEL: &str = "off";
const REQUIRED_HOST_API: (u32, u32, u32) = (1, 0, 0);

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    process: Option<String>,
    format: Option<String>,
    active_label: Option<String>,
    inactive_label: Option<String>,
}

thread_local! {
    static PROCESS: RefCell<String> = const { RefCell::new(String::new()) };
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
    static ACTIVE_LABEL: RefCell<String> = RefCell::new(DEFAULT_ACTIVE_LABEL.to_string());
    static INACTIVE_LABEL: RefCell<String> = RefCell::new(DEFAULT_INACTIVE_LABEL.to_string());
}

mod logic {
    use super::Config;

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn format_status(
        format: &str,
        running: bool,
        active_label: &str,
        inactive_label: &str,
    ) -> String {
        let label = if running {
            active_label
        } else {
            inactive_label
        };
        fmt_common::format_template(format, &[("", label)])
    }

    pub fn format_error(err: &str) -> String {
        format!("process error: {err}")
    }
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Some(parsed) = logic::parse_config(&config) {
            let process = parsed.process.unwrap_or_default();
            let format = parsed.format.unwrap_or_else(|| DEFAULT_FORMAT.to_string());
            let active_label = parsed
                .active_label
                .unwrap_or_else(|| DEFAULT_ACTIVE_LABEL.to_string());
            let inactive_label = parsed
                .inactive_label
                .unwrap_or_else(|| DEFAULT_INACTIVE_LABEL.to_string());
            PROCESS.with(|p| *p.borrow_mut() = process);
            FORMAT.with(|f| *f.borrow_mut() = format);
            ACTIVE_LABEL.with(|a| *a.borrow_mut() = active_label);
            INACTIVE_LABEL.with(|i| *i.borrow_mut() = inactive_label);
        }
    }

    fn update() -> Output {
        let process = PROCESS.with(|p| p.borrow().clone());
        let text = if process.is_empty() {
            logic::format_error("no process configured")
        } else {
            match host::read_process_running(&process) {
                Ok(running) => FORMAT.with(|f| {
                    ACTIVE_LABEL.with(|a| {
                        INACTIVE_LABEL.with(|i| {
                            logic::format_status(&f.borrow(), running, &a.borrow(), &i.borrow())
                        })
                    })
                }),
                Err(err) => logic::format_error(&err),
            }
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
            ("process", ""),
            ("format", DEFAULT_FORMAT),
            ("active_label", DEFAULT_ACTIVE_LABEL),
            ("inactive_label", DEFAULT_INACTIVE_LABEL),
        ]
    }

    fn get_metadata() -> Metadata {
        Metadata {
            display_name: "Process".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "ArtemChuban".to_string(),
        }
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::Guest;
    use super::logic::*;

    #[test]
    fn parses_valid_config_with_all_fields() {
        assert_eq!(
            parse_config(
                r#"{"process":"openvpn","format":"VPN {}","active_label":"on","inactive_label":"off"}"#
            ),
            Some(Config {
                process: Some("openvpn".to_string()),
                format: Some("VPN {}".to_string()),
                active_label: Some("on".to_string()),
                inactive_label: Some("off".to_string()),
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_process() {
        assert_eq!(
            parse_config(r#"{"process":"openvpn"}"#),
            Some(Config {
                process: Some("openvpn".to_string()),
                format: None,
                active_label: None,
                inactive_label: None,
            })
        );
    }

    #[test]
    fn empty_json_object_yields_all_fields_none() {
        assert_eq!(
            parse_config("{}"),
            Some(Config {
                process: None,
                format: None,
                active_label: None,
                inactive_label: None,
            })
        );
    }

    #[test]
    fn null_fields_yield_none() {
        assert_eq!(
            parse_config(
                r#"{"process":null,"format":null,"active_label":null,"inactive_label":null}"#
            ),
            Some(Config {
                process: None,
                format: None,
                active_label: None,
                inactive_label: None,
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
        assert_eq!(parse_config(r#"{"process":123}"#), None);
    }

    #[test]
    fn ignores_unknown_extra_fields() {
        assert_eq!(
            parse_config(r#"{"process":"openvpn","extra":"ignored"}"#),
            Some(Config {
                process: Some("openvpn".to_string()),
                format: None,
                active_label: None,
                inactive_label: None,
            })
        );
    }

    #[test]
    fn substitutes_active_label_when_running() {
        assert_eq!(format_status("VPN {}", true, "on", "off"), "VPN on");
    }

    #[test]
    fn substitutes_inactive_label_when_not_running() {
        assert_eq!(format_status("VPN {}", false, "on", "off"), "VPN off");
    }

    #[test]
    fn no_placeholder_returns_format_unchanged() {
        assert_eq!(
            format_status("static text", true, "on", "off"),
            "static text"
        );
    }

    #[test]
    fn multiple_placeholders_all_replaced() {
        assert_eq!(format_status("{} {}", true, "on", "off"), "on on");
    }

    #[test]
    fn empty_format_string_returns_empty_string() {
        assert_eq!(format_status("", true, "on", "off"), "");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(
            format_error("read failed: no such file"),
            "process error: read failed: no such file"
        );
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "process error: ");
    }

    #[test]
    fn passes_through_special_characters() {
        assert_eq!(
            format_error("err\nwith\nnewlines"),
            "process error: err\nwith\nnewlines"
        );
    }

    #[test]
    fn config_schema_declares_all_params() {
        assert_eq!(
            super::Component::config_schema(),
            vec![
                super::ConfigParam {
                    name: "process".to_string(),
                    param_type: "string".to_string(),
                    default: "".to_string(),
                },
                super::ConfigParam {
                    name: "format".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_FORMAT.to_string(),
                },
                super::ConfigParam {
                    name: "active_label".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_ACTIVE_LABEL.to_string(),
                },
                super::ConfigParam {
                    name: "inactive_label".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_INACTIVE_LABEL.to_string(),
                },
            ]
        );
    }

    #[test]
    fn get_metadata_reports_display_name_version_and_author() {
        assert_eq!(
            super::Component::get_metadata(),
            super::Metadata {
                display_name: "Process".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: "ArtemChuban".to_string(),
            }
        );
    }
}
