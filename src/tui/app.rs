use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) const ACTION_LOG_CAPACITY: usize = 3;

#[derive(Default, PartialEq, Eq, Debug)]
pub(super) enum Mode {
    #[default]
    Normal,
    EditingSeparator {
        buffer: String,
        cursor: usize,
    },
}

#[derive(Default)]
pub(super) struct App {
    pub(super) should_quit: bool,
    pub(super) daemon_status: Option<crate::daemon::DaemonStatus>,
    pub(super) action_log: Vec<String>,
    pub(super) pending_start: Option<std::process::Child>,
    pub(super) pending_start_confirmed_running: bool,
    pub(super) mode: Mode,
    pub(super) config_path: Option<PathBuf>,
    pub(super) separator: Option<String>,
    pub(super) config_watcher: Option<crate::watcher::ReloadWatcher>,
    pub(super) last_separator_error: Option<String>,
    pub(super) modules: Option<Vec<String>>,
    pub(super) last_modules_error: Option<String>,
    pub(super) module_scroll_offset: usize,
    pub(super) modules_viewport_height: usize,
    pub(super) selected_index: Option<usize>,
}

impl App {
    pub(super) fn new() -> Self {
        let mut app = Self::default();
        match crate::config::default_config_dir() {
            Ok(config_dir) => {
                let config_path = config_dir.join("config.toml");
                let modules_dir = config_dir.join("modules");
                match crate::watcher::ReloadWatcher::new(
                    &config_dir,
                    config_path.clone(),
                    modules_dir,
                ) {
                    Ok(watcher) => app.config_watcher = Some(watcher),
                    Err(err) => {
                        app.push_action_message(format!("config hot-reload unavailable: {err}"))
                    }
                }
                app.config_path = Some(config_path);
                app.refresh_config();
            }
            Err(err) => app.push_action_message(format!("could not determine config path: {err}")),
        }
        app
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        match self.mode {
            Mode::EditingSeparator { .. } => self.handle_key_editing_separator(key),
            Mode::Normal => self.handle_key_normal(key),
        }
    }

