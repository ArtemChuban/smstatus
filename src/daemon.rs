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

fn run_locked(report_already_running: impl FnOnce(Option<i32>)) -> ExitCode {
    match lock::acquire_lock() {
        Ok(LockOutcome::AlreadyRunning(pid)) => {
            report_already_running(pid);
            ExitCode::from(EXIT_ALREADY_RUNNING)
        }
        Ok(LockOutcome::Acquired(_flock)) => match bar::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("smstatus exited with an error: {err}");
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            eprintln!("failed to acquire lock: {err}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_daemon() -> ExitCode {
    run_locked(|_pid| {})
}

pub(crate) fn cmd_run() -> ExitCode {
    run_locked(|pid| match pid {
        Some(pid) => eprintln!("smstatus is already running (pid {pid})"),
        None => eprintln!("smstatus is already running"),
    })
}

pub(crate) fn cmd_start() -> ExitCode {
    let log_path = match lock::log_file_path() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine log file path: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(parent) = log_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!("failed to create {}: {err}", parent.display());
        return ExitCode::FAILURE;
    }
    let log_file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
    {
        Ok(file) => file,
        Err(err) => {
            eprintln!("failed to open log file {}: {err}", log_path.display());
            return ExitCode::FAILURE;
        }
    };
    let stdout_log = match log_file.try_clone() {
        Ok(file) => file,
        Err(err) => {
            eprintln!("failed to duplicate log file handle: {err}");
            return ExitCode::FAILURE;
        }
    };

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine current executable: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mut command = Command::new(current_exe);
    command
        .env(DAEMON_ENV_VAR, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_log))
        .stderr(Stdio::from(log_file));

    // SAFETY: `pre_exec` runs in the forked child, after fork but before
    // exec — the only point `setsid()` can take effect. It detaches the
    // daemon into its own session so it survives the parent shell exiting
    // and won't receive signals meant for the invoking terminal's process
    // group. The closure only calls the async-signal-safe `setsid()` and
    // returns an `io::Error`, upholding `pre_exec`'s safety contract.
    unsafe {
        command.pre_exec(|| setsid().map(|_| ()).map_err(std::io::Error::from));
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            eprintln!("failed to spawn smstatus daemon: {err}");
            return ExitCode::FAILURE;
        }
    };

    let lock_path = match lock::lock_file_path() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine lock file path: {err}");
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
                        Some(pid) => eprintln!("smstatus is already running (pid {pid})"),
                        None => eprintln!("smstatus is already running"),
                    }
                    return ExitCode::from(EXIT_ALREADY_RUNNING);
                }
                eprintln!("smstatus failed to start, see {}:", log_path.display());
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
                    println!("smstatus started (pid {pid})");
                    return ExitCode::SUCCESS;
                }
            }
            Err(err) => {
                eprintln!("failed to check daemon status: {err}");
                return ExitCode::FAILURE;
            }
        }

        if Instant::now() >= deadline {
            eprintln!(
                "smstatus did not confirm startup in time; check {}",
                log_path.display()
            );
            return ExitCode::FAILURE;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(crate) fn cmd_stop() -> ExitCode {
    let lock_path = match lock::lock_file_path() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("failed to determine lock file path: {err}");
            return ExitCode::FAILURE;
        }
    };

    if !lock_path.exists() {
        println!("smstatus is not running");
        return ExitCode::SUCCESS;
    }

    let file = match OpenOptions::new().read(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("failed to open {}: {err}", lock_path.display());
            return ExitCode::FAILURE;
        }
    };

    let mut file = match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(flock) => {
            drop(flock);
            println!("smstatus is not running");
            return ExitCode::SUCCESS;
        }
        Err((file, Errno::EWOULDBLOCK)) => file,
        Err((_, err)) => {
            eprintln!("failed to check lock on {}: {err}", lock_path.display());
            return ExitCode::FAILURE;
        }
    };

    let pid = match lock::read_pid(&mut file) {
        Some(pid) => pid,
        None => {
            eprintln!("smstatus is running, but its pid file is unreadable");
            return ExitCode::FAILURE;
        }
    };
    let target = Pid::from_raw(pid);

    match kill(target, Signal::SIGTERM) {
        Ok(()) => {}
        Err(Errno::ESRCH) => {
            println!("smstatus is not running");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("failed to signal smstatus (pid {pid}): {err}");
            return ExitCode::FAILURE;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Err(Errno::ESRCH) = kill(target, None) {
            println!("smstatus stopped (pid {pid})");
            return ExitCode::SUCCESS;
        }
        if Instant::now() >= deadline {
            eprintln!("sent SIGTERM to smstatus (pid {pid}) but it has not exited yet");
            return ExitCode::FAILURE;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
