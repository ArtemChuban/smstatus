wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, HostApiVersion, Metadata, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_CREDENTIALS_PATH: &str = "~/.claude/.credentials.json";
const DEFAULT_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const DEFAULT_FORMAT: &str = "5h:{session}%({session_reset}) 7d:{week}%({week_reset})";
const DEFAULT_INTERVAL_MS: u32 = 300_000;
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";
const REQUIRED_HOST_API: (u32, u32, u32) = (1, 0, 0);

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    credentials_path: Option<String>,
    url: Option<String>,
    format: Option<String>,
    #[serde(default, deserialize_with = "deserialize_interval_ms")]
    interval_ms: Option<u32>,
}

fn deserialize_interval_ms<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IntervalMs {
        Number(u32),
        Text(String),
    }

    let value = Option::<IntervalMs>::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        IntervalMs::Number(n) => Some(n),
        IntervalMs::Text(s) => s.parse().ok(),
    }))
}

thread_local! {
    static CREDENTIALS_PATH: RefCell<String> = RefCell::new(DEFAULT_CREDENTIALS_PATH.to_string());
    static URL: RefCell<String> = RefCell::new(DEFAULT_URL.to_string());
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
    static INTERVAL_MS: RefCell<u32> = const { RefCell::new(DEFAULT_INTERVAL_MS) };
}

mod logic {
    use super::Config;
    use serde::Deserialize;
    use time::OffsetDateTime;

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    #[derive(Deserialize)]
    struct CredentialsFile {
        #[serde(rename = "claudeAiOauth")]
        claude_ai_oauth: OauthCreds,
    }

    #[derive(Deserialize)]
    struct OauthCreds {
        #[serde(rename = "accessToken")]
        access_token: String,
    }

    pub fn extract_access_token(credentials_json: &str) -> Result<String, String> {
        serde_json::from_str::<CredentialsFile>(credentials_json)
            .map(|c| c.claude_ai_oauth.access_token)
            .map_err(|e| format!("malformed credentials file: {e}"))
    }

    pub fn build_headers(access_token: &str) -> Vec<(String, String)> {
        vec![
            (
                "Authorization".to_string(),
                format!("Bearer {access_token}"),
            ),
            (
                "anthropic-beta".to_string(),
                super::ANTHROPIC_BETA.to_string(),
            ),
        ]
    }

    pub fn http_get_payload(url: &str, headers: &[(String, String)]) -> String {
        serde_json::json!({
            "url": url,
            "headers": headers,
        })
        .to_string()
    }

    #[derive(Deserialize)]
    struct TimeStateJson {
        now_ms: u64,
    }

    pub fn parse_time_now_ms(json: &str) -> Result<u64, String> {
        serde_json::from_str::<TimeStateJson>(json)
            .map(|s| s.now_ms)
            .map_err(|e| format!("malformed time state: {e}"))
    }

    #[derive(Deserialize)]
    struct UsageResponse {
        five_hour: Window,
        seven_day: Window,
    }

    #[derive(Deserialize)]
    struct Window {
        utilization: f64,
        #[serde(default)]
        resets_at: Option<String>,
    }

    #[derive(Debug, PartialEq)]
    pub struct Usage {
        pub session_pct: f64,
        pub week_pct: f64,
        pub session_resets_at: Option<String>,
        pub week_resets_at: Option<String>,
    }

    pub fn parse_usage(body: &str) -> Result<Usage, String> {
        serde_json::from_str::<UsageResponse>(body)
            .map(|u| Usage {
                session_pct: u.five_hour.utilization,
                week_pct: u.seven_day.utilization,
                session_resets_at: u.five_hour.resets_at,
                week_resets_at: u.seven_day.resets_at,
            })
            .map_err(|e| format!("malformed usage response: {e}"))
    }