    fn handle_key_normal(&mut self, key: KeyEvent) {
        if is_quit(key) {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('s') => self.start_daemon(),
            KeyCode::Char('k') => self.stop_daemon(),
            KeyCode::Char('e') => self.begin_edit_separator(),
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => self.move_module_up(),
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_module_down()
            }
            KeyCode::Up => self.select_previous_module(),
            KeyCode::Down => self.select_next_module(),
            _ => {}
        }
    }

    fn handle_key_editing_separator(&mut self, key: KeyEvent) {
        if is_hard_quit(key) {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Esc => self.cancel_edit_separator(),
            KeyCode::Enter => self.commit_edit_separator(),
            KeyCode::Left => {
                if let Mode::EditingSeparator { cursor, .. } = &mut self.mode {
                    *cursor = cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if let Mode::EditingSeparator { buffer, cursor } = &mut self.mode {
                    *cursor = (*cursor + 1).min(buffer.chars().count());
                }
            }
            KeyCode::Backspace => {
                if let Mode::EditingSeparator { buffer, cursor } = &mut self.mode
                    && *cursor > 0
                {
                    let byte_idx = super::char_byte_offset(buffer, *cursor - 1);
                    buffer.remove(byte_idx);
                    *cursor -= 1;
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Mode::EditingSeparator { buffer, cursor } = &mut self.mode {
                    let byte_idx = super::char_byte_offset(buffer, *cursor);
                    buffer.insert(byte_idx, c);
                    *cursor += 1;
                }
            }
            _ => {}
        }
    }

    fn begin_edit_separator(&mut self) {
        if self.config_path.is_none() {
            self.push_action_message("cannot edit separator: config path unknown".to_string());
            return;
        }
        let initial = self.separator.clone().unwrap_or_default();
        let cursor = initial.chars().count();
        self.mode = Mode::EditingSeparator {
            buffer: initial,
            cursor,
        };
    }

    fn cancel_edit_separator(&mut self) {
        self.mode = Mode::Normal;
    }

    fn commit_edit_separator(&mut self) {
        let Mode::EditingSeparator { buffer: value, .. } = std::mem::take(&mut self.mode) else {
            return;
        };
        let Some(path) = self.config_path.clone() else {
            self.push_action_message("cannot save separator: config path unknown".to_string());
            return;
        };
        match crate::config::BarConfig::write_separator(&path, &value) {
            Ok(()) => {
                self.separator = Some(value);
                self.push_action_message("Separator updated".to_string());
            }
            Err(err) => self.push_action_message(format!("Failed to update separator: {err}")),
        }
    }

    pub(super) fn poll_config_changes(&mut self) {
        let reloaded = self
            .config_watcher
            .as_mut()
            .map(|watcher| watcher.try_reload())
            .unwrap_or(false);
        if reloaded {
            self.refresh_config();
        }
    }

    fn refresh_config(&mut self) {
        let Some(path) = self.config_path.as_deref() else {
            return;
        };
        match crate::config::BarConfig::load(path) {
            Ok(config) => {
                self.separator = Some(config.separator());
                self.last_separator_error = None;
                match config.module_names() {
                    Ok(names) => {
                        self.module_scroll_offset = self
                            .module_scroll_offset
                            .min(names.len().saturating_sub(self.modules_viewport_height));
                        self.selected_index = if names.is_empty() {
                            None
                        } else {
                            Some(self.selected_index.unwrap_or(0).min(names.len() - 1))
                        };
                        self.modules = Some(names);
                        self.last_modules_error = None;
                        self.ensure_selected_visible();
                    }
                    Err(err) => {
                        self.modules = None;
                        self.selected_index = None;
                        let message = err.to_string();
                        if self.last_modules_error.as_deref() != Some(message.as_str()) {
                            self.push_action_message(format!("Failed to read modules: {message}"));
                            self.last_modules_error = Some(message);
                        }
                    }
                }
            }
            Err(err) => {
                self.separator = None;
                self.modules = None;
                self.selected_index = None;
                let message = err.to_string();
                if self.last_separator_error.as_deref() != Some(message.as_str()) {
                    self.push_action_message(format!("Failed to read config: {message}"));
                    self.last_separator_error = Some(message);
                }
            }
        }
    }

    fn ensure_selected_visible(&mut self) {
        let Some(idx) = self.selected_index else {
            return;
        };
        let viewport = self.modules_viewport_height.max(1);
        if idx < self.module_scroll_offset {
            self.module_scroll_offset = idx;
        } else if idx >= self.module_scroll_offset + viewport {
            self.module_scroll_offset = idx + 1 - viewport;
        }
    }

    fn select_previous_module(&mut self) {
        let Some(modules) = self.modules.as_ref() else {
            return;
        };
        let Some(idx) = self.selected_index else {
            return;
        };
        if !modules.is_empty() && idx > 0 {
            self.selected_index = Some(idx - 1);
            self.ensure_selected_visible();
        }
    }

    fn select_next_module(&mut self) {
        let Some(modules) = self.modules.as_ref() else {
            return;
        };
        let Some(idx) = self.selected_index else {
            return;
        };
        if idx + 1 < modules.len() {
            self.selected_index = Some(idx + 1);
            self.ensure_selected_visible();
        }
    }

    fn move_module_up(&mut self) {
        self.move_module(-1);
    }

    fn move_module_down(&mut self) {
        self.move_module(1);
    }

    fn move_module(&mut self, delta: isize) {
        let Some(modules) = self.modules.as_ref() else {
            return;
        };
        let Some(idx) = self.selected_index else {
            return;
        };
        let Some(target) = idx.checked_add_signed(delta).filter(|&t| t < modules.len()) else {
            return;
        };
        let Some(path) = self.config_path.clone() else {
            self.push_action_message("cannot reorder modules: config path unknown".to_string());
            return;
        };
        let mut new_order = modules.clone();
        new_order.swap(idx, target);
        let name = modules[idx].clone();
        match crate::config::BarConfig::write_module_order(&path, &new_order) {
            Ok(()) => {
                self.modules = Some(new_order);
                self.selected_index = Some(target);
                self.ensure_selected_visible();
                let direction = if delta < 0 { "up" } else { "down" };
                self.push_action_message(format!("Moved {name} {direction}"));
            }
            Err(err) => self.push_action_message(format!("Failed to reorder modules: {err}")),
        }
    }

    pub(super) fn refresh_daemon_status(
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

    pub(super) fn poll_pending_start(&mut self) {
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

    fn push_action_message(&mut self, message: String) {
        self.action_log.push(message);
        if self.action_log.len() > ACTION_LOG_CAPACITY {
            self.action_log.remove(0);
        }
    }
}

fn is_hard_quit(key: KeyEvent) -> bool {
    (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn is_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q')) || is_hard_quit(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    static TEST_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_path(purpose: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = TEST_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "smstatus-tui-app-test-{purpose}-{}-{nanos}-{counter}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn q_key_requests_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_c_requests_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_d_requests_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn unrelated_key_does_not_request_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.should_quit);
    }

    #[test]
    fn plain_c_without_control_does_not_request_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!app.should_quit);
    }

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
    fn push_action_message_caps_at_capacity_keeping_most_recent() {
        let mut app = App::default();
        app.push_action_message("one".to_string());
        app.push_action_message("two".to_string());
        app.push_action_message("three".to_string());
        app.push_action_message("four".to_string());
        assert_eq!(app.action_log, vec!["two", "three", "four"]);
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

    #[test]
    fn begin_edit_separator_prefills_buffer_with_current_separator() {
        let mut app = App {
            config_path: Some(unique_temp_path("prefill")),
            separator: Some(" | ".to_string()),
            ..App::default()
        };
        app.begin_edit_separator();
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: " | ".to_string(),
                cursor: 3,
            }
        );
    }

    #[test]
    fn begin_edit_separator_without_config_path_pushes_message_and_stays_normal() {
        let mut app = App {
            config_path: None,
            ..App::default()
        };
        app.begin_edit_separator();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(
            app.action_log,
            vec!["cannot edit separator: config path unknown"]
        );
    }

    #[test]
    fn typing_plain_char_while_editing_appends_to_buffer() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "a".to_string(),
                cursor: 1,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "ab".to_string(),
                cursor: 2,
            }
        );
    }

    #[test]
    fn ctrl_modified_char_while_editing_is_ignored() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "a".to_string(),
                cursor: 1,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "a".to_string(),
                cursor: 1,
            }
        );
    }

    #[test]
    fn backspace_removes_last_char_while_editing() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "ab".to_string(),
                cursor: 2,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "a".to_string(),
                cursor: 1,
            }
        );
    }

    #[test]
    fn backspace_on_empty_buffer_while_editing_is_noop() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: String::new(),
                cursor: 0,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: String::new(),
                cursor: 0,
            }
        );
    }

    #[test]
    fn backspace_at_mid_buffer_cursor_removes_char_before_cursor_only() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 1,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "bc".to_string(),
                cursor: 0,
            }
        );
    }

    #[test]
    fn typing_char_at_mid_buffer_cursor_inserts_at_cursor_not_at_end() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "ac".to_string(),
                cursor: 1,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 2,
            }
        );
    }

    #[test]
    fn left_key_moves_cursor_left_and_saturates_at_zero() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 1,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 0,
            }
        );
        app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 0,
            }
        );
    }

    #[test]
    fn right_key_moves_cursor_right_and_saturates_at_buffer_length() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 2,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 3,
            }
        );
        app.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 3,
            }
        );
    }

    #[test]
    fn multi_byte_utf8_buffer_uses_char_indices_not_byte_indices() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "é".to_string(),
                cursor: 1,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "éx".to_string(),
                cursor: 2,
            }
        );
        app.handle_key(key(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "éx".to_string(),
                cursor: 1,
            }
        );
        app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "x".to_string(),
                cursor: 0,
            }
        );
    }

    #[test]
    fn q_while_editing_appends_literal_char_rather_than_quitting() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: String::new(),
                cursor: 0,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.should_quit);
        assert_eq!(
            app.mode,
            Mode::EditingSeparator {
                buffer: "q".to_string(),
                cursor: 1,
            }
        );
    }

    #[test]
    fn ctrl_c_while_editing_still_quits() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 3,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn esc_while_editing_returns_to_normal_without_logging() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: "abc".to_string(),
                cursor: 3,
            },
            ..App::default()
        };
        app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.action_log.is_empty());
    }

    #[test]
    fn enter_while_editing_writes_value_updates_separator_and_logs_success() {
        let path = unique_temp_path("commit");
        std::fs::write(&path, "separator = \" | \"\n").unwrap();
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: " :: ".to_string(),
                cursor: 4,
            },
            config_path: Some(path.clone()),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
        let _ = std::fs::remove_file(&path);

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.separator, Some(" :: ".to_string()));
        assert_eq!(app.action_log, vec!["Separator updated"]);
    }

    #[test]
    fn enter_with_empty_buffer_writes_empty_separator_successfully() {
        let path = unique_temp_path("commit-empty");
        std::fs::write(&path, "separator = \" | \"\n").unwrap();
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: String::new(),
                cursor: 0,
            },
            config_path: Some(path.clone()),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
        let content = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.separator, Some(String::new()));
        assert_eq!(app.action_log, vec!["Separator updated"]);
        assert!(content.contains("separator = \"\""));
    }

    #[test]
    fn enter_without_config_path_logs_failure_and_returns_to_normal() {
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: " | ".to_string(),
                cursor: 3,
            },
            config_path: None,
            ..App::default()
        };
        app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(
            app.action_log,
            vec!["cannot save separator: config path unknown"]
        );
    }

    #[test]
    fn refresh_config_logs_once_for_a_persisting_error() {
        let path = unique_temp_path("invalid-toml");
        std::fs::write(&path, "this is not valid toml [[[").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };

        app.refresh_config();
        assert_eq!(app.separator, None);
        assert_eq!(app.action_log.len(), 1);
        assert!(
            app.action_log[0].starts_with("Failed to read config:"),
            "unexpected message: {}",
            app.action_log[0]
        );
        assert!(app.last_separator_error.is_some());

        app.refresh_config();
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.action_log.len(), 1);
    }

    #[test]
    fn refresh_config_recovers_and_clears_dedup_state_once_file_is_fixed() {
        let path = unique_temp_path("recovers");
        std::fs::write(&path, "this is not valid toml [[[").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };

        app.refresh_config();
        assert_eq!(app.action_log.len(), 1);
        assert!(app.last_separator_error.is_some());

        std::fs::write(&path, "separator = \" :: \"\nmodules = [\"cpu\"]\n").unwrap();
        app.refresh_config();
        let _ = std::fs::remove_file(&path);

        assert_eq!(app.separator, Some(" :: ".to_string()));
        assert!(app.last_separator_error.is_none());
        assert_eq!(app.modules, Some(vec!["cpu".to_string()]));
        assert_eq!(app.action_log.len(), 1);
    }

    #[test]
    fn refresh_config_missing_modules_key_does_not_clobber_working_separator() {
        let path = unique_temp_path("missing-modules");
        std::fs::write(&path, "separator = \" | \"\n").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };

        app.refresh_config();
        let _ = std::fs::remove_file(&path);

        assert_eq!(app.separator, Some(" | ".to_string()));
        assert_eq!(app.modules, None);
        assert_eq!(app.action_log.len(), 1);
        assert!(
            app.action_log[0].starts_with("Failed to read modules:"),
            "unexpected message: {}",
            app.action_log[0]
        );
        assert!(app.last_modules_error.is_some());
        assert!(app.last_separator_error.is_none());
    }

    #[test]
    fn refresh_config_logs_modules_error_once_for_a_persisting_error() {
        let path = unique_temp_path("modules-persisting-error");
        std::fs::write(&path, "separator = \" | \"\n").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };

        app.refresh_config();
        assert_eq!(app.action_log.len(), 1);
        assert!(app.last_modules_error.is_some());

        app.refresh_config();
        assert_eq!(app.action_log.len(), 1);

        std::fs::write(
            &path,
            "separator = \" | \"\nmodules = [\"cpu\", \"disk#root\"]\n",
        )
        .unwrap();
        app.refresh_config();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            app.modules,
            Some(vec!["cpu".to_string(), "disk#root".to_string()])
        );
        assert!(app.last_modules_error.is_none());
        assert_eq!(app.action_log.len(), 1);
    }

    #[test]
    fn refresh_config_does_not_reflap_a_persisting_modules_error_across_a_whole_file_failure() {
        let path = unique_temp_path("modules-error-survives-whole-file-failure");
        let working_separator_malformed_modules = "separator = \" | \"\nmodules = \"not-a-list\"\n";
        std::fs::write(&path, working_separator_malformed_modules).unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };

        app.refresh_config();
        assert_eq!(app.action_log.len(), 1);
        assert!(
            app.action_log[0].starts_with("Failed to read modules:"),
            "unexpected message: {}",
            app.action_log[0]
        );
        assert!(app.last_modules_error.is_some());
        let modules_error = app.last_modules_error.clone();

        std::fs::write(&path, "this is not valid toml [[[").unwrap();
        app.refresh_config();
        assert_eq!(app.action_log.len(), 2);
        assert!(
            app.action_log[1].starts_with("Failed to read config:"),
            "unexpected message: {}",
            app.action_log[1]
        );
        assert_eq!(app.last_modules_error, modules_error);

        std::fs::write(&path, working_separator_malformed_modules).unwrap();
        app.refresh_config();
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.action_log.len(), 2);
    }

    #[test]
    fn refresh_config_reload_clamps_selected_index_and_keeps_it_visible() {
        let path = unique_temp_path("reload-clamp-viewport");
        std::fs::write(
            &path,
            "separator = \" | \"\nmodules = [\"m0\", \"m1\", \"m2\", \"m3\", \"m4\"]\n",
        )
        .unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            selected_index: Some(4),
            module_scroll_offset: 0,
            modules_viewport_height: 2,
            ..App::default()
        };

        app.refresh_config();
        assert_eq!(app.selected_index, Some(4));
        assert_eq!(app.module_scroll_offset, 3);

        std::fs::write(&path, "separator = \" | \"\nmodules = [\"m0\"]\n").unwrap();
        app.refresh_config();
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.selected_index, Some(0));
        assert_eq!(app.module_scroll_offset, 0);
    }

    #[test]
    fn refresh_config_selected_index_resets_to_none_when_list_becomes_empty_then_to_zero_when_repopulated()
     {
        let path = unique_temp_path("selected-index-empty-cycle");
        std::fs::write(
            &path,
            "separator = \" | \"\nmodules = [\"cpu\", \"disk\"]\n",
        )
        .unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };
        app.refresh_config();
        assert_eq!(app.selected_index, Some(0));

        std::fs::write(&path, "separator = \" | \"\nmodules = []\n").unwrap();
        app.refresh_config();
        assert_eq!(app.selected_index, None);

        std::fs::write(&path, "separator = \" | \"\nmodules = [\"cpu\"]\n").unwrap();
        app.refresh_config();
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.selected_index, Some(0));
    }

    #[test]
    fn refresh_config_resets_selected_index_to_none_when_whole_file_load_fails() {
        let path = unique_temp_path("selected-index-whole-file-failure");
        std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };
        app.refresh_config();
        assert_eq!(app.selected_index, Some(0));

        std::fs::write(&path, "this is not valid toml [[[").unwrap();
        app.refresh_config();
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.modules, None);
        assert_eq!(app.selected_index, None);
    }

    #[test]
    fn refresh_config_resets_selected_index_to_none_when_modules_key_becomes_missing() {
        let path = unique_temp_path("selected-index-modules-key-missing");
        std::fs::write(&path, "modules = [\"cpu\", \"disk\"]\n").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            ..App::default()
        };
        app.refresh_config();
        assert_eq!(app.selected_index, Some(0));

        std::fs::write(&path, "separator = \" | \"\n").unwrap();
        app.refresh_config();
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.modules, None);
        assert_eq!(app.selected_index, None);
    }

    #[test]
    fn up_key_in_normal_mode_selects_previous_module() {
        let mut app = App {
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(1),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.selected_index, Some(0));
    }

    #[test]
    fn down_key_in_normal_mode_selects_next_module() {
        let mut app = App {
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(0),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.selected_index, Some(1));
    }

    #[test]
    fn select_previous_module_is_noop_at_top() {
        let mut app = App {
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(0),
            ..App::default()
        };
        app.select_previous_module();
        assert_eq!(app.selected_index, Some(0));
    }

    #[test]
    fn select_next_module_is_noop_at_bottom() {
        let mut app = App {
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(1),
            ..App::default()
        };
        app.select_next_module();
        assert_eq!(app.selected_index, Some(1));
    }

    #[test]
    fn select_next_module_is_noop_when_modules_is_none() {
        let mut app = App {
            modules: None,
            selected_index: None,
            ..App::default()
        };
        app.select_next_module();
        assert_eq!(app.selected_index, None);
    }

    #[test]
    fn select_next_module_pulls_scroll_offset_down_when_selection_moves_below_viewport() {
        let mut app = App {
            modules: Some(vec![
                "m0".to_string(),
                "m1".to_string(),
                "m2".to_string(),
                "m3".to_string(),
            ]),
            selected_index: Some(1),
            module_scroll_offset: 0,
            modules_viewport_height: 2,
            ..App::default()
        };
        app.select_next_module();
        assert_eq!(app.selected_index, Some(2));
        assert_eq!(app.module_scroll_offset, 1);
    }

    #[test]
    fn select_previous_module_pulls_scroll_offset_up_when_selection_moves_above_viewport() {
        let mut app = App {
            modules: Some(vec![
                "m0".to_string(),
                "m1".to_string(),
                "m2".to_string(),
                "m3".to_string(),
            ]),
            selected_index: Some(2),
            module_scroll_offset: 2,
            modules_viewport_height: 2,
            ..App::default()
        };
        app.select_previous_module();
        assert_eq!(app.selected_index, Some(1));
        assert_eq!(app.module_scroll_offset, 1);
    }

    #[test]
    fn move_module_up_swaps_with_previous_and_persists() {
        let path = unique_temp_path("move-up");
        std::fs::write(&path, "modules = [\"cpu\", \"disk\", \"battery\"]\n").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            modules: Some(vec![
                "cpu".to_string(),
                "disk".to_string(),
                "battery".to_string(),
            ]),
            selected_index: Some(1),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
        let content = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            app.modules,
            Some(vec![
                "disk".to_string(),
                "cpu".to_string(),
                "battery".to_string()
            ])
        );
        assert_eq!(app.selected_index, Some(0));
        assert_eq!(app.action_log, vec!["Moved disk up"]);
        let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
        let names: Vec<&str> = doc["modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["disk", "cpu", "battery"]);
    }

    #[test]
    fn move_module_down_swaps_with_next_and_persists() {
        let path = unique_temp_path("move-down");
        std::fs::write(&path, "modules = [\"cpu\", \"disk\", \"battery\"]\n").unwrap();
        let mut app = App {
            config_path: Some(path.clone()),
            modules: Some(vec![
                "cpu".to_string(),
                "disk".to_string(),
                "battery".to_string(),
            ]),
            selected_index: Some(1),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Down, KeyModifiers::CONTROL));
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            app.modules,
            Some(vec![
                "cpu".to_string(),
                "battery".to_string(),
                "disk".to_string()
            ])
        );
        assert_eq!(app.selected_index, Some(2));
        assert_eq!(app.action_log, vec!["Moved disk down"]);
    }

    #[test]
    fn move_module_up_at_top_is_noop_no_write_no_log() {
        let path = unique_temp_path("move-up-top-noop");
        let mut app = App {
            config_path: Some(path),
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(0),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
        assert_eq!(app.selected_index, Some(0));
        assert!(app.action_log.is_empty());
    }

    #[test]
    fn move_module_down_at_bottom_is_noop_no_write_no_log() {
        let path = unique_temp_path("move-down-bottom-noop");
        let mut app = App {
            config_path: Some(path),
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(1),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Down, KeyModifiers::CONTROL));
        assert_eq!(app.selected_index, Some(1));
        assert!(app.action_log.is_empty());
    }

    #[test]
    fn move_module_without_config_path_logs_failure() {
        let mut app = App {
            config_path: None,
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(1),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
        assert_eq!(
            app.action_log,
            vec!["cannot reorder modules: config path unknown"]
        );
        assert_eq!(
            app.modules,
            Some(vec!["cpu".to_string(), "disk".to_string()])
        );
        assert_eq!(app.selected_index, Some(1));
    }

    #[test]
    fn move_module_when_write_fails_logs_failure_and_leaves_state_unchanged() {
        let path = unique_temp_path("move-write-fails");
        let mut app = App {
            config_path: Some(path),
            modules: Some(vec!["cpu".to_string(), "disk".to_string()]),
            selected_index: Some(1),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
        assert_eq!(app.action_log.len(), 1);
        assert!(
            app.action_log[0].starts_with("Failed to reorder modules:"),
            "unexpected message: {}",
            app.action_log[0]
        );
        assert_eq!(
            app.modules,
            Some(vec!["cpu".to_string(), "disk".to_string()])
        );
        assert_eq!(app.selected_index, Some(1));
    }

    #[test]
    fn move_module_is_noop_when_modules_is_none() {
        let mut app = App {
            modules: None,
            selected_index: None,
            ..App::default()
        };
        app.handle_key(key(KeyCode::Up, KeyModifiers::CONTROL));
        assert!(app.action_log.is_empty());
    }

    #[test]
    fn enter_when_write_fails_logs_failure_and_returns_to_normal() {
        let path = unique_temp_path("nonexistent");
        let mut app = App {
            mode: Mode::EditingSeparator {
                buffer: " | ".to_string(),
                cursor: 3,
            },
            config_path: Some(path),
            ..App::default()
        };
        app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.action_log.len(), 1);
        assert!(
            app.action_log[0].starts_with("Failed to update separator:"),
            "unexpected message: {}",
            app.action_log[0]
        );
    }
}
