use std::str::FromStr;
use std::time::Duration;

use extension_protocol::{self as protocol, Request, Response};
use http::Uri;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct HttpGetPayload {
    url: String,
    headers: Vec<(String, String)>,
}

fn build_http_agent() -> ureq::Agent {
    let agent_config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    ureq::Agent::new_with_config(agent_config)
}

fn parse_http_get_payload(payload: &str) -> Result<HttpGetPayload, String> {
    serde_json::from_str(payload).map_err(|e| format!("malformed payload: {e}"))
}

fn effective_port(scheme: &str, port: Option<u16>) -> Option<u16> {
    match port {
        Some(p) => Some(p),
        None if scheme.eq_ignore_ascii_case("https") => Some(443),
        None if scheme.eq_ignore_ascii_case("http") => Some(80),
        None => None,
    }
}

fn path_and_query(uri: &Uri) -> String {
    match (uri.path(), uri.query()) {
        (path, Some(query)) => format!("{path}?{query}"),
        (path, None) => path.to_string(),
    }
}

fn url_under_prefix(url: &str, prefix: &str) -> Result<bool, String> {
    let url = Uri::from_str(url).map_err(|e| format!("invalid url: {e}"))?;
    let prefix = Uri::from_str(prefix).map_err(|e| format!("invalid url prefix: {e}"))?;

    let url_scheme = url.scheme_str().unwrap_or("");
    let prefix_scheme = prefix.scheme_str().unwrap_or("");
    if !url_scheme.eq_ignore_ascii_case(prefix_scheme) {
        return Ok(false);
    }

    let url_auth = url
        .authority()
        .ok_or_else(|| "url missing host".to_string())?;
    let prefix_auth = prefix
        .authority()
        .ok_or_else(|| "url prefix missing host".to_string())?;
    if !url_auth.host().eq_ignore_ascii_case(prefix_auth.host()) {
        return Ok(false);
    }
    if effective_port(url_scheme, url_auth.port_u16())
        != effective_port(prefix_scheme, prefix_auth.port_u16())
    {
        return Ok(false);
    }

    let url_pq = path_and_query(&url);
    let prefix_pq = path_and_query(&prefix);
    if prefix_pq.is_empty() || prefix_pq == "/" {
        return Ok(url_pq.starts_with('/'));
    }
    if !url_pq.starts_with(&prefix_pq) {
        return Ok(false);
    }
    if prefix_pq.ends_with('/') {
        return Ok(true);
    }
    Ok(matches!(
        url_pq.as_bytes().get(prefix_pq.len()),
        None | Some(b'/') | Some(b'?')
    ))
}

fn http_get(agent: &ureq::Agent, payload: &HttpGetPayload) -> Result<String, String> {
    let mut request = agent.get(&payload.url);
    for (name, value) in &payload.headers {
        request = request.header(name, value);
    }
    let mut response = request
        .call()
        .map_err(|e| format!("http request failed: {e}"))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("failed to read response body: {e}"))
}

fn handle_check(payload: &str) -> Response {
    let check = match protocol::decode_check_payload(payload) {
        Ok(check) => check,
        Err(msg) => return Response::Err(msg),
    };
    let entries = protocol::matching_entries(&check, "http");
    if entries.is_empty() {
        return Response::Err(format!(
            "permission denied: no allowlisted entry for extension `http` method `{}`",
            check.method
        ));
    }
    if check.method != "get" {
        return Response::Ok(String::new());
    }
    let get_payload = match parse_http_get_payload(&check.payload) {
        Ok(payload) => payload,
        Err(msg) => return Response::Err(msg),
    };
    for entry in entries {
        let prefixes = match protocol::constraint_string_list(entry, "url_prefixes") {
            Ok(prefixes) => prefixes,
            Err(msg) => return Response::Err(msg),
        };
        for prefix in prefixes {
            match url_under_prefix(&get_payload.url, &prefix) {
                Ok(true) => return Response::Ok(String::new()),
                Ok(false) => {}
                Err(msg) => return Response::Err(msg),
            }
        }
    }
    Response::Err(format!(
        "permission denied: url `{}` is not under any allowed url_prefixes",
        get_payload.url
    ))
}

fn handle_request(agent: &ureq::Agent, request: &Request) -> Response {
    if protocol::is_reserved_method(&request.method) {
        return handle_check(&request.payload);
    }
    if request.method == "get" {
        match parse_http_get_payload(&request.payload) {
            Ok(payload) => match http_get(agent, &payload) {
                Ok(body) => Response::Ok(body),
                Err(msg) => Response::Err(msg),
            },
            Err(msg) => Response::Err(msg),
        }
    } else {
        Response::Err(format!("unknown method: {}", request.method))
    }
}

fn main() {
    let agent = build_http_agent();
    protocol::serve(move |request| handle_request(&agent, request));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_get_payload_success() {
        let payload = r#"{"url":"https://example.com","headers":[["Authorization","Bearer x"]]}"#;
        let parsed = parse_http_get_payload(payload).unwrap();
        assert_eq!(parsed.url, "https://example.com");
        assert_eq!(
            parsed.headers,
            vec![("Authorization".to_string(), "Bearer x".to_string())]
        );
    }

    #[test]
    fn parse_http_get_payload_missing_fields() {
        let err = parse_http_get_payload(r#"{"url":"https://example.com"}"#).unwrap_err();
        assert!(err.starts_with("malformed payload:"));
    }

    #[test]
    fn parse_http_get_payload_garbage() {
        let err = parse_http_get_payload("not-json").unwrap_err();
        assert!(err.starts_with("malformed payload:"));
    }

    #[test]
    fn unknown_method_returns_err() {
        let agent = build_http_agent();
        let response = handle_request(
            &agent,
            &Request {
                method: "nope".to_string(),
                payload: String::new(),
            },
        );
        match response {
            Response::Err(msg) => assert!(msg.contains("unknown method")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn http_get_returns_err_for_invalid_url() {
        let agent = build_http_agent();
        let response = handle_request(
            &agent,
            &Request {
                method: "get".to_string(),
                payload: r#"{"url":"not-a-valid-url","headers":[]}"#.to_string(),
            },
        );
        match response {
            Response::Err(msg) => assert!(msg.starts_with("http request failed:")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    fn get_check(url: &str, prefixes: &[&str]) -> Response {
        let prefixes_json = serde_json::to_string(prefixes).unwrap();
        let get_payload = serde_json::json!({
            "url": url,
            "headers": []
        });
        let encoded = serde_json::json!({
            "permissions": [{
                "extension": "http",
                "method": "get",
                "constraints": {
                    "url_prefixes": serde_json::from_str::<serde_json::Value>(&prefixes_json).unwrap()
                }
            }],
            "method": "get",
            "payload": get_payload.to_string()
        })
        .to_string();
        handle_check(&encoded)
    }

    #[test]
    fn check_allows_url_under_prefix() {
        match get_check(
            "https://api.anthropic.com/v1/messages",
            &["https://api.anthropic.com/"],
        ) {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_url_outside_prefix() {
        match get_check("https://evil.test/", &["https://api.anthropic.com/"]) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn check_denies_lookalike_host_even_without_trailing_slash() {
        match get_check(
            "https://api.anthropic.com.evil.com/v1",
            &["https://api.anthropic.com"],
        ) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err for lookalike host"),
        }
    }

    #[test]
    fn check_allows_path_under_prefix_without_trailing_slash() {
        match get_check(
            "https://api.anthropic.com/v1/messages",
            &["https://api.anthropic.com"],
        ) {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }
}
