use std::os::unix::net::UnixListener;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use extension_protocol::{self as protocol, Request, Response};
use serde::Serialize;

#[derive(Serialize)]
struct TimeStateJson {
    now_ms: u64,
    offset_seconds: i32,
}

fn wall_clock_state() -> TimeStateJson {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;
    let offset_seconds = chrono::Local::now().offset().local_minus_utc();
    TimeStateJson {
        now_ms,
        offset_seconds,
    }
}

fn time_state_json(state: &TimeStateJson) -> String {
    serde_json::to_string(state).expect("TimeStateJson always serializes")
}

fn handle_request(request: &Request) -> Response {
    if request.method == "read-time-state" {
        Response::Ok(time_state_json(&wall_clock_state()))
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

    while let Ok(request) = protocol::read_frame::<_, Request>(&mut stream) {
        let response = handle_request(&request);
        if protocol::write_frame(&mut stream, &response).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_state_json_has_now_ms_and_offset() {
        let state = TimeStateJson {
            now_ms: 1_700_000_000_000,
            offset_seconds: 3600,
        };
        let json = time_state_json(&state);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["now_ms"], 1_700_000_000_000u64);
        assert_eq!(value["offset_seconds"], 3600);
    }

    #[test]
    fn wall_clock_state_fields_are_sane() {
        let state = wall_clock_state();
        assert!(state.now_ms > 0);
        assert!((-14 * 3600..=14 * 3600).contains(&state.offset_seconds));
    }

    #[test]
    fn unknown_method_returns_err() {
        let response = handle_request(&Request {
            method: "nope".to_string(),
            payload: String::new(),
        });
        match response {
            Response::Err(msg) => assert!(msg.contains("unknown method")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn read_time_state_returns_ok_json() {
        let response = handle_request(&Request {
            method: "read-time-state".to_string(),
            payload: String::new(),
        });
        match response {
            Response::Ok(body) => {
                let value: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert!(value.get("now_ms").is_some());
                assert!(value.get("offset_seconds").is_some());
            }
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }
}
