use std::collections::HashMap;

use extension_protocol::{self as protocol, Request, Response};

const EXTENSION_NAME: &str = "{{name}}";

fn check(request: &Request) -> Response {
    protocol::allowlist_check_response(&request.payload, EXTENSION_NAME)
}

fn ping(request: &Request) -> Response {
    Response::Ok(request.payload.clone())
}

fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .expect("missing socket path argument");
    let mut handlers = HashMap::new();
    handlers.insert("ping".to_string(), ping as protocol::MethodHandler);
    protocol::run_unix_extension_server(&socket_path, handlers, Some(check), None)
        .expect("extension server failed");
}
