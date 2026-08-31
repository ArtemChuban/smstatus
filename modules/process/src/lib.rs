wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_FORMAT: &str = "{}";
const DEFAULT_ACTIVE_LABEL: &str = "on";
const DEFAULT_INACTIVE_LABEL: &str = "off";
const FALLBACK_INTERVAL_MS: u32 = 300_000;

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
    static SUBSCRIBE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static LAST_RUNNING: RefCell<Option<bool>> = const { RefCell::new(None) };
}

mod logic {
    use super::Config;
    use serde::Deserialize;

    const COMM_MAX_LEN: usize = 15;

    #[derive(Deserialize)]
    struct RunningChanged {
        process: String,
        running: bool,
    }

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn process_name_matches(comm_contents: &str, target_name: &str) -> bool {
        let comm = comm_contents.trim();
        if comm == target_name {
            return true;
        }
        if target_name.len() <= COMM_MAX_LEN {
            return false;
        }
        let mut boundary = COMM_MAX_LEN;
        while !target_name.is_char_boundary(boundary) {
            boundary -= 1;
        }
        Some(comm) == target_name.get(..boundary)
    }

    pub fn running_from_matching_payload(json: &str, configured: &str) -> Option<bool> {
        let parsed: RunningChanged = serde_json::from_str(json).ok()?;
        if process_name_matches(&parsed.process, configured) {
            Some(parsed.running)
        } else {
            None
        }
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

enum DrainResult {
    Matched(bool),
    UnrelatedOnly,
    Empty,
}

fn take_matching_running_changed(configured: &str) -> DrainResult {
    let mut latest = None;
    let mut saw_any = false;
    while let Some(event) = host::take_extension_event() {
        saw_any = true;
        if event.extension == "process"
            && event.event == "running-changed"
            && let Some(running) = logic::running_from_matching_payload(&event.payload, configured)
        {
            latest = Some(running);
        }
    }
    match latest {
        Some(running) => DrainResult::Matched(running),
        None if saw_any => DrainResult::UnrelatedOnly,
        None => DrainResult::Empty,
    }
}

fn format_with_labels(running: bool) -> String {
    FORMAT.with(|f| {
        ACTIVE_LABEL.with(|a| {
            INACTIVE_LABEL
                .with(|i| logic::format_status(&f.borrow(), running, &a.borrow(), &i.borrow()))
        })
    })
}

fn running_via_rpc(process: &str) -> Result<bool, String> {
    let body = host::call_extension("process", "is-running", process)?;
    body.parse::<bool>()
        .map_err(|err| format!("malformed process-running response: {err}"))
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
            LAST_RUNNING.with(|s| *s.borrow_mut() = None);
        }
        match host::subscribe_extension_event("process", "running-changed") {
            Ok(()) => SUBSCRIBE_ERROR.with(|err| *err.borrow_mut() = None),
            Err(err) => SUBSCRIBE_ERROR.with(|slot| *slot.borrow_mut() = Some(err)),
        }
    }

    fn update() -> Output {
        let process = PROCESS.with(|p| p.borrow().clone());
        if process.is_empty() {
            return Output {
                text: logic::format_error("no process configured"),
                interval_ms: FALLBACK_INTERVAL_MS,
            };
        }
        if let Some(err) = SUBSCRIBE_ERROR.with(|slot| slot.borrow().clone()) {
            return Output {
                text: logic::format_error(&format!("subscribe: {err}")),
                interval_ms: FALLBACK_INTERVAL_MS,
            };
        }
        let text = match take_matching_running_changed(&process) {
            DrainResult::Matched(running) => {
                LAST_RUNNING.with(|s| *s.borrow_mut() = Some(running));
                format_with_labels(running)
            }
            DrainResult::UnrelatedOnly => {
                if let Some(running) = LAST_RUNNING.with(|s| *s.borrow()) {
                    format_with_labels(running)
                } else {
                    match running_via_rpc(&process) {
                        Ok(running) => {
                            LAST_RUNNING.with(|s| *s.borrow_mut() = Some(running));
                            format_with_labels(running)
                        }
                        Err(err) => logic::format_error(&err),
                    }
                }
            }
            DrainResult::Empty => match running_via_rpc(&process) {
                Ok(running) => {
                    LAST_RUNNING.with(|s| *s.borrow_mut() = Some(running));
                    format_with_labels(running)
                }
                Err(err) => logic::format_error(&err),
            },
        };
        Output {
            text,
            interval_ms: FALLBACK_INTERVAL_MS,
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
    fn process_name_matches_exact_comm() {
        assert!(process_name_matches("firefox", "firefox"));
    }

    #[test]
    fn process_name_matches_truncated_prefix_of_long_config() {
        assert!(process_name_matches(
            "some-very-long-",
            "some-very-long-process-name"
        ));
    }

    #[test]
    fn process_name_matches_rejects_unrelated_comm() {
        assert!(!process_name_matches(
            "other-name",
            "some-very-long-process-name"
        ));
    }

    #[test]
    fn running_from_matching_payload_accepts_match() {
        assert_eq!(
            running_from_matching_payload(r#"{"process":"firefox","running":true}"#, "firefox"),
            Some(true)
        );
    }

    #[test]
    fn running_from_matching_payload_filters_non_match() {
        assert_eq!(
            running_from_matching_payload(r#"{"process":"chrome","running":true}"#, "firefox"),
            None
        );
    }

    #[test]
    fn running_from_matching_payload_matches_truncated_comm() {
        assert_eq!(
            running_from_matching_payload(
                r#"{"process":"some-very-long-","running":false}"#,
                "some-very-long-process-name"
            ),
            Some(false)
        );
    }

    #[test]
    fn running_from_matching_payload_rejects_garbage() {
        assert_eq!(running_from_matching_payload("not json", "firefox"), None);
    }
}
