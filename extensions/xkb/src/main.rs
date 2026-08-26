use std::os::unix::net::UnixListener;

use extension_protocol::{self as protocol, Request, Response};
use serde::Serialize;
use x11rb::protocol::xkb;
use x11rb::protocol::xkb::ConnectionExt as _;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

#[derive(Serialize)]
struct XkbStateJson {
    active_group: u8,
    symbols: String,
}

fn xkb_state_json(state: &XkbStateJson) -> String {
    serde_json::to_string(state).expect("XkbStateJson always serializes")
}

fn read_xkb_state_from(connection: &RustConnection) -> Result<XkbStateJson, String> {
    let group = connection
        .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .group;

    let names_reply = connection
        .xkb_get_names(xkb::ID::USE_CORE_KBD.into(), xkb::NameDetail::SYMBOLS)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;

    let symbols_atom = names_reply
        .value_list
        .symbols_name
        .ok_or_else(|| "no symbols name reported".to_string())?;

    let symbols = connection
        .get_atom_name(symbols_atom)
        .map_err(|e| e.to_string())?
        .reply()
        .map(|r| String::from_utf8_lossy(&r.name).into_owned())
        .map_err(|e| e.to_string())?;

    Ok(XkbStateJson {
        active_group: u8::from(group),
        symbols,
    })
}

fn connect_xkb() -> Result<RustConnection, String> {
    let (connection, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
    connection
        .xkb_use_extension(1, 0)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    Ok(connection)
}

fn read_with_cached_connection(
    cached: &mut Option<RustConnection>,
) -> Result<XkbStateJson, String> {
    if cached.is_none() {
        *cached = Some(connect_xkb()?);
    }
    let result = read_xkb_state_from(cached.as_ref().expect("connection set above"));
    if result.is_err() {
        *cached = None;
    }
    result
}

fn handle_request_with(
    request: &Request,
    mut read: impl FnMut() -> Result<XkbStateJson, String>,
) -> Response {
    if protocol::is_reserved_method(&request.method) {
        return protocol::allowlist_check_response(&request.payload, "xkb");
    }
    if request.method == "state" {
        match read() {
            Ok(state) => Response::Ok(xkb_state_json(&state)),
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

    let mut connection = None;
    while let Ok(request) = protocol::read_frame::<_, Request>(&mut stream) {
        let response =
            handle_request_with(&request, || read_with_cached_connection(&mut connection));
        if protocol::write_frame(&mut stream, &response).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xkb_state_json_has_active_group_and_symbols() {
        let state = XkbStateJson {
            active_group: 1,
            symbols: "pc+us+ru:2".to_string(),
        };
        let json = xkb_state_json(&state);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["active_group"], 1);
        assert_eq!(value["symbols"], "pc+us+ru:2");
    }

    #[test]
    fn unknown_method_returns_err() {
        let response = handle_request_with(
            &Request {
                method: "nope".to_string(),
                payload: String::new(),
            },
            || unreachable!("read should not be called for unknown method"),
        );
        match response {
            Response::Err(msg) => assert!(msg.contains("unknown method")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn state_returns_ok_json_when_injected_ok() {
        let response = handle_request_with(
            &Request {
                method: "state".to_string(),
                payload: String::new(),
            },
            || {
                Ok(XkbStateJson {
                    active_group: 0,
                    symbols: "pc+us".to_string(),
                })
            },
        );
        match response {
            Response::Ok(body) => {
                let value: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert_eq!(value["active_group"], 0);
                assert_eq!(value["symbols"], "pc+us");
            }
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn state_returns_err_when_injected_err() {
        let response = handle_request_with(
            &Request {
                method: "state".to_string(),
                payload: String::new(),
            },
            || Err("no display".to_string()),
        );
        match response {
            Response::Err(msg) => assert_eq!(msg, "no display"),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    fn state_check(method: &str, allowed: &str) -> Response {
        let encoded = protocol::encode_check_payload(
            vec![protocol::PermissionEntry {
                extension: "xkb".to_string(),
                method: allowed.to_string(),
                constraints: Default::default(),
            }],
            method,
            "",
        )
        .unwrap();
        handle_request_with(
            &Request {
                method: protocol::CHECK_METHOD.to_string(),
                payload: encoded,
            },
            || unreachable!("read should not be called for check"),
        )
    }

    #[test]
    fn check_allows_allowlisted_state() {
        match state_check("state", "state") {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_unlisted_method() {
        match state_check("other", "state") {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }
}
