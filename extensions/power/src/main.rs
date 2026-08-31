use std::collections::HashMap;
use std::io::{self, Read};
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::io::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::Duration;

use extension_protocol::{self as protocol, EventEmit, Request, Response};
use serde::Serialize;

const EXTENSION_NAME: &str = "power";
const CAPACITY_CHANGED_EVENT: &str = "capacity-changed";
const POWER_SUPPLY_ROOT: &str = "/sys/class/power_supply";

#[derive(Clone, PartialEq, Eq, Serialize)]
struct CapacityChangedJson {
    path: String,
    capacity: String,
}

fn capacity_changed_payload(event: &CapacityChangedJson) -> (String, String) {
    (
        CAPACITY_CHANGED_EVENT.to_string(),
        serde_json::to_string(event).expect("CapacityChangedJson always serializes"),
    )
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

fn resolved_keeps_power_supply(resolved: &Path) -> bool {
    resolved
        .components()
        .any(|c| c.as_os_str() == "power_supply")
}

fn path_under_prefix(path: &Path, prefix: &Path) -> bool {
    let lex_path = lexical_normalize(path);
    let lex_prefix = lexical_normalize(prefix);
    if !lex_path.starts_with(&lex_prefix) {
        return false;
    }
    match path.canonicalize() {
        Err(_) => true,
        Ok(resolved) => match prefix.canonicalize() {
            Ok(resolved_prefix) if resolved.starts_with(&resolved_prefix) => true,
            _ => resolved_keeps_power_supply(&resolved),
        },
    }
}

fn read_capacity(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))
}

fn handle_check(payload: &str) -> Response {
    let check = match protocol::decode_check_payload(payload) {
        Ok(check) => check,
        Err(msg) => return Response::Err(msg),
    };
    let entries = protocol::matching_entries(&check, EXTENSION_NAME);
    if entries.is_empty() {
        return Response::Err(format!(
            "permission denied: no allowlisted entry for extension `{EXTENSION_NAME}` method `{}`",
            check.method
        ));
    }
    if check.method != "capacity" {
        return Response::Ok(String::new());
    }
    let path = PathBuf::from(&check.payload);
    if !path.is_absolute() {
        return Response::Err(format!(
            "permission denied: path `{}` must be absolute",
            check.payload
        ));
    }
    for entry in entries {
        let prefixes = match protocol::constraint_string_list(entry, "path_prefixes") {
            Ok(prefixes) => prefixes,
            Err(msg) => return Response::Err(msg),
        };
        for prefix in prefixes {
            let prefix_path = PathBuf::from(&prefix);
            if path_under_prefix(&path, &prefix_path) {
                return Response::Ok(String::new());
            }
        }
    }
    Response::Err(format!(
        "permission denied: path `{}` is not under any allowed path_prefixes",
        check.payload
    ))
}

fn handle_request(request: &Request) -> Response {
    if protocol::is_reserved_method(&request.method) {
        return handle_check(&request.payload);
    }
    if request.method == "capacity" {
        match read_capacity(&request.payload) {
            Ok(contents) => Response::Ok(contents),
            Err(msg) => Response::Err(msg),
        }
    } else {
        Response::Err(format!("unknown method: {}", request.method))
    }
}

fn parse_uevent_fields(buf: &[u8]) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for part in buf.split(|&b| b == 0) {
        if part.is_empty() {
            continue;
        }
        let Ok(s) = std::str::from_utf8(part) else {
            continue;
        };
        if let Some((key, value)) = s.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        }
    }
    fields
}

fn capacity_path_for_uevent(fields: &HashMap<String, String>) -> Option<PathBuf> {
    if fields.get("SUBSYSTEM").map(String::as_str) != Some("power_supply") {
        return None;
    }
    let name = fields.get("POWER_SUPPLY_NAME")?;
    let path = PathBuf::from(POWER_SUPPLY_ROOT).join(name).join("capacity");
    if path.is_file() { Some(path) } else { None }
}

fn open_uevent_socket() -> io::Result<OwnedFd> {
    // SAFETY: socket() returns a new fd or -1; OwnedFd takes ownership on success.
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_KOBJECT_UEVENT,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    addr.nl_pid = 0;
    addr.nl_groups = 1;
    let rc = unsafe {
        libc::bind(
            owned.as_raw_fd(),
            &raw const addr as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(owned)
}

fn watch_capacity_changes(emit: &EventEmit) -> Result<(), String> {
    let fd = open_uevent_socket().map_err(|e| e.to_string())?;
    let mut file = unsafe { std::fs::File::from_raw_fd(fd.into_raw_fd()) };
    let mut last_by_path: HashMap<String, String> = HashMap::new();
    let mut buf = vec![0u8; 8192];

    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            continue;
        }
        let fields = parse_uevent_fields(&buf[..n]);
        let Some(path) = capacity_path_for_uevent(&fields) else {
            continue;
        };
        let path_str = path.to_string_lossy().into_owned();
        let capacity = match read_capacity(&path_str) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let changed = match last_by_path.get(&path_str) {
            Some(prev) => prev != &capacity,
            None => true,
        };
        if !changed {
            continue;
        }
        last_by_path.insert(path_str.clone(), capacity.clone());
        let event = CapacityChangedJson {
            path: path_str,
            capacity,
        };
        let (name, payload) = capacity_changed_payload(&event);
        emit.emit(name, payload)?;
    }
}

