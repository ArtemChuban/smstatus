use super::App;

impl App {
    pub(in crate::tui) fn refresh_daemon_status(
        &mut self,
        status: crate::error::Result<crate::daemon::DaemonStatus>,
    ) {
        self.daemon_status = status.ok();
        if self.pending_start.is_some()
            && matches!(
                self.daemon_status,
                Some(crate::daemon::DaemonStatus::Running { .. })
                    | Some(crate::daemon::DaemonStatus::RunningPidUnknown)
            )
        {
            self.pending_start_confirmed_running = true;
        }
    }

    pub(in crate::tui) fn poll_pending_start(&mut self) {
        let Some(child) = self.pending_start.as_mut() else {
            return;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.code() == Some(crate::cli::EXIT_ALREADY_RUNNING as i32) {
                    self.push_action_message("smstatus is already running".to_string());
                } else if self.pending_start_confirmed_running {
                    match crate::lock::log_file_path() {
                        Ok(log_path) => self.push_action_message(format!(
                            "smstatus exited unexpectedly, see {}",
                            log_path.display()
                        )),
                        Err(_) => {
                            self.push_action_message("smstatus exited unexpectedly".to_string())
                        }
                    }
                } else {
                    match crate::lock::log_file_path() {
                        Ok(log_path) => self.push_action_message(format!(
                            "smstatus failed to start, see {}",
                            log_path.display()
                        )),
                        Err(_) => self.push_action_message("smstatus failed to start".to_string()),
                    }
                }
                self.pending_start = None;
                self.pending_start_confirmed_running = false;
            }
            Ok(None) => {}
            Err(err) => {
                self.push_action_message(format!("failed to check daemon start: {err}"));
                self.pending_start = None;
                self.pending_start_confirmed_running = false;
            }
        }
    }

    pub(super) fn start_daemon(&mut self) {
        match self.daemon_status {
            Some(crate::daemon::DaemonStatus::Running { .. })
            | Some(crate::daemon::DaemonStatus::RunningPidUnknown) => {
                self.push_action_message("smstatus is already running".to_string());
            }
            _ if self.pending_start.is_some() => {
                self.push_action_message("smstatus is already starting".to_string());
            }
            _ => match crate::daemon::spawn_daemon() {
                Ok(child) => {
                    self.pending_start = Some(child);
                    self.pending_start_confirmed_running = false;
                    self.push_action_message("Starting smstatus...".to_string());
                }
                Err(err) => self.push_action_message(format!("Failed to start smstatus: {err}")),
            },
        }
    }

    pub(super) fn stop_daemon(&mut self) {
        match self.daemon_status {
            Some(crate::daemon::DaemonStatus::Stopped) | None => {
                self.push_action_message("smstatus is not running".to_string());
            }
            _ => match crate::daemon::signal_stop() {
                Ok(crate::daemon::StopOutcome::Signaled { pid }) => {
                    self.push_action_message(format!("Sent stop signal to smstatus (pid {pid})"))
                }
                Ok(crate::daemon::StopOutcome::NotRunning) => {
                    self.push_action_message("smstatus is not running".to_string())
                }
                Ok(crate::daemon::StopOutcome::PidUnknown) => self.push_action_message(
                    "smstatus is running, but its pid file is unreadable".to_string(),
                ),
                Err(err) => self.push_action_message(format!("Failed to stop smstatus: {err}")),
            },
        }
    }
}
