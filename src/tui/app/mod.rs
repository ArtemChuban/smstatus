use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{BarConfig, ModuleParamValue, ParamWriteExpect};

mod daemon;
mod help;
mod modules;
mod params;
mod reload;
mod separator;
mod text;

use text::{is_hard_quit, is_quit};

pub(super) const LOGS_PANEL_LINES: usize = 3;

#[derive(Default, PartialEq, Eq, Debug)]
pub(super) enum Mode {
    #[default]
    Normal,
    EditingSeparator {
        buffer: String,
        cursor: usize,
    },
    AddingModule {
        available: Vec<String>,
        selected: usize,
        scroll_offset: usize,
    },
    NamingModuleInstance {
        kind: String,
        buffer: String,
        cursor: usize,
    },
    ConfirmingRemove {
        index: usize,
        name: String,
    },
    AddingParamKey {
        section: String,
        buffer: String,
        cursor: usize,
    },
    EditingParamValue {
        section: String,
        key: String,
        buffer: String,
        cursor: usize,
        expect: ParamWriteExpect,
    },
    ConfirmingRemoveParam {
        section: String,
        key: String,
    },
    RenamingParamKey {
        section: String,
        old_key: String,
        buffer: String,
        cursor: usize,
    },
    Help,
}

#[derive(Default, PartialEq, Eq, Debug, Clone, Copy)]
pub(super) enum PanelFocus {
    #[default]
    Modules,
    Params,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ModuleParamsStatus {
    Missing { section: String },
    Empty,
    Entries,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModuleParamsState {
    pub(super) status: ModuleParamsStatus,
    pub(super) entries: Vec<(String, ModuleParamValue)>,
    pub(super) selected_index: Option<usize>,
    pub(super) scroll_offset: usize,
}

#[derive(Default)]
pub(super) struct App {
    pub(super) should_quit: bool,
    pub(super) daemon_status: Option<crate::daemon::DaemonStatus>,
    pub(super) pending_start: Option<std::process::Child>,
    pub(super) pending_start_confirmed_running: bool,
    pub(super) mode: Mode,
    pub(super) config_path: Option<PathBuf>,
    pub(super) modules_dir: Option<PathBuf>,
    pub(super) separator: Option<String>,
    pub(super) config_watcher: Option<crate::watcher::ReloadWatcher>,
    pub(super) last_separator_error: Option<String>,
    pub(super) modules: Option<Vec<String>>,
    pub(super) last_modules_error: Option<String>,
    pub(super) module_scroll_offset: usize,
    pub(super) modules_viewport_height: usize,
    pub(super) overlay_viewport_height: usize,
    pub(super) selected_index: Option<usize>,
    pub(super) panel_focus: PanelFocus,
    pub(super) module_params: Option<ModuleParamsState>,
    pub(super) help_scroll_offset: usize,
    pub(super) config_cache: Option<BarConfig>,
}

impl App {
    pub(in crate::tui) fn new() -> Self {
        let mut app = Self::default();
        match crate::config::default_config_dir() {
            Ok(config_dir) => {
                let config_path = config_dir.join("config.toml");
                let modules_dir = config_dir.join("modules");
                let log_days = BarConfig::load(&config_path)
                    .map(|config| config.log_days())
                    .unwrap_or(7);
                if let Err(err) = crate::logging::init(log_days) {
                    let message = format!("failed to initialize logging: {err}");
                    eprintln!("{message}");
                    crate::logging::append_message(log::Level::Error, &message);
                }
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
                app.modules_dir = Some(config_dir.join("modules"));
                app.refresh_config();
            }
            Err(err) => app.push_action_message(format!("could not determine config path: {err}")),
        }
        app
    }

    pub(in crate::tui) fn handle_key(&mut self, key: KeyEvent) {
        if is_hard_quit(key) {
            self.should_quit = true;
            return;
        }
        match self.mode {
            Mode::EditingSeparator { .. } => self.handle_key_editing_separator(key),
            Mode::Normal => self.handle_key_normal(key),
            Mode::AddingModule { .. } => self.handle_key_adding_module(key),
            Mode::NamingModuleInstance { .. } => self.handle_key_naming_module_instance(key),
            Mode::ConfirmingRemove { .. } => self.handle_key_confirming_remove(key),
            Mode::AddingParamKey { .. } => self.handle_key_adding_param_key(key),
            Mode::EditingParamValue { .. } => self.handle_key_editing_param_value(key),
            Mode::ConfirmingRemoveParam { .. } => self.handle_key_confirming_remove_param(key),
            Mode::RenamingParamKey { .. } => self.handle_key_renaming_param_key(key),
            Mode::Help => self.handle_key_help(key),
        }
    }

    fn handle_key_normal(&mut self, key: KeyEvent) {
        if is_quit(key) {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('s') => {
                self.start_daemon();
                return;
            }
            KeyCode::Char('k') => {
                self.stop_daemon();
                return;
            }
            KeyCode::Char('?') => {
                self.help_scroll_offset = 0;
                self.mode = Mode::Help;
                return;
            }
            _ => {}
        }
        match self.panel_focus {
            PanelFocus::Modules => self.handle_key_normal_modules(key),
            PanelFocus::Params => self.handle_key_normal_params(key),
        }
    }

    fn handle_key_normal_modules(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('e') => self.begin_edit_separator(),
            KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => self.move_module_up(),
            KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_module_down()
            }
            KeyCode::Up => self.select_previous_module(),
            KeyCode::Down => self.select_next_module(),
            KeyCode::Char('a') => self.begin_add_module(),
            KeyCode::Char('d') => self.begin_remove_module(),
            KeyCode::Enter | KeyCode::Right => self.focus_params(),
            _ => {}
        }
    }

    fn handle_key_normal_params(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('a') => self.begin_add_param(),
            KeyCode::Char('d') => self.begin_remove_param(),
            KeyCode::Char('e') | KeyCode::Enter => self.begin_edit_param_value(),
            KeyCode::Char('r') => self.begin_rename_param(),
            KeyCode::Esc | KeyCode::Left => self.panel_focus = PanelFocus::Modules,
            KeyCode::Up => self.select_previous_param(),
            KeyCode::Down => self.select_next_param(),
            _ => {}
        }
    }

    fn push_action_message(&mut self, message: String) {
        if log::log_enabled!(log::Level::Info) {
            log::info!("{message}");
        } else {
            crate::logging::append_message(log::Level::Info, &message);
        }
    }
}

#[cfg(test)]
mod tests;