fn spawn_capacity_watcher(emit: EventEmit) {
    thread::spawn(move || {
        loop {
            if let Err(_err) = watch_capacity_changes(&emit) {
                thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

fn main() {
    protocol::serve_with_events(|emit| {
        spawn_capacity_watcher(emit);
        handle_request
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_changed_event_uses_json_shape() {
        let event = CapacityChangedJson {
            path: "/sys/class/power_supply/BAT1/capacity".to_string(),
            capacity: "42".to_string(),
        };
        let (name, payload) = capacity_changed_payload(&event);
        assert_eq!(name, "capacity-changed");
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["path"], "/sys/class/power_supply/BAT1/capacity");
        assert_eq!(value["capacity"], "42");
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
    fn capacity_returns_file_contents_for_existing_path() {
        let dir = std::env::temp_dir().join("smstatus-power-ok");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("capacity");
        std::fs::write(&path, "87\n").unwrap();

        let response = handle_request(&Request {
            method: "capacity".to_string(),
            payload: path.to_string_lossy().into_owned(),
        });
        match response {
            Response::Ok(body) => assert_eq!(body, "87\n"),
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn capacity_returns_err_for_missing_path() {
        let response = handle_request(&Request {
            method: "capacity".to_string(),
            payload: "/no/such/path/for/smstatus-power".to_string(),
        });
        match response {
            Response::Err(msg) => assert!(msg.starts_with("read failed:")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    fn capacity_check(path: &str, prefixes: &[&str]) -> Response {
        let prefixes_json = prefixes
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(",");
        let encoded = format!(
            r#"{{"permissions":[{{"extension":"power","method":"capacity","constraints":{{"path_prefixes":[{prefixes_json}]}}}}],"method":"capacity","payload":"{path}"}}"#
        );
        handle_request(&Request {
            method: protocol::CHECK_METHOD.to_string(),
            payload: encoded,
        })
    }

    fn event_check(method: &str, allowed: &str) -> Response {
        handle_request(&protocol::check_request("power", method, "", allowed))
    }

    #[test]
    fn check_allows_capacity_under_prefix() {
        match capacity_check(
            "/sys/class/power_supply/BAT0/capacity",
            &["/sys/class/power_supply/"],
        ) {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_capacity_outside_prefix() {
        match capacity_check("/etc/passwd", &["/sys/class/power_supply/"]) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn check_denies_relative_capacity_path() {
        match capacity_check(
            "sys/class/power_supply/BAT0/capacity",
            &["/sys/class/power_supply/"],
        ) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err for relative path"),
        }
    }

    #[test]
    fn check_denies_dotdot_escape_outside_prefix() {
        match capacity_check(
            "/sys/class/power_supply/../passwd",
            &["/sys/class/power_supply/"],
        ) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err for .. escape"),
        }
    }

    #[test]
    fn check_allows_sysfs_class_symlink_layout() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join("smstatus-power-sysfs-class");
        let _ = std::fs::remove_dir_all(&root);
        let devices = root
            .join("devices")
            .join("LNXSYSTM:00")
            .join("power_supply")
            .join("BAT0");
        let class = root.join("class").join("power_supply");
        std::fs::create_dir_all(&devices).unwrap();
        std::fs::create_dir_all(&class).unwrap();
        std::fs::write(devices.join("capacity"), "87\n").unwrap();
        symlink(&devices, class.join("BAT0")).unwrap();

        // Pretend class dir is /sys/class/power_supply via a prefix rooted at `class`.
        // The request path goes through the class symlink; resolve lands under devices.
        let prefix = format!("{}/", class.to_string_lossy());
        let path = class.join("BAT0").join("capacity");
        let path = path.to_string_lossy().into_owned();
        match capacity_check(&path, &[&prefix]) {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok for class symlink layout, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_symlink_escape_outside_prefix() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join("smstatus-power-symlink-escape");
        let allowed = root.join("allowed");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&allowed).unwrap();
        let secret = root.join("secret.txt");
        std::fs::write(&secret, "nope\n").unwrap();
        let link = allowed.join("escape");
        symlink(&secret, &link).unwrap();

        let prefix = format!("{}/", allowed.to_string_lossy());
        let path = link.to_string_lossy().into_owned();
        match capacity_check(&path, &[&prefix]) {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err for symlink escape"),
        }
    }

    #[test]
    fn check_allows_capacity_changed_event_without_path() {
        match event_check("capacity-changed", "capacity-changed") {
            Response::Ok(_) => {}
            Response::Err(msg) => panic!("expected Ok, got Err: {msg}"),
        }
    }

    #[test]
    fn check_denies_unlisted_method() {
        match event_check("other", "capacity") {
            Response::Err(msg) => assert!(msg.contains("permission denied")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn parse_uevent_fields_extracts_key_values() {
        let raw = b"change@/devices/LNXSYSTM:00/power_supply/BAT0\0\
SUBSYSTEM=power_supply\0\
POWER_SUPPLY_NAME=BAT0\0\
ACTION=change\0";
        let fields = parse_uevent_fields(raw);
        assert_eq!(
            fields.get("SUBSYSTEM").map(String::as_str),
            Some("power_supply")
        );
        assert_eq!(
            fields.get("POWER_SUPPLY_NAME").map(String::as_str),
            Some("BAT0")
        );
        assert_eq!(fields.get("ACTION").map(String::as_str), Some("change"));
    }
}
