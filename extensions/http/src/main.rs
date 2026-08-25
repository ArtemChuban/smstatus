use std::os::unix::net::UnixListener;
use std::time::Duration;

use extension_protocol::{self as protocol, Request, Response};
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

fn handle_request(agent: &ureq::Agent, request: &Request) -> Response {
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
    let socket_path = std::env::args()
        .nth(1)
        .expect("missing socket path argument");
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("failed to bind socket");
    let (mut stream, _) = listener.accept().expect("failed to accept connection");
    protocol::perform_handshake_server(&mut stream).expect("handshake failed");

    let agent = build_http_agent();
    while let Ok(request) = protocol::read_frame::<_, Request>(&mut stream) {
        let response = handle_request(&agent, &request);
        if protocol::write_frame(&mut stream, &response).is_err() {
            break;
        }
    }
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
}
