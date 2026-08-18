use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::bindings::{DiskUsage, MemUsage, TimeState};
use crate::error::Result;

pub(crate) fn expand_sysfs_path(path: &str, home_dir: Option<&Path>) -> Result<PathBuf> {
    match path.strip_prefix("~/") {
        Some(rest) => {
            let home = home_dir.ok_or_else(|| "could not determine home directory".to_string())?;
            Ok(home.join(rest))
        }
        None => Ok(PathBuf::from(path)),
    }
}

pub(crate) fn read_sysfs(path: &str) -> Result<String> {
    let expanded = expand_sysfs_path(path, dirs::home_dir().as_deref())?;
    std::fs::read_to_string(&expanded).map_err(|e| format!("read failed: {e}").into())
}

pub(crate) fn time_state() -> TimeState {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64;
    let offset_seconds = chrono::Local::now().offset().local_minus_utc();
    TimeState {
        now_ms,
        offset_seconds,
    }
}

pub(crate) fn find_mount_point(mounts_content: &str, target: &Path) -> Option<String> {
    mounts_content.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let dev = fields.next()?;
        let mount_point = fields.next()?;
        let dev_canon = std::fs::canonicalize(dev).unwrap_or_else(|_| PathBuf::from(dev));
        (dev_canon.as_path() == target).then(|| mount_point.to_string())
    })
}

pub(crate) fn compute_disk_usage(
    total_blocks: u64,
    free_blocks: u64,
    block_size: u64,
) -> DiskUsage {
    let total_bytes = total_blocks * block_size;
    let free_bytes = free_blocks * block_size;
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    DiskUsage {
        total_bytes,
        used_bytes,
        free_bytes,
    }
}

pub(crate) fn disk_usage(device: &str) -> Result<DiskUsage> {
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

pub(crate) fn parse_meminfo_field(meminfo_content: &str, field: &str) -> Option<u64> {
    meminfo_content.lines().find_map(|line| {
        let rest = line.strip_prefix(field)?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

pub(crate) fn compute_mem_usage(total_kb: u64, available_kb: u64) -> MemUsage {
    let total_bytes = total_kb * 1024;
    let free_bytes = available_kb * 1024;
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    MemUsage {
        total_bytes,
        used_bytes,
        free_bytes,
    }
}

pub(crate) fn mem_usage() -> Result<MemUsage> {
    let meminfo = std::fs::read_to_string("/proc/meminfo")
        .map_err(|e| format!("cannot read /proc/meminfo: {e}"))?;

    let total_kb =
        parse_meminfo_field(&meminfo, "MemTotal:").ok_or("MemTotal not found in /proc/meminfo")?;
    let available_kb = parse_meminfo_field(&meminfo, "MemAvailable:")
        .ok_or("MemAvailable not found in /proc/meminfo")?;

    Ok(compute_mem_usage(total_kb, available_kb))
}

pub(crate) fn is_pid_dir(file_name: &str) -> bool {
    !file_name.is_empty() && file_name.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn process_name_matches(comm_contents: &str, target_name: &str) -> bool {
    comm_contents.trim() == target_name
}

pub(crate) fn process_running(name: &str) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_sysfs_path_should_join_home_dir_when_path_starts_with_tilde_slash() {
        let home = Path::new("/home/user");
        let result = expand_sysfs_path("~/foo/bar", Some(home)).unwrap();
        assert_eq!(result, PathBuf::from("/home/user/foo/bar"));
    }

    #[test]
    fn expand_sysfs_path_should_return_error_when_tilde_path_given_and_no_home_dir() {
        assert!(expand_sysfs_path("~/foo", None).is_err());
    }

    #[test]
    fn expand_sysfs_path_should_return_path_unchanged_when_path_has_no_tilde_prefix() {
        let result = expand_sysfs_path("/sys/class/power_supply/BAT0", None).unwrap();
        assert_eq!(result, PathBuf::from("/sys/class/power_supply/BAT0"));
    }

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
    fn compute_mem_usage_should_convert_kilobytes_to_bytes() {
        let usage = compute_mem_usage(1000, 400);
        assert_eq!(usage.total_bytes, 1_024_000);
    }

    #[test]
    fn compute_mem_usage_should_saturate_at_zero_when_available_exceeds_total() {
        let usage = compute_mem_usage(100, 400);
        assert_eq!(usage.used_bytes, 0);
    }

    #[test]
    fn parse_meminfo_field_should_extract_value_when_key_present() {
        let meminfo = "MemTotal:       16384000 kB\nMemFree:        2048000 kB\n";
        assert_eq!(parse_meminfo_field(meminfo, "MemTotal:"), Some(16_384_000));
    }

    #[test]
    fn parse_meminfo_field_should_return_none_when_key_missing() {
        let meminfo = "MemFree:        2048000 kB\n";
        assert_eq!(parse_meminfo_field(meminfo, "MemTotal:"), None);
    }

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
}
