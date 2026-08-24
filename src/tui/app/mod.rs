use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::bindings::{ConfigParam, Metadata};
use crate::config::{BarConfig, ModuleParamValue, ParamWriteExpect};
use crate::meta::MetadataProbe;
use crate::schema_probe::SchemaProbe;

mod daemon;
mod help;
mod logs;
mod modules;
mod params;
mod reload;
mod separator;
mod text;

use text::{is_hard_quit, is_quit};

pub(super) const LOGS_CHUNK_LINES: usize = 200;

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
    Logs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ModuleParamsStatus {
    Missing { section: String },
    Empty,
    Entries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParamOrigin {
    Explicit,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParamEntry {
    pub(super) key: String,
    pub(super) value: ModuleParamValue,
    pub(super) origin: ParamOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ModuleParamsState {
    pub(super) status: ModuleParamsStatus,
    pub(super) entries: Vec<ParamEntry>,
    pub(super) selected_index: Option<usize>,
    pub(super) scroll_offset: usize,
}

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
    pub(super) metadata_by_kind: HashMap<String, Metadata>,
    pub(super) metadata_failed: HashSet<String>,
    pub(super) metadata_needs_stable: HashSet<String>,
    pub(super) metadata_probe: Option<MetadataProbe>,
    pub(super) schema_by_kind: HashMap<String, Vec<ConfigParam>>,
    pub(super) schema_failed: HashSet<String>,
    pub(super) schema_needs_stable: HashSet<String>,
    pub(super) schema_probe: Option<SchemaProbe>,
    pub(super) last_modules_error: Option<String>,
    pub(super) module_scroll_offset: usize,
    pub(super) modules_viewport_height: usize,
    pub(super) overlay_viewport_height: usize,
    pub(super) selected_index: Option<usize>,
    pub(super) panel_focus: PanelFocus,
    pub(super) module_params: Option<ModuleParamsState>,
    pub(super) help_scroll_offset: usize,
    pub(super) config_cache: Option<BarConfig>,
    pub(super) logs_scroll_offset: usize,
    pub(super) logs_selected_index: Option<usize>,
    pub(super) logs_follow: bool,
    pub(super) logs_viewport_height: usize,
    /// Absolute index of `log_history[0]` in the full log file.
    pub(super) logs_loaded_from: usize,
    /// Total non-empty lines in the log file (not just the loaded window).
    pub(super) logs_total: usize,
    pub(super) logs_file_total: usize,
    pub(super) logs_show_error: bool,
    pub(super) logs_show_warn: bool,
    pub(super) logs_show_info: bool,
    pub(in crate::tui) log_history: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            should_quit: false,
            daemon_status: None,
            pending_start: None,
            pending_start_confirmed_running: false,
            mode: Mode::default(),
            config_path: None,
            modules_dir: None,
            separator: None,
            config_watcher: None,
            last_separator_error: None,
            modules: None,
            metadata_by_kind: HashMap::new(),
            metadata_failed: HashSet::new(),
            metadata_needs_stable: HashSet::new(),
            metadata_probe: None,
            schema_by_kind: HashMap::new(),
            schema_failed: HashSet::new(),
            schema_needs_stable: HashSet::new(),
            schema_probe: None,
            last_modules_error: None,
            module_scroll_offset: 0,
            modules_viewport_height: 0,
            overlay_viewport_height: 0,
            selected_index: None,
            panel_focus: PanelFocus::default(),
            module_params: None,
            help_scroll_offset: 0,
            config_cache: None,
            logs_scroll_offset: 0,
            logs_selected_index: None,
            logs_follow: true,
            logs_viewport_height: 0,
            logs_loaded_from: 0,
            logs_total: 0,
            logs_file_total: 0,
            logs_show_error: true,
            logs_show_warn: true,
            logs_show_info: true,
            log_history: Vec::new(),
        }
    }
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
                    crate::logging::to_stderr(
                        log::Level::Error,
                        &format!("failed to initialize logging: {err}"),
                    );
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
            PanelFocus::Logs => self.handle_key_normal_logs(key),
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
            KeyCode::Tab => self.focus_logs(),
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
            KeyCode::Tab => self.focus_logs(),
            _ => {}
        }
    }

    fn push_action_message(&mut self, message: String) {
        crate::logging::log_message(log::Level::Info, &message);
    }
}

#[cfg(test)]
mod tests;