    pub fn seconds_until(resets_at: Option<&str>, now_ms: u64) -> Option<i64> {
        let resets_at = resets_at?;
        let target =
            OffsetDateTime::parse(resets_at, &time::format_description::well_known::Rfc3339)
                .ok()?;
        let now = OffsetDateTime::from_unix_timestamp_nanos(now_ms as i128 * 1_000_000)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);
        Some((target - now).whole_seconds().max(0))
    }

    pub fn format_duration(seconds: Option<i64>) -> String {
        let seconds = seconds.unwrap_or(0).max(0);
        let days = seconds / 86_400;
        let hours = (seconds % 86_400) / 3600;
        let minutes = (seconds % 3600) / 60;
        if days >= 1 {
            format!("{days}d{hours}h")
        } else if hours >= 1 {
            format!("{hours}h{minutes}m")
        } else {
            format!("{minutes}m")
        }
    }

    pub fn format_usage(
        format: &str,
        session_pct: f64,
        week_pct: f64,
        session_reset_secs: Option<i64>,
        week_reset_secs: Option<i64>,
    ) -> String {
        let session = format!("{session_pct:.0}");
        let week = format!("{week_pct:.0}");
        let session_reset = format_duration(session_reset_secs);
        let week_reset = format_duration(week_reset_secs);
        fmt_common::format_template(
            format,
            &[
                ("session", &session),
                ("week", &week),
                ("session_reset", &session_reset),
                ("week_reset", &week_reset),
            ],
        )
    }

    pub fn format_error(err: &str) -> String {
        format!("claude error: {err}")
    }
}

