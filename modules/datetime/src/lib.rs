wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, HostApiVersion, Metadata, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_FORMAT: &str = "[year]-[month]-[day] [hour]:[minute]:[second]";
const REQUIRED_HOST_API: (u32, u32, u32) = (1, 0, 0);

#[derive(Deserialize, Default)]
struct Config {
    format: Option<String>,
}

thread_local! {
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
}

mod logic {
    use crate::DEFAULT_FORMAT;

    use super::Config;
    use serde::Deserialize;
    use time::OffsetDateTime;

    #[derive(Deserialize)]
    struct TimeStateJson {
        now_ms: u64,
        offset_seconds: i32,
    }

    pub fn parse_format_from_config(config: &str) -> Option<String> {
        serde_json::from_str::<Config>(config)
            .ok()
            .map(|c| c.format.unwrap_or_else(|| DEFAULT_FORMAT.to_string()))
    }

    pub fn parse_time_state_json(json: &str) -> Result<(u64, i32), String> {
        serde_json::from_str::<TimeStateJson>(json)
            .map(|s| (s.now_ms, s.offset_seconds))
            .map_err(|e| format!("malformed time state: {e}"))
    }

    pub fn to_local_datetime(ms: u64, offset_secs: i32) -> OffsetDateTime {
        let dt = OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let offset =
            time::UtcOffset::from_whole_seconds(offset_secs).unwrap_or(time::UtcOffset::UTC);
        dt.to_offset(offset)
    }

    pub fn format_datetime(dt: OffsetDateTime, fmt: &str) -> String {
        time::format_description::parse_borrowed::<3>(fmt)
            .ok()
            .and_then(|desc| dt.format(&desc).ok())
            .unwrap_or_else(|| "time format error".to_string())
    }

