use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use extension_protocol::{self as protocol, Request, Response};
use serde::Serialize;

#[derive(Serialize)]
struct DiskUsage {
    total_bytes: u64,
    used_bytes: u64,
    free_bytes: u64,
}

fn find_mount_point(mounts_content: &str, target: &Path) -> Option<String> {
    mounts_content.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let dev = fields.next()?;
        let mount_point = fields.next()?;
        let dev_canon = std::fs::canonicalize(dev).unwrap_or_else(|_| PathBuf::from(dev));
        (dev_canon.as_path() == target).then(|| mount_point.to_string())
    })
}

fn compute_disk_usage(total_blocks: u64, free_blocks: u64, block_size: u64) -> DiskUsage {
    let total_bytes = total_blocks * block_size;
    let free_bytes = free_blocks * block_size;
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    DiskUsage {
        total_bytes,
        used_bytes,
        free_bytes,
    }
}

fn disk_usage(device: &str) -> Result<DiskUsage, String> {
    let target = std::fs::canonicalize(device).unwrap_or_else(|_| PathBuf::from(device));

    let mounts_content = std::fs::read_to_string("/proc/mounts")
        .map_err(|e| format!("cannot read /proc/mounts: {e}"))?;
    let mount_point = find_mount_point(&mounts_content, &target)
        .ok_or_else(|| format!("device `{device}` not found in /proc/mounts"))?;

    let stat = nix::sys::statvfs::statvfs(mount_point.as_str())
        .map_err(|e| format!("statvfs failed for `{mount_point}`: {e}"))?;
    let block_size = stat.fragment_size();

    Ok(compute_disk_usage(
        stat.blocks(),
        stat.blocks_free(),
        block_size,
    ))
}

fn disk_usage_json(usage: &DiskUsage) -> String {
    serde_json::to_string(usage).expect("DiskUsage always serializes")
}

fn handle_request(request: &Request) -> Response {
    if request.method == "read-disk-usage" {
        match disk_usage(&request.payload) {
            Ok(usage) => Response::Ok(disk_usage_json(&usage)),
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
    fn compute_disk_usage_should_subtract_free_from_total_when_computing_used_bytes() {
        let usage = compute_disk_usage(100, 40, 1);
        assert_eq!(usage.used_bytes, 60);
    }

    #[test]
    fn compute_disk_usage_should_saturate_at_zero_when_free_exceeds_total() {
        let usage = compute_disk_usage(10, 40, 1);
        assert_eq!(usage.used_bytes, 0);
    }

    #[test]
    fn find_mount_point_should_return_mount_point_when_device_matches_a_line() {
        let mounts = "/dev/sda1 / ext4 rw 0 0\n/dev/sda2 /home ext4 rw 0 0\n";
        let target = Path::new("/dev/sda2");
        assert_eq!(find_mount_point(mounts, target), Some("/home".to_string()));
    }

    #[test]
    fn find_mount_point_should_return_none_when_device_not_present() {
        let mounts = "/dev/sda1 / ext4 rw 0 0\n";
        let target = Path::new("/dev/sda9");
        assert_eq!(find_mount_point(mounts, target), None);
    }

    #[test]
    fn disk_usage_json_has_byte_fields() {
        let usage = DiskUsage {
            total_bytes: 1000,
            used_bytes: 400,
            free_bytes: 600,
        };
        let json = disk_usage_json(&usage);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["total_bytes"], 1000);
        assert_eq!(value["used_bytes"], 400);
        assert_eq!(value["free_bytes"], 600);
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
    fn read_disk_usage_returns_err_for_missing_device() {
        let response = handle_request(&Request {
            method: "read-disk-usage".to_string(),
            payload: "/dev/smstatus-disk-missing-device".to_string(),
        });
        match response {
            Response::Err(msg) => assert!(msg.contains("not found")),
            Response::Ok(_) => panic!("expected Err"),
        }
    }
}
