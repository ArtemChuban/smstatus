use super::*;

#[test]
fn daemon_status_phrase_running() {
    assert_eq!(
        daemon_status_phrase(Some(DaemonStatus::Running { pid: 42 })),
        "running (pid 42)"
    );
}

#[test]
fn daemon_status_phrase_running_pid_unknown() {
    assert_eq!(
        daemon_status_phrase(Some(DaemonStatus::RunningPidUnknown)),
        "running (pid unknown)"
    );
}

#[test]
fn daemon_status_phrase_stopped() {
    assert_eq!(daemon_status_phrase(Some(DaemonStatus::Stopped)), "stopped");
}

#[test]
fn daemon_status_phrase_unknown() {
    assert_eq!(daemon_status_phrase(None), "status unknown");
}
