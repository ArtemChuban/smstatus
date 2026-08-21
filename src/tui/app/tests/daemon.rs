use super::*;

#[test]
fn refresh_daemon_status_stores_ok_value() {
    let mut app = App::default();
    app.refresh_daemon_status(Ok(crate::daemon::DaemonStatus::Running { pid: 42 }));
    assert_eq!(
        app.daemon_status,
        Some(crate::daemon::DaemonStatus::Running { pid: 42 })
    );
}

#[test]
fn refresh_daemon_status_maps_err_to_none() {
    let mut app = App::default();
    app.refresh_daemon_status(Err("boom".into()));
    assert_eq!(app.daemon_status, None);
}

#[test]
fn stop_daemon_when_stopped_is_a_noop_with_message() {
    let mut app = App {
        daemon_status: Some(crate::daemon::DaemonStatus::Stopped),
        ..App::default()
    };
    app.stop_daemon();
    assert_eq!(app.action_log, vec!["smstatus is not running"]);
    assert!(app.pending_start.is_none());
}

#[test]
fn stop_daemon_when_status_unknown_is_a_noop_with_message() {
    let mut app = App {
        daemon_status: None,
        ..App::default()
    };
    app.stop_daemon();
    assert_eq!(app.action_log, vec!["smstatus is not running"]);
    assert!(app.pending_start.is_none());
}

#[test]
fn start_daemon_when_running_is_a_noop_with_message() {
    let mut app = App {
        daemon_status: Some(crate::daemon::DaemonStatus::Running { pid: 42 }),
        ..App::default()
    };
    app.start_daemon();
    assert_eq!(app.action_log, vec!["smstatus is already running"]);
    assert!(app.pending_start.is_none());
}

#[test]
fn start_daemon_when_running_pid_unknown_is_a_noop_with_message() {
    let mut app = App {
        daemon_status: Some(crate::daemon::DaemonStatus::RunningPidUnknown),
        ..App::default()
    };
    app.start_daemon();
    assert_eq!(app.action_log, vec!["smstatus is already running"]);
    assert!(app.pending_start.is_none());
}

#[test]
fn poll_pending_start_is_noop_when_nothing_pending() {
    let mut app = App::default();
    app.poll_pending_start();
    assert!(app.action_log.is_empty());
    assert!(app.pending_start.is_none());
}

fn spawn_test_child(shell_code: &str) -> std::process::Child {
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(shell_code)
        .spawn()
        .expect("failed to spawn test child process")
}

fn wait_for_pending_start_to_be_reaped(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.poll_pending_start();
        if app.pending_start.is_none() {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("pending_start was not reaped in time");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn start_daemon_when_pending_start_already_some_is_a_noop_and_keeps_existing_child() {
    let child = spawn_test_child(":");
    let pid = child.id();
    let mut app = App {
        pending_start: Some(child),
        ..App::default()
    };
    app.start_daemon();
    assert_eq!(app.action_log, vec!["smstatus is already starting"]);
    assert_eq!(app.pending_start.as_ref().map(|c| c.id()), Some(pid));
    app.pending_start.as_mut().unwrap().wait().unwrap();
}

#[test]
fn pending_start_confirmed_running_stays_false_for_stopped_or_unknown_status() {
    let mut app = App {
        pending_start: Some(spawn_test_child("sleep 0.2")),
        ..App::default()
    };
    app.refresh_daemon_status(Ok(crate::daemon::DaemonStatus::Stopped));
    assert!(!app.pending_start_confirmed_running);
    app.refresh_daemon_status(Err("boom".into()));
    assert!(!app.pending_start_confirmed_running);
    app.pending_start.as_mut().unwrap().wait().unwrap();
}

#[test]
fn pending_start_confirmed_running_is_set_once_status_shows_running() {
    let mut app = App {
        pending_start: Some(spawn_test_child("sleep 0.2")),
        ..App::default()
    };
    app.refresh_daemon_status(Ok(crate::daemon::DaemonStatus::Running { pid: 4242 }));
    assert!(app.pending_start_confirmed_running);
    app.pending_start.as_mut().unwrap().wait().unwrap();
}

#[test]
fn poll_pending_start_reports_failed_to_start_when_never_confirmed_running() {
    let mut app = App {
        pending_start: Some(spawn_test_child("exit 7")),
        ..App::default()
    };
    wait_for_pending_start_to_be_reaped(&mut app);
    assert_eq!(app.action_log.len(), 1);
    assert!(
        app.action_log[0].starts_with("smstatus failed to start"),
        "unexpected message: {}",
        app.action_log[0]
    );
    assert!(!app.pending_start_confirmed_running);
}

#[test]
fn poll_pending_start_reports_exited_unexpectedly_when_confirmed_running_first() {
    let mut app = App {
        pending_start: Some(spawn_test_child("exit 7")),
        ..App::default()
    };
    app.refresh_daemon_status(Ok(crate::daemon::DaemonStatus::Running { pid: 4242 }));
    assert!(app.pending_start_confirmed_running);
    wait_for_pending_start_to_be_reaped(&mut app);
    assert_eq!(app.action_log.len(), 1);
    assert!(
        app.action_log[0].starts_with("smstatus exited unexpectedly"),
        "unexpected message: {}",
        app.action_log[0]
    );
    assert!(!app.pending_start_confirmed_running);
}
