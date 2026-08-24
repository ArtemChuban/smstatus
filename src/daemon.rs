use std::fs::{File, OpenOptions};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal::{Signal, kill};
use nix::unistd::{Pid, setsid};

use crate::bar;
use crate::cli::{DAEMON_ENV_VAR, EXIT_ALREADY_RUNNING};
use crate::lock::{self, LockOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DaemonStatus {
    Stopped,
    Running { pid: i32 },
    RunningPidUnknown,
}

fn run_locked(report_already_running: impl FnOnce(Option<i32>)) -> ExitCode {
    match lock::acquire_lock() {
        Ok(LockOutcome::AlreadyRunning(pid)) => {
            report_already_running(pid);
            ExitCode::from(EXIT_ALREADY_RUNNING)
        }
        Ok(LockOutcome::Acquired(_flock)) => match bar::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                crate::logging::to_stderr(
                    log::Level::Error,
                    &format!("smstatus exited with an error: {err}"),
                );
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            crate::logging::to_stderr(log::Level::Error, &format!("failed to acquire lock: {err}"));
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_daemon() -> ExitCode {
    run_locked(|_pid| {})
}

pub(crate) fn cmd_run() -> ExitCode {
    run_locked(|pid| match pid {
        Some(pid) => crate::logging::to_stderr(
            log::Level::Info,
            &format!("smstatus is already running (pid {pid})"),
        ),
        None => crate::logging::to_stderr(log::Level::Info, "smstatus is already running"),
    })
}

pub(crate) fn spawn_daemon() -> crate::error::Result<std::process::Child> {
    let log_path =
        lock::log_file_path().map_err(|err| format!("failed to determine log file path: {err}"))?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let _log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| format!("failed to open log file {}: {err}", log_path.display()))?;

    let current_exe = std::env::current_exe()
        .map_err(|err| format!("failed to determine current executable: {err}"))?;

    let mut command = Command::new(current_exe);
    command
        .env(DAEMON_ENV_VAR, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // SAFETY: `pre_exec` runs in the forked child, after fork but before
    // exec - the only point `setsid()` can take effect. It detaches the
    // daemon into its own session so it survives the parent shell exiting
    // and won't receive signals meant for the invoking terminal's process
    // group. The closure only calls the async-signal-safe `setsid()` and
    // returns an `io::Error`, upholding `pre_exec`'s safety contract.
    unsafe {
        command.pre_exec(|| setsid().map(|_| ()).map_err(std::io::Error::from));
    }

    command
        .spawn()
        .map_err(|err| format!("failed to spawn smstatus daemon: {err}").into())
}

pub(crate) fn cmd_start() -> ExitCode {
    let mut child = match spawn_daemon() {
        Ok(child) => child,
        Err(err) => {
            crate::logging::to_stderr(log::Level::Error, &err.to_string());
            return ExitCode::FAILURE;
        }
    };

    let lock_path = match lock::lock_file_path() {
        Ok(path) => path,
        Err(err) => {
            crate::logging::to_stderr(
                log::Level::Error,
                &format!("failed to determine lock file path: {err}"),
            );
            return ExitCode::FAILURE;
        }
    };
    let log_path = match lock::log_file_path() {
        Ok(path) => path,
        Err(err) => {
            crate::logging::to_stderr(
                log::Level::Error,
                &format!("failed to determine log file path: {err}"),
            );
            return ExitCode::FAILURE;
        }
    };

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.code() == Some(EXIT_ALREADY_RUNNING as i32) {
                    let pid = File::open(&lock_path)
                        .ok()
                        .and_then(|mut f| lock::read_pid(&mut f));
                    match pid {
                        Some(pid) => crate::logging::to_stderr(
                            log::Level::Info,
                            &format!("smstatus is already running (pid {pid})"),
                        ),
                        None => crate::logging::to_stderr(
                            log::Level::Info,
                            "smstatus is already running",
                        ),
                    }
                    return ExitCode::from(EXIT_ALREADY_RUNNING);
                }
                crate::logging::to_stderr(
                    log::Level::Error,
                    &format!("smstatus failed to start, see {}:", log_path.display()),
                );
                if let Ok(contents) = std::fs::read_to_string(&log_path) {
                    eprint!("{contents}");
                }
                return ExitCode::FAILURE;
            }
            Ok(None) => {
                if let Some(pid) = File::open(&lock_path)
                    .ok()
                    .and_then(|mut f| lock::read_pid(&mut f))
                    && pid == child.id() as i32
                {
                    crate::logging::to_stdout(
                        log::Level::Info,
                        &format!("smstatus started (pid {pid})"),
                    );
                    return ExitCode::SUCCESS;
                }
            }
            Err(err) => {
                crate::logging::to_stderr(
                    log::Level::Error,
                    &format!("failed to check daemon status: {err}"),
                );
                return ExitCode::FAILURE;
            }
        }

        if Instant::now() >= deadline {
            crate::logging::to_stderr(
                log::Level::Error,
                &format!(
                    "smstatus did not confirm startup in time; check {}",
                    log_path.display()
                ),
            );
            return ExitCode::FAILURE;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(crate) fn status() -> crate::error::Result<DaemonStatus> {
    let lock_path = lock::lock_file_path()?;
    if !lock_path.exists() {
        return Ok(DaemonStatus::Stopped);
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|err| format!("failed to open {}: {err}", lock_path.display()))?;
    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(flock) => {
            drop(flock);
            Ok(DaemonStatus::Stopped)
        }
        Err((mut file, Errno::EWOULDBLOCK)) => Ok(match lock::read_pid(&mut file) {
            Some(pid) => DaemonStatus::Running { pid },
            None => DaemonStatus::RunningPidUnknown,
        }),
        Err((_, err)) => {
            Err(format!("failed to check lock on {}: {err}", lock_path.display()).into())
        }
    }
}

pub(crate) enum StopOutcome {
    Signaled { pid: i32 },
    NotRunning,
    PidUnknown,
}

pub(crate) fn signal_stop() -> crate::error::Result<StopOutcome> {
    let pid = match status()? {
        DaemonStatus::Stopped => return Ok(StopOutcome::NotRunning),
        DaemonStatus::RunningPidUnknown => return Ok(StopOutcome::PidUnknown),
        DaemonStatus::Running { pid } => pid,
    };

    match kill(Pid::from_raw(pid), Signal::SIGTERM) {
        Ok(()) => Ok(StopOutcome::Signaled { pid }),
        Err(Errno::ESRCH) => Ok(StopOutcome::NotRunning),
        Err(err) => Err(format!("failed to signal smstatus (pid {pid}): {err}").into()),
    }
}

pub(crate) fn cmd_stop() -> ExitCode {
    let pid = match signal_stop() {
        Ok(StopOutcome::Signaled { pid }) => pid,
        Ok(StopOutcome::NotRunning) => {
            crate::logging::to_stdout(log::Level::Info, "smstatus is not running");
            return ExitCode::SUCCESS;
        }
        Ok(StopOutcome::PidUnknown) => {
            crate::logging::to_stderr(
                log::Level::Error,
                "smstatus is running, but its pid file is unreadable",
            );
            return ExitCode::FAILURE;
        }
        Err(err) => {
            crate::logging::to_stderr(log::Level::Error, &format!("failed to stop daemon: {err}"));
            return ExitCode::FAILURE;
        }
    };
    let target = Pid::from_raw(pid);

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Err(Errno::ESRCH) = kill(target, None) {
            crate::logging::to_stdout(log::Level::Info, &format!("smstatus stopped (pid {pid})"));
            return ExitCode::SUCCESS;
        }
        if Instant::now() >= deadline {
            crate::logging::to_stderr(
                log::Level::Error,
                &format!("sent SIGTERM to smstatus (pid {pid}) but it has not exited yet"),
            );
            return ExitCode::FAILURE;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