fn fetch_usage_text(credentials_path: &str, url: &str, format: &str) -> Result<String, String> {
    let time_json = host::call_extension("time", "now", "")?;
    let now_ms = logic::parse_time_now_ms(&time_json)?;
    let credentials = host::call_extension("fs", "read", credentials_path)?;
    let token = logic::extract_access_token(&credentials)?;
    let headers = logic::build_headers(&token);
    let payload = logic::http_get_payload(url, &headers);
    let body = host::call_extension("http", "http-get", &payload)?;
    let usage = logic::parse_usage(&body)?;
    let session_reset_secs = logic::seconds_until(usage.session_resets_at.as_deref(), now_ms);
    let week_reset_secs = logic::seconds_until(usage.week_resets_at.as_deref(), now_ms);
    Ok(logic::format_usage(
        format,
        usage.session_pct,
        usage.week_pct,
        session_reset_secs,
        week_reset_secs,
    ))
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Some(parsed) = logic::parse_config(&config) {
            let credentials_path = parsed
                .credentials_path
                .unwrap_or_else(|| DEFAULT_CREDENTIALS_PATH.to_string());
            let url = parsed.url.unwrap_or_else(|| DEFAULT_URL.to_string());
            let format = parsed.format.unwrap_or_else(|| DEFAULT_FORMAT.to_string());
            let interval_ms = parsed.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS);
            CREDENTIALS_PATH.with(|p| *p.borrow_mut() = credentials_path);
            URL.with(|u| *u.borrow_mut() = url);
            FORMAT.with(|f| *f.borrow_mut() = format);
            INTERVAL_MS.with(|i| *i.borrow_mut() = interval_ms);
        }
    }

    fn update() -> Output {
        let credentials_path = CREDENTIALS_PATH.with(|p| p.borrow().clone());
        let url = URL.with(|u| u.borrow().clone());
        let format = FORMAT.with(|f| f.borrow().clone());
        let text = match fetch_usage_text(&credentials_path, &url, &format) {
            Ok(t) => t,
            Err(err) => logic::format_error(&err),
        };
        Output {
            text,
            interval_ms: INTERVAL_MS.with(|i| *i.borrow()),
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
            ("credentials_path", DEFAULT_CREDENTIALS_PATH),
            ("url", DEFAULT_URL),
            ("format", DEFAULT_FORMAT),
            ("interval_ms", DEFAULT_INTERVAL_MS),
        ]
    }

    fn get_metadata() -> Metadata {
        Metadata {
            display_name: "Claude".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "ArtemChuban".to_string(),
        }
    }

    fn required_extensions() -> Vec<String> {
        vec!["fs".to_string(), "http".to_string(), "time".to_string()]
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::Guest;
    use super::logic::*;
    use time::OffsetDateTime;

    #[test]
    fn parses_valid_config_with_all_fields() {
        assert_eq!(
            parse_config(
                r#"{"credentials_path":"~/creds.json","url":"https://example.com","format":"{session}/{week}","interval_ms":1000}"#
            ),
            Some(Config {
                credentials_path: Some("~/creds.json".to_string()),
                url: Some("https://example.com".to_string()),
                format: Some("{session}/{week}".to_string()),
                interval_ms: Some(1000),
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_credentials_path() {
        assert_eq!(
            parse_config(r#"{"credentials_path":"~/creds.json"}"#),
            Some(Config {
                credentials_path: Some("~/creds.json".to_string()),
                url: None,
                format: None,
                interval_ms: None,
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_format() {
        assert_eq!(
            parse_config(r#"{"format":"{session}%"}"#),
            Some(Config {
                credentials_path: None,
                url: None,
                format: Some("{session}%".to_string()),
                interval_ms: None,
            })
        );
    }

    #[test]
    fn empty_json_object_yields_all_fields_none() {
        assert_eq!(
            parse_config("{}"),
            Some(Config {
                credentials_path: None,
                url: None,
                format: None,
                interval_ms: None,
            })
        );
    }

    #[test]
    fn explicit_null_fields_yield_none() {
        assert_eq!(
            parse_config(
                r#"{"credentials_path":null,"url":null,"format":null,"interval_ms":null}"#
            ),
            Some(Config {
                credentials_path: None,
                url: None,
                format: None,
                interval_ms: None,
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
        assert_eq!(parse_config(r#"{"interval_ms":{"nested":true}}"#), None);
    }

    #[test]
    fn interval_ms_accepts_numeric_string() {
        assert_eq!(
            parse_config(r#"{"interval_ms":"1000"}"#),
            Some(Config {
                credentials_path: None,
                url: None,
                format: None,
                interval_ms: Some(1000),
            })
        );
    }

    #[test]
    fn interval_ms_unparseable_string_falls_back_to_none() {
        assert_eq!(
            parse_config(r#"{"interval_ms":"soon"}"#),
            Some(Config {
                credentials_path: None,
                url: None,
                format: None,
                interval_ms: None,
            })
        );
    }

    #[test]
    fn unknown_extra_fields_are_ignored() {
        assert_eq!(
            parse_config(r#"{"format":"{session}","unexpected":true}"#),
            Some(Config {
                credentials_path: None,
                url: None,
                format: Some("{session}".to_string()),
                interval_ms: None,
            })
        );
    }

    #[test]
    fn extracts_valid_access_token() {
        assert_eq!(
            extract_access_token(r#"{"claudeAiOauth":{"accessToken":"tok-123"}}"#),
            Ok("tok-123".to_string())
        );
    }

    #[test]
    fn extract_access_token_fails_on_missing_oauth_key() {
        assert!(extract_access_token(r#"{"other":{}}"#).is_err());
    }

    #[test]
    fn extract_access_token_fails_on_missing_access_token() {
        assert!(extract_access_token(r#"{"claudeAiOauth":{}}"#).is_err());
    }

    #[test]
    fn extract_access_token_fails_on_malformed_json() {
        assert!(extract_access_token("not json").is_err());
    }

    #[test]
    fn extract_access_token_fails_on_empty_string() {
        assert!(extract_access_token("").is_err());
    }

    #[test]
    fn build_headers_returns_authorization_and_beta_header() {
        assert_eq!(
            build_headers("tok-123"),
            vec![
                ("Authorization".to_string(), "Bearer tok-123".to_string()),
                ("anthropic-beta".to_string(), "oauth-2025-04-20".to_string()),
            ]
        );
    }

    #[test]
    fn build_headers_handles_empty_token() {
        assert_eq!(
            build_headers(""),
            vec![
                ("Authorization".to_string(), "Bearer ".to_string()),
                ("anthropic-beta".to_string(), "oauth-2025-04-20".to_string()),
            ]
        );
    }

    #[test]
    fn http_get_payload_round_trips_url_and_headers() {
        let headers = build_headers("tok-123");
        let payload = http_get_payload("https://example.com/usage", &headers);
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["url"], "https://example.com/usage");
        assert_eq!(
            value["headers"],
            serde_json::json!([
                ["Authorization", "Bearer tok-123"],
                ["anthropic-beta", "oauth-2025-04-20"],
            ])
        );
    }

    #[test]
    fn parse_time_now_ms_success() {
        assert_eq!(
            parse_time_now_ms(r#"{"now_ms":123,"offset_seconds":3600}"#),
            Ok(123)
        );
        assert_eq!(parse_time_now_ms(r#"{"now_ms":123}"#), Ok(123));
    }

    #[test]
    fn parse_time_now_ms_missing_now_ms() {
        assert!(parse_time_now_ms(r#"{"offset_seconds":3600}"#).is_err());
    }

    #[test]
    fn parse_time_now_ms_garbage() {
        assert!(parse_time_now_ms("not json").is_err());
    }

    #[test]
    fn parses_valid_usage_response() {
        let body = r#"{"five_hour":{"utilization":14.0,"resets_at":"2026-08-18T20:00:00Z"},"seven_day":{"utilization":26.0}}"#;
        assert_eq!(
            parse_usage(body),
            Ok(Usage {
                session_pct: 14.0,
                week_pct: 26.0,
                session_resets_at: Some("2026-08-18T20:00:00Z".to_string()),
                week_resets_at: None,
            })
        );
    }

    #[test]
    fn parse_usage_ignores_unknown_top_level_fields() {
        let body =
            r#"{"five_hour":{"utilization":1.0},"seven_day":{"utilization":2.0},"extra":null}"#;
        assert_eq!(
            parse_usage(body),
            Ok(Usage {
                session_pct: 1.0,
                week_pct: 2.0,
                session_resets_at: None,
                week_resets_at: None,
            })
        );
    }

    #[test]
    fn parse_usage_fails_on_missing_five_hour() {
        let body = r#"{"seven_day":{"utilization":26.0}}"#;
        assert!(parse_usage(body).is_err());
    }

    #[test]
    fn parse_usage_fails_on_missing_seven_day() {
        let body = r#"{"five_hour":{"utilization":14.0}}"#;
        assert!(parse_usage(body).is_err());
    }

    #[test]
    fn parse_usage_fails_on_malformed_json() {
        assert!(parse_usage("not json").is_err());
    }

    #[test]
    fn parse_usage_accepts_integer_utilization() {
        let body = r#"{"five_hour":{"utilization":14},"seven_day":{"utilization":26}}"#;
        assert_eq!(
            parse_usage(body),
            Ok(Usage {
                session_pct: 14.0,
                week_pct: 26.0,
                session_resets_at: None,
                week_resets_at: None,
            })
        );
    }

    #[test]
    fn format_usage_substitutes_and_rounds_both_placeholders() {
        assert_eq!(
            format_usage("5h:{session}% 7d:{week}%", 14.4, 26.6, None, None),
            "5h:14% 7d:27%"
        );
    }

    #[test]
    fn format_usage_with_no_placeholders_returns_unchanged() {
        assert_eq!(
            format_usage("static text", 14.0, 26.0, None, None),
            "static text"
        );
    }

    #[test]
    fn format_usage_repeated_placeholder_replaced_everywhere() {
        assert_eq!(
            format_usage("{session}/{session}", 14.0, 26.0, None, None),
            "14/14"
        );
    }

    #[test]
    fn format_usage_zero_pads_percentages() {
        assert_eq!(
            format_usage("{session:03}%/{week:03}%", 5.0, 9.0, None, None),
            "005%/009%"
        );
    }

    #[test]
    fn format_usage_substitutes_reset_placeholders() {
        assert_eq!(
            format_usage(
                "5h:{session}%({session_reset}) 7d:{week}%({week_reset})",
                14.0,
                26.0,
                Some(4 * 3600 + 32 * 60),
                Some(3 * 86_400 + 12 * 3600),
            ),
            "5h:14%(4h32m) 7d:26%(3d12h)"
        );
    }

    #[test]
    fn format_usage_missing_reset_renders_zero_minutes() {
        assert_eq!(
            format_usage("{session_reset}/{week_reset}", 14.0, 26.0, None, None),
            "0m/0m"
        );
    }

    #[test]
    fn format_usage_repeated_reset_placeholder_replaced_everywhere() {
        assert_eq!(
            format_usage(
                "{session_reset}-{session_reset}",
                0.0,
                0.0,
                Some(90),
                Some(0)
            ),
            "1m-1m"
        );
    }

    #[test]
    fn seconds_until_computes_positive_delta() {
        let now_ms = OffsetDateTime::parse(
            "2026-08-18T19:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
        .unix_timestamp() as u64
            * 1000;
        assert_eq!(
            seconds_until(Some("2026-08-18T20:00:00Z"), now_ms),
            Some(3600)
        );
    }

    #[test]
    fn seconds_until_returns_none_for_missing_resets_at() {
        assert_eq!(seconds_until(None, 0), None);
    }

    #[test]
    fn seconds_until_returns_none_for_malformed_timestamp() {
        assert_eq!(seconds_until(Some("not a date"), 0), None);
    }

    #[test]
    fn seconds_until_clamps_past_due_to_zero() {
        let now_ms = OffsetDateTime::parse(
            "2026-08-18T20:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
        .unix_timestamp() as u64
            * 1000;
        assert_eq!(seconds_until(Some("2026-08-18T19:00:00Z"), now_ms), Some(0));
    }

    #[test]
    fn format_duration_under_a_minute_floors_to_zero_minutes() {
        assert_eq!(format_duration(Some(30)), "0m");
    }

    #[test]
    fn format_duration_under_an_hour_shows_minutes_only() {
        assert_eq!(format_duration(Some(45 * 60)), "45m");
    }

    #[test]
    fn format_duration_at_exact_one_hour_boundary_shows_hours_and_minutes() {
        assert_eq!(format_duration(Some(3600)), "1h0m");
    }

    #[test]
    fn format_duration_hours_and_minutes() {
        assert_eq!(format_duration(Some(4 * 3600 + 32 * 60)), "4h32m");
    }

    #[test]
    fn format_duration_at_exact_one_day_boundary_drops_minutes() {
        assert_eq!(format_duration(Some(86_400)), "1d0h");
    }

    #[test]
    fn format_duration_multi_day_drops_minutes() {
        assert_eq!(
            format_duration(Some(3 * 86_400 + 12 * 3600 + 59 * 60)),
            "3d12h"
        );
    }

    #[test]
    fn format_duration_none_renders_as_zero_minutes() {
        assert_eq!(format_duration(None), "0m");
    }

    #[test]
    fn format_duration_negative_seconds_clamped_to_zero_minutes() {
        assert_eq!(format_duration(Some(-100)), "0m");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(format_error("boom"), "claude error: boom");
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "claude error: ");
    }

    #[test]
    fn config_schema_declares_all_params() {
        assert_eq!(
            super::Component::config_schema(),
            vec![
                super::ConfigParam {
                    name: "credentials_path".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_CREDENTIALS_PATH.to_string(),
                },
                super::ConfigParam {
                    name: "url".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_URL.to_string(),
                },
                super::ConfigParam {
                    name: "format".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_FORMAT.to_string(),
                },
                super::ConfigParam {
                    name: "interval_ms".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_INTERVAL_MS.to_string(),
                },
            ]
        );
    }

    #[test]
    fn get_metadata_reports_display_name_version_and_author() {
        assert_eq!(
            super::Component::get_metadata(),
            super::Metadata {
                display_name: "Claude".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                author: "ArtemChuban".to_string(),
            }
        );
    }

    #[test]
    fn required_extensions_is_fs_http_time() {
        assert_eq!(
            super::Component::required_extensions(),
            vec!["fs".to_string(), "http".to_string(), "time".to_string(),]
        );
    }
}