    pub fn format_error(err: &str) -> String {
        format!("datetime error: {err}")
    }
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Some(format) = logic::parse_format_from_config(&config) {
            FORMAT.with(|f| *f.borrow_mut() = format);
        }
    }

    fn update() -> Output {
        let text = match host::call_extension("time", "now", "") {
            Ok(json) => match logic::parse_time_state_json(&json) {
                Ok((now_ms, offset_seconds)) => {
                    let dt = logic::to_local_datetime(now_ms, offset_seconds);
                    FORMAT.with(|f| logic::format_datetime(dt, &f.borrow()))
                }
                Err(err) => logic::format_error(&err),
            },
            Err(err) => logic::format_error(&err),
        };

        Output {
            text,
            interval_ms: 1000,
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
            display_name: "Datetime".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "ArtemChuban".to_string(),
        }
    }

    fn required_extensions() -> Vec<String> {
        vec!["time".to_string()]
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::DEFAULT_FORMAT;
    use super::Guest;
    use super::logic::*;
    use time::OffsetDateTime;

    #[test]
    fn parses_valid_config_with_format() {
        assert_eq!(
            parse_format_from_config(r#"{"format":"[hour]:[minute]"}"#),
            Some("[hour]:[minute]".to_string())
        );
    }

    #[test]
    fn missing_format_field_falls_back_to_default() {
        assert_eq!(
            parse_format_from_config(r#"{}"#),
            Some(DEFAULT_FORMAT.to_string())
        );
    }

    #[test]
    fn null_format_field_falls_back_to_default() {
        assert_eq!(
            parse_format_from_config(r#"{"format":null}"#),
            Some(DEFAULT_FORMAT.to_string())
        );
    }
    #[test]
    fn returns_none_for_invalid_json() {
        assert_eq!(parse_format_from_config("not json"), None);
    }

    #[test]
    fn returns_none_for_empty_string() {
        assert_eq!(parse_format_from_config(""), None);
    }

    #[test]
    fn returns_none_for_json_with_wrong_types() {
        assert_eq!(parse_format_from_config(r#"{"format":123}"#), None);
    }

    #[test]
    fn ignores_unknown_extra_fields() {
        assert_eq!(
            parse_format_from_config(r#"{"format":"F","extra":"ignored"}"#),
            Some("F".to_string())
        );
    }

    #[test]
    fn accepts_empty_format_string_as_some() {
        assert_eq!(
            parse_format_from_config(r#"{"format":""}"#),
            Some(String::new())
        );
    }

    #[test]
    fn ms_zero_is_unix_epoch_utc() {
        let dt = to_local_datetime(0, 0);
        assert_eq!(dt, OffsetDateTime::UNIX_EPOCH);
    }

    #[test]
    fn known_ms_converts_to_expected_utc_datetime() {
        let dt = to_local_datetime(1_546_300_800_000, 0);
        assert_eq!(dt.unix_timestamp(), 1_546_300_800);
        assert_eq!(dt.offset(), time::UtcOffset::UTC);
    }

    #[test]
    fn applies_positive_offset() {
        let dt = to_local_datetime(0, 3600);
        assert_eq!(dt.hour(), 1);
        assert_eq!(dt.offset().whole_seconds(), 3600);
    }

    #[test]
    fn applies_negative_offset() {
        let dt = to_local_datetime(0, -3600);
        assert_eq!(dt.hour(), 23);
        assert_eq!(dt.offset().whole_seconds(), -3600);
    }

    #[test]
    fn ms_u64_max_falls_back_to_unix_epoch() {
        let dt = to_local_datetime(u64::MAX, 0);
        assert_eq!(dt, OffsetDateTime::UNIX_EPOCH);
    }

    #[test]
    fn max_representable_ms_does_not_fall_back() {
        let max_ok_ms = 253_402_300_799_000u64;
        let dt = to_local_datetime(max_ok_ms, 0);
        assert_eq!(dt.year(), 9999);
        assert_ne!(dt, OffsetDateTime::UNIX_EPOCH);
    }

    #[test]
    fn offset_out_of_range_positive_falls_back_to_utc() {
        let dt = to_local_datetime(0, 93_600);
        assert_eq!(dt.offset(), time::UtcOffset::UTC);
    }

    #[test]
    fn offset_out_of_range_negative_falls_back_to_utc() {
        let dt = to_local_datetime(0, -93_600);
        assert_eq!(dt.offset(), time::UtcOffset::UTC);
    }

    #[test]
    fn offset_at_max_valid_boundary_is_applied() {
        let dt = to_local_datetime(0, 93_599);
        assert_eq!(dt.offset().whole_seconds(), 93_599);
    }

    #[test]
    fn offset_at_min_valid_boundary_is_applied() {
        let dt = to_local_datetime(0, -93_599);
        assert_eq!(dt.offset().whole_seconds(), -93_599);
    }

    #[test]
    fn both_fallbacks_can_apply_independently() {
        let dt = to_local_datetime(u64::MAX, 999_999);
        assert_eq!(
            dt.unix_timestamp(),
            OffsetDateTime::UNIX_EPOCH.unix_timestamp()
        );
        assert_eq!(dt.offset(), time::UtcOffset::UTC);
    }

    #[test]
    fn formats_with_default_format() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let text = format_datetime(dt, "[year]-[month]-[day] [hour]:[minute]:[second]");
        assert_eq!(text, "1970-01-01 00:00:00");
    }

    #[test]
    fn formats_with_custom_format() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let text = format_datetime(dt, "[hour]:[minute]");
        assert_eq!(text, "00:00");
    }

    #[test]
    fn invalid_format_description_returns_error_text() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let text = format_datetime(dt, "[not_a_real_component]");
        assert_eq!(text, "time format error");
    }

    #[test]
    fn unclosed_bracket_returns_error_text() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let text = format_datetime(dt, "[year");
        assert_eq!(text, "time format error");
    }

    #[test]
    fn literal_text_passes_through() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let text = format_datetime(dt, "literal text, no components");
        assert_eq!(text, "literal text, no components");
    }

    #[test]
    fn empty_format_string_produces_empty_output() {
        let dt = OffsetDateTime::UNIX_EPOCH;
        let text = format_datetime(dt, "");
        assert_eq!(text, "");
    }

    #[test]
    fn config_schema_declares_format() {
        assert_eq!(
            super::Component::config_schema(),
            vec![super::ConfigParam {
                name: "format".to_string(),
                param_type: "string".to_string(),
                default: DEFAULT_FORMAT.to_string(),
            }]
        );
    }

    #[test]
    fn get_metadata_reports_display_name_version_and_author() {
        assert_eq!(
            super::Component::get_metadata(),
            super::Metadata {
                display_name: "Datetime".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: "ArtemChuban".to_string(),
            }
        );
    }

    #[test]
    fn required_extensions_is_time() {
        assert_eq!(
            super::Component::required_extensions(),
            vec!["time".to_string()]
        );
    }

    #[test]
    fn parse_time_state_json_success() {
        assert_eq!(
            parse_time_state_json(r#"{"now_ms":123,"offset_seconds":3600}"#),
            Ok((123, 3600))
        );
    }

    #[test]
    fn parse_time_state_json_missing_fields() {
        assert!(parse_time_state_json(r#"{"now_ms":123}"#).is_err());
    }

    #[test]
    fn parse_time_state_json_garbage() {
        assert!(parse_time_state_json("not json").is_err());
    }
}
