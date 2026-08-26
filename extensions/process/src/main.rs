use std::os::unix::net::UnixListener;

use extension_protocol::{self as protocol, Request, Response};

fn is_pid_dir(file_name: &str) -> bool {
    !file_name.is_empty() && file_name.bytes().all(|b| b.is_ascii_digit())
}

fn process_name_matches(comm_contents: &str, target_name: &str) -> bool {
    const COMM_MAX_LEN: usize = 15;

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

fn process_running(name: &str) -> Result<bool, String> {
    let entries = std::fs::read_dir("/proc").map_err(|e| format!("cannot read /proc: {e}"))?;
    for entry in entries.flatten() {
        let is_pid_dir = entry.file_name().to_str().is_some_and(is_pid_dir);
        if !is_pid_dir {
            continue;
        }
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        if process_name_matches(&comm, name) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn handle_request(request: &Request) -> Response {
    if protocol::is_reserved_method(&request.method) {
        return protocol::allowlist_check_response(&request.payload, "process");
    }
    if request.method == "is-running" {
        match process_running(&request.payload) {
            Ok(running) => Response::Ok(running.to_string()),
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
    fn is_pid_dir_should_return_true_when_name_is_all_digits() {
        assert!(is_pid_dir("1234"));
    }

    #[test]
    fn is_pid_dir_should_return_false_when_name_contains_non_digit_characters() {
        assert!(!is_pid_dir("self"));
    }

    #[test]
    fn is_pid_dir_should_return_false_when_name_is_empty() {
        assert!(!is_pid_dir(""));
    }

    #[test]
    fn process_name_matches_should_return_true_when_comm_trimmed_equals_target() {
        assert!(process_name_matches("firefox\n", "firefox"));
    }

    #[test]
    fn process_name_matches_should_return_true_when_comm_is_truncated_prefix_of_long_target() {
        assert!(process_name_matches(
            "some-very-long-\n",
            "some-very-long-process-name"
        ));
    }

    #[test]
    fn process_name_matches_should_return_false_when_comm_differs_from_truncated_target() {
        assert!(!process_name_matches(
            "other-name\n",
            "some-very-long-process-name"
        ));
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
    fn is_running_returns_ok_bool_string() {
        let response = handle_request(&Request {
            method: "is-running".to_string(),
            payload: "definitely-not-a-real-process-name-xyzzy".to_string(),
        });
        match response {
            Response::Ok(body) => {
                assert!(body == "true" || body == "false");
                assert_eq!(body, "false");
            }
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn is_running_finds_self() {
        let self_name = std::fs::read_to_string(format!("/proc/{}/comm", std::process::id()))
            .unwrap()
            .trim()
            .to_string();
        let response = handle_request(&Request {
            method: "is-running".to_string(),
            payload: self_name,
        });
        match response {
            Response::Ok(body) => assert_eq!(body, "true"),
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    fn is_running_check(method: &str, allowed: &str) -> Response {
        let encoded = protocol::encode_check_payload(
            vec![protocol::PermissionEntry {
                extension: "process".to_string(),
                method: allowed.to_string(),
                constraints: Default::default(),
            }],
            method,
            "firefox",
        )
        .unwrap();
        handle_request(&Request {
            method: protocol::CHECK_METHOD.to_string(),
            payload: encoded,
        })
    }

    #[test]
    fn check_allows_allowlisted_is_running() {
        match is_running_check("is-running", "is-running") {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_unlisted_method() {
        match is_running_check("other", "is-running") {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }
}
