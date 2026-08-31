use std::collections::HashMap;
use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::thread;
use std::time::Duration;

use extension_protocol::{self as protocol, EventEmit, Request, Response};
use serde::Serialize;

const RUNNING_CHANGED_EVENT: &str = "running-changed";
const COMM_MAX_LEN: usize = 15;

#[derive(Clone, PartialEq, Eq, Serialize)]
struct RunningChanged {
    process: String,
    running: bool,
}

fn running_changed_json(event: &RunningChanged) -> String {
    serde_json::to_string(event).expect("RunningChanged always serializes")
}

fn running_changed_payload(event: &RunningChanged) -> (String, String) {
    (
        RUNNING_CHANGED_EVENT.to_string(),
        running_changed_json(event),
    )
}

fn is_pid_dir(file_name: &str) -> bool {
    !file_name.is_empty() && file_name.bytes().all(|b| b.is_ascii_digit())
}

fn process_name_matches(comm_contents: &str, target_name: &str) -> bool {
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

fn read_comm(pid: i32) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let trimmed = comm.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
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

struct ProcessTracker {
    pids: HashMap<i32, String>,
    counts: HashMap<String, usize>,
}

impl ProcessTracker {
    fn new() -> Self {
        Self {
            pids: HashMap::new(),
            counts: HashMap::new(),
        }
    }

    fn bump(counts: &mut HashMap<String, usize>, name: &str, delta: isize) -> Option<bool> {
        let entry = counts.entry(name.to_string()).or_insert(0);
        let before = *entry;
        let after = before as isize + delta;
        if after < 0 {
            *entry = 0;
            return None;
        }
        *entry = after as usize;
        if before == 0 && after > 0 {
            Some(true)
        } else if before > 0 && after == 0 {
            Some(false)
        } else {
            None
        }
    }

    fn appear(&mut self, pid: i32, comm: String) -> Vec<(String, bool)> {
        if self.pids.contains_key(&pid) {
            return self.rename(pid, comm);
        }
        self.pids.insert(pid, comm.clone());
        match Self::bump(&mut self.counts, &comm, 1) {
            Some(running) => vec![(comm, running)],
            None => Vec::new(),
        }
    }

    fn disappear(&mut self, pid: i32) -> Vec<(String, bool)> {
        let Some(comm) = self.pids.remove(&pid) else {
            return Vec::new();
        };
        match Self::bump(&mut self.counts, &comm, -1) {
            Some(running) => vec![(comm, running)],
            None => Vec::new(),
        }
    }

    fn rename(&mut self, pid: i32, new_comm: String) -> Vec<(String, bool)> {
        let Some(old_comm) = self.pids.get(&pid).cloned() else {
            return self.appear(pid, new_comm);
        };
        if old_comm == new_comm {
            return Vec::new();
        }
        let mut flips = Vec::new();
        if let Some(running) = Self::bump(&mut self.counts, &old_comm, -1) {
            flips.push((old_comm, running));
        }
        self.pids.insert(pid, new_comm.clone());
        if let Some(running) = Self::bump(&mut self.counts, &new_comm, 1) {
            flips.push((new_comm, running));
        }
        flips
    }

    fn seed_from_proc(&mut self) {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !is_pid_dir(&name) {
                continue;
            }
            let Ok(pid) = name.parse::<i32>() else {
                continue;
            };
            let Some(comm) = read_comm(pid) else {
                continue;
            };
            let _ = self.appear(pid, comm);
        }
    }
}

#[repr(C)]
struct CbId {
    idx: u32,
    val: u32,
}

#[repr(C)]
struct CnMsg {
    id: CbId,
    seq: u32,
    ack: u32,
    len: u16,
    flags: u16,
}

#[repr(C)]
struct ListenMsg {
    nl: libc::nlmsghdr,
    cn: CnMsg,
    op: u32,
}

