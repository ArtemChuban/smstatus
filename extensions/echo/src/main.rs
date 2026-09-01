use extension_protocol::{self as protocol, Request, Response};

const EXTENSION_NAME: &str = "echo";

fn handle_request(request: &Request) -> Response {
    if protocol::is_reserved_method(&request.method) {
        return protocol::allowlist_check_response(&request.payload, EXTENSION_NAME);
    }
    Response::Ok(request.payload.clone())
}

fn main() {
    protocol::serve(handle_request);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use extension_protocol::PermissionEntry;

    use super::*;

    fn check_request(method: &str, payload: &str, allowed_method: &str) -> Request {
        let encoded = protocol::encode_check_payload(
            vec![PermissionEntry {
                extension: EXTENSION_NAME.to_string(),
                method: allowed_method.to_string(),
                constraints: BTreeMap::new(),
            }],
            method,
            payload,
        )
        .unwrap();
        Request {
            method: protocol::CHECK_METHOD.to_string(),
            payload: encoded,
        }
    }

    #[test]
    fn check_allows_allowlisted_method() {
        let response = handle_request(&check_request("ping", "hi", "ping"));
        match response {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_unlisted_method() {
        let response = handle_request(&check_request("pong", "hi", "ping"));
        match response {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn non_check_call_echoes_payload() {
        let response = handle_request(&Request {
            method: "ping".to_string(),
            payload: "hello".to_string(),
        });
        match response {
            Response::Ok(body) => assert_eq!(body, "hello"),
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }
}
