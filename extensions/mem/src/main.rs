use std::os::unix::net::UnixListener;

use extension_protocol::{self as protocol, Request, Response};
use serde::Serialize;

#[derive(Serialize)]
struct MemUsageJson {
    total_bytes: u64,
    used_bytes: u64,
    free_bytes: u64,
}

fn parse_meminfo_field(meminfo_content: &str, field: &str) -> Option<u64> {
    meminfo_content.lines().find_map(|line| {
        let rest = line.strip_prefix(field)?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

fn compute_mem_usage(total_kb: u64, available_kb: u64) -> MemUsageJson {
    let total_bytes = total_kb * 1024;
    let free_bytes = available_kb * 1024;
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    MemUsageJson {
        total_bytes,
        used_bytes,
        free_bytes,
    }
}

fn mem_usage_from_meminfo(meminfo: &str) -> Result<MemUsageJson, String> {
    let total_kb =
        parse_meminfo_field(meminfo, "MemTotal:").ok_or("MemTotal not found in /proc/meminfo")?;
    let available_kb = parse_meminfo_field(meminfo, "MemAvailable:")
        .ok_or("MemAvailable not found in /proc/meminfo")?;
    Ok(compute_mem_usage(total_kb, available_kb))
}

fn mem_usage() -> Result<MemUsageJson, String> {
    let meminfo = std::fs::read_to_string("/proc/meminfo")
        .map_err(|e| format!("cannot read /proc/meminfo: {e}"))?;
    mem_usage_from_meminfo(&meminfo)
}

fn mem_usage_json(usage: &MemUsageJson) -> String {
    serde_json::to_string(usage).expect("MemUsageJson always serializes")
}

fn handle_request(request: &Request) -> Response {
    if request.method == "read-mem-usage" {
        match mem_usage() {
            Ok(usage) => Response::Ok(mem_usage_json(&usage)),
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
    fn mem_usage_json_has_total_used_free() {
        let usage = MemUsageJson {
            total_bytes: 1_024_000,
            used_bytes: 614_400,
            free_bytes: 409_600,
        };
        let json = mem_usage_json(&usage);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["total_bytes"], 1_024_000u64);
        assert_eq!(value["used_bytes"], 614_400u64);
        assert_eq!(value["free_bytes"], 409_600u64);
    }

    #[test]
    fn compute_mem_usage_converts_kilobytes_to_bytes() {
        let usage = compute_mem_usage(1000, 400);
        assert_eq!(usage.total_bytes, 1_024_000);
        assert_eq!(usage.free_bytes, 409_600);
        assert_eq!(usage.used_bytes, 614_400);
    }

    #[test]
    fn compute_mem_usage_saturates_when_available_exceeds_total() {
        let usage = compute_mem_usage(100, 400);
        assert_eq!(usage.used_bytes, 0);
    }

    #[test]
    fn parse_meminfo_field_extracts_value_when_key_present() {
        let meminfo = "MemTotal:       16384000 kB\nMemFree:        2048000 kB\n";
        assert_eq!(parse_meminfo_field(meminfo, "MemTotal:"), Some(16_384_000));
    }

    #[test]
    fn mem_usage_from_meminfo_parses_known_good_text() {
        let meminfo = "MemTotal:       1000 kB\nMemAvailable:   400 kB\n";
        let usage = mem_usage_from_meminfo(meminfo).unwrap();
        assert_eq!(usage.total_bytes, 1_024_000);
        assert_eq!(usage.free_bytes, 409_600);
        assert_eq!(usage.used_bytes, 614_400);
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
    fn read_mem_usage_returns_ok_json() {
        let response = handle_request(&Request {
            method: "read-mem-usage".to_string(),
            payload: String::new(),
        });
        match response {
            Response::Ok(body) => {
                let value: serde_json::Value = serde_json::from_str(&body).unwrap();
                assert!(value.get("total_bytes").is_some());
                assert!(value.get("used_bytes").is_some());
                assert!(value.get("free_bytes").is_some());
            }
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }
}
