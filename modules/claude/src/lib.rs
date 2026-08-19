wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{Guest, HostApiVersion, Output};
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_CREDENTIALS_PATH: &str = "~/.claude/.credentials.json";
const DEFAULT_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const DEFAULT_FORMAT: &str = "5h:{session}% 7d:{week}%";
const DEFAULT_INTERVAL_MS: u32 = 300_000;
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";
const REQUIRED_HOST_API: (u32, u32, u32) = (1, 0, 0);

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    credentials_path: Option<String>,
    url: Option<String>,
    format: Option<String>,
    interval_ms: Option<u32>,
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

    #[derive(Deserialize)]
    struct UsageResponse {
        five_hour: Window,
        seven_day: Window,
    }

    #[derive(Deserialize)]
    struct Window {
        utilization: f64,
    }

    pub fn parse_usage(body: &str) -> Result<(f64, f64), String> {
        serde_json::from_str::<UsageResponse>(body)
            .map(|u| (u.five_hour.utilization, u.seven_day.utilization))
            .map_err(|e| format!("malformed usage response: {e}"))
    }

    pub fn format_usage(format: &str, session_pct: f64, week_pct: f64) -> String {
        format
            .replace("{session}", &format!("{session_pct:.0}"))
            .replace("{week}", &format!("{week_pct:.0}"))
    }

    pub fn format_error(err: &str) -> String {
        format!("claude error: {err}")
    }
}

fn fetch_usage_text(credentials_path: &str, url: &str, format: &str) -> Result<String, String> {
    let credentials = host::read_sysfs(credentials_path)?;
    let token = logic::extract_access_token(&credentials)?;
    let headers = logic::build_headers(&token);
    let body = host::http_get(url, &headers)?;
    let (session_pct, week_pct) = logic::parse_usage(&body)?;
    Ok(logic::format_usage(format, session_pct, week_pct))
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
        HostApiVersion { major, minor, patch }
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::logic::*;

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
        assert_eq!(parse_config(r#"{"interval_ms":"soon"}"#), None);
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
    fn parses_valid_usage_response() {
        let body = r#"{"five_hour":{"utilization":14.0,"resets_at":"2026-08-18T20:00:00Z"},"seven_day":{"utilization":26.0}}"#;
        assert_eq!(parse_usage(body), Ok((14.0, 26.0)));
    }

    #[test]
    fn parse_usage_ignores_unknown_top_level_fields() {
        let body =
            r#"{"five_hour":{"utilization":1.0},"seven_day":{"utilization":2.0},"extra":null}"#;
        assert_eq!(parse_usage(body), Ok((1.0, 2.0)));
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
        assert_eq!(parse_usage(body), Ok((14.0, 26.0)));
    }

    #[test]
    fn format_usage_substitutes_and_rounds_both_placeholders() {
        assert_eq!(
            format_usage("5h:{session}% 7d:{week}%", 14.4, 26.6),
            "5h:14% 7d:27%"
        );
    }

    #[test]
    fn format_usage_with_no_placeholders_returns_unchanged() {
        assert_eq!(format_usage("static text", 14.0, 26.0), "static text");
    }

    #[test]
    fn format_usage_repeated_placeholder_replaced_everywhere() {
        assert_eq!(format_usage("{session}/{session}", 14.0, 26.0), "14/14");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(format_error("boom"), "claude error: boom");
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "claude error: ");
    }
}