fn open_proc_connector() -> Result<OwnedFd, String> {
    let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM, libc::NETLINK_CONNECTOR) };
    if fd < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };

    let mut addr: libc::sockaddr_nl = unsafe { mem::zeroed() };
    addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    addr.nl_groups = libc::CN_IDX_PROC;

    let rc = unsafe {
        libc::bind(
            owned.as_raw_fd(),
            &addr as *const _ as *const libc::sockaddr,
            mem::size_of_val(&addr) as libc::socklen_t,
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error().to_string());
    }

    let msg = ListenMsg {
        nl: libc::nlmsghdr {
            nlmsg_len: mem::size_of::<ListenMsg>() as u32,
            nlmsg_type: libc::NLMSG_DONE as u16,
            nlmsg_flags: libc::NLM_F_REQUEST as u16,
            nlmsg_seq: 0,
            nlmsg_pid: 0,
        },
        cn: CnMsg {
            id: CbId {
                idx: libc::CN_IDX_PROC,
                val: libc::CN_VAL_PROC,
            },
            seq: 0,
            ack: 0,
            len: mem::size_of::<u32>() as u16,
            flags: 0,
        },
        op: libc::PROC_CN_MCAST_LISTEN,
    };

    let sent = unsafe {
        libc::send(
            owned.as_raw_fd(),
            &msg as *const _ as *const libc::c_void,
            mem::size_of_val(&msg),
            0,
        )
    };
    if sent < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(owned)
}

fn emit_flips(
    emit: &EventEmit,
    flips: impl IntoIterator<Item = (String, bool)>,
) -> Result<(), String> {
    for (process, running) in flips {
        let (name, payload) = running_changed_payload(&RunningChanged { process, running });
        emit.emit(name, payload)?;
    }
    Ok(())
}

fn handle_proc_event(tracker: &mut ProcessTracker, what: u32, data: &[u8]) -> Vec<(String, bool)> {
    match what {
        libc::PROC_EVENT_FORK => {
            if data.len() < 16 {
                return Vec::new();
            }
            let parent_pid = i32::from_ne_bytes(data[0..4].try_into().unwrap());
            let parent_tgid = i32::from_ne_bytes(data[4..8].try_into().unwrap());
            let child_pid = i32::from_ne_bytes(data[8..12].try_into().unwrap());
            let child_tgid = i32::from_ne_bytes(data[12..16].try_into().unwrap());
            // Thread creates have child_pid != child_tgid; only track group leaders
            // (same set `/proc` and `is-running` see).
            if child_pid != child_tgid {
                return Vec::new();
            }
            let comm = tracker
                .pids
                .get(&parent_tgid)
                .cloned()
                .or_else(|| tracker.pids.get(&parent_pid).cloned())
                .or_else(|| read_comm(child_pid))
                .or_else(|| read_comm(parent_tgid))
                .or_else(|| read_comm(parent_pid));
            let Some(comm) = comm else {
                return Vec::new();
            };
            tracker.appear(child_pid, comm)
        }
        libc::PROC_EVENT_EXEC => {
            if data.len() < 8 {
                return Vec::new();
            }
            let process_pid = i32::from_ne_bytes(data[0..4].try_into().unwrap());
            let process_tgid = i32::from_ne_bytes(data[4..8].try_into().unwrap());
            if process_pid != process_tgid {
                return Vec::new();
            }
            let Some(comm) = read_comm(process_pid) else {
                return Vec::new();
            };
            tracker.rename(process_pid, comm)
        }
        libc::PROC_EVENT_COMM => {
            if data.len() < 8 + 16 {
                return Vec::new();
            }
            let process_pid = i32::from_ne_bytes(data[0..4].try_into().unwrap());
            let process_tgid = i32::from_ne_bytes(data[4..8].try_into().unwrap());
            if process_pid != process_tgid {
                return Vec::new();
            }
            let raw = &data[8..24];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            let Ok(comm) = std::str::from_utf8(&raw[..end]) else {
                return Vec::new();
            };
            let comm = comm.trim();
            if comm.is_empty() {
                return Vec::new();
            }
            tracker.rename(process_pid, comm.to_string())
        }
        libc::PROC_EVENT_EXIT => {
            if data.len() < 8 {
                return Vec::new();
            }
            let process_pid = i32::from_ne_bytes(data[0..4].try_into().unwrap());
            let process_tgid = i32::from_ne_bytes(data[4..8].try_into().unwrap());
            if process_pid != process_tgid {
                return Vec::new();
            }
            tracker.disappear(process_pid)
        }
        _ => Vec::new(),
    }
}

fn watch_running_changes(emit: &EventEmit) -> Result<(), String> {
    let sock = open_proc_connector()?;
    let mut tracker = ProcessTracker::new();
    tracker.seed_from_proc();

    let mut buf = vec![0u8; 4096];
    loop {
        let n = unsafe {
            libc::recv(
                sock.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                0,
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        let n = n as usize;
        if n < mem::size_of::<libc::nlmsghdr>() + mem::size_of::<CnMsg>() + 16 {
            continue;
        }

        let nlmsg_type = u16::from_ne_bytes(buf[4..6].try_into().unwrap());
        if nlmsg_type == libc::NLMSG_ERROR as u16 {
            return Err("netlink error".to_string());
        }

        let cn_offset = mem::size_of::<libc::nlmsghdr>();
        let cn_len = u16::from_ne_bytes(buf[cn_offset + 16..cn_offset + 18].try_into().unwrap());
        let event_offset = cn_offset + mem::size_of::<CnMsg>();
        if n < event_offset + 16 || cn_len as usize + mem::size_of::<CnMsg>() > n - cn_offset {
            continue;
        }

        let what = u32::from_ne_bytes(buf[event_offset..event_offset + 4].try_into().unwrap());
        let data_offset = event_offset + 16;
        let data_end = (event_offset + cn_len as usize).min(n);
        if data_end < data_offset {
            continue;
        }
        let flips = handle_proc_event(&mut tracker, what, &buf[data_offset..data_end]);
        emit_flips(emit, flips)?;
    }
}

fn spawn_running_watcher(emit: EventEmit) {
    thread::spawn(move || {
        loop {
            if let Err(_err) = watch_running_changes(&emit) {
                thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

fn main() {
    protocol::serve_with_events(|emit| {
        spawn_running_watcher(emit);
        handle_request
    });
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
    fn running_changed_event_uses_process_json_shape() {
        let event = RunningChanged {
            process: "firefox".to_string(),
            running: true,
        };
        let (name, payload) = running_changed_payload(&event);
        assert_eq!(name, "running-changed");
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["process"], "firefox");
        assert_eq!(value["running"], true);
    }

    #[test]
    fn tracker_appear_then_disappear_emits_flips() {
        let mut tracker = ProcessTracker::new();
        assert_eq!(
            tracker.appear(1, "firefox".to_string()),
            vec![("firefox".to_string(), true)]
        );
        assert!(tracker.appear(2, "firefox".to_string()).is_empty());
        assert!(tracker.disappear(1).is_empty());
        assert_eq!(tracker.disappear(2), vec![("firefox".to_string(), false)]);
    }

    #[test]
    fn tracker_rename_emits_old_and_new_flips() {
        let mut tracker = ProcessTracker::new();
        assert_eq!(
            tracker.appear(1, "old".to_string()),
            vec![("old".to_string(), true)]
        );
        assert_eq!(
            tracker.rename(1, "new".to_string()),
            vec![("old".to_string(), false), ("new".to_string(), true)]
        );
    }

    #[test]
    fn tracker_appear_existing_pid_keeps_both_rename_flips() {
        let mut tracker = ProcessTracker::new();
        assert_eq!(
            tracker.appear(1, "old".to_string()),
            vec![("old".to_string(), true)]
        );
        assert_eq!(
            tracker.appear(1, "new".to_string()),
            vec![("old".to_string(), false), ("new".to_string(), true)]
        );
    }

    #[test]
    fn handle_proc_event_ignores_thread_fork_and_exit() {
        let mut tracker = ProcessTracker::new();
        assert_eq!(
            tracker.appear(100, "firefox".to_string()),
            vec![("firefox".to_string(), true)]
        );
        // parent_pid=100, parent_tgid=100, child_pid=101, child_tgid=100 (thread)
        let mut fork = [0u8; 16];
        fork[0..4].copy_from_slice(&100i32.to_ne_bytes());
        fork[4..8].copy_from_slice(&100i32.to_ne_bytes());
        fork[8..12].copy_from_slice(&101i32.to_ne_bytes());
        fork[12..16].copy_from_slice(&100i32.to_ne_bytes());
        assert!(handle_proc_event(&mut tracker, libc::PROC_EVENT_FORK, &fork).is_empty());

        // process_pid=101, process_tgid=100 (thread exit)
        let mut exit = [0u8; 8];
        exit[0..4].copy_from_slice(&101i32.to_ne_bytes());
        exit[4..8].copy_from_slice(&100i32.to_ne_bytes());
        assert!(handle_proc_event(&mut tracker, libc::PROC_EVENT_EXIT, &exit).is_empty());
        assert_eq!(tracker.disappear(100), vec![("firefox".to_string(), false)]);
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
        handle_request(&protocol::check_request(
            "process", method, "firefox", allowed,
        ))
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
