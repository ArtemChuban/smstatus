use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::bindings::ConfigParam;
use crate::config::{
    BarConfig, ModuleParamValue, ParamWriteExpect, active_config_path, read_active_name,
};
use crate::manifest::{Metadata, RequiredExtension};
use crate::schema_probe::SchemaProbe;

mod daemon;
mod extensions;
mod help;
mod logs;
mod modules;
mod params;
mod presets;
mod reload;
mod requirement_status;
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
    ChoosingPreset {
        names: Vec<String>,
        selected: usize,
        scroll_offset: usize,
    },
    NamingPreset {
        buffer: String,
        cursor: usize,
    },
    ConfirmingRemovePreset {
        name: String,
    },
    RenamingParamKey {
        section: String,
        old_key: String,
        buffer: String,
        cursor: usize,
    },
    ChoosingInstallKind {
        selected: usize,
    },
    EnteringInstallSource {
        target: InstallTarget,
        buffer: String,
        cursor: usize,
    },
    Help,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum InstallTarget {
    Module,
    Extension,
}

#[derive(Default, PartialEq, Eq, Debug, Clone, Copy)]
pub(super) enum DetailContext {
    #[default]
    Module,
    Extension,
}

#[derive(Default, PartialEq, Eq, Debug, Clone, Copy)]
pub(super) enum PanelFocus {
    #[default]
    Modules,
    Extensions,
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
    pub(super) config_dir: Option<PathBuf>,
    pub(super) active_preset: Option<String>,
    pub(super) config_path: Option<PathBuf>,
    pub(super) modules_dir: Option<PathBuf>,
    pub(super) extensions_dir: Option<PathBuf>,
    pub(super) separator: Option<String>,
    pub(super) last_separator_error: Option<String>,
    pub(super) modules: Option<Vec<String>>,
    pub(super) installed_extensions: Vec<String>,
    pub(super) extension_overlay_labels: Vec<String>,
    pub(super) required_extensions_by_kind: HashMap<String, Vec<RequiredExtension>>,
    pub(super) requirement_lines_by_kind: HashMap<String, Vec<String>>,
    pub(super) metadata_by_kind: HashMap<String, Metadata>,
    pub(super) metadata_failed: HashSet<String>,
    pub(super) metadata_needs_stable: HashSet<String>,
    pub(super) schema_by_kind: HashMap<String, Vec<ConfigParam>>,
    pub(super) schema_failed: HashSet<String>,
    pub(super) schema_needs_stable: HashSet<String>,
    pub(super) schema_probe: Option<SchemaProbe>,
    pub(super) last_modules_error: Option<String>,
    pub(super) module_scroll_offset: usize,
    pub(super) modules_viewport_height: usize,
    pub(super) extensions_viewport_height: usize,
    pub(super) params_viewport_height: usize,
    pub(super) extension_scroll_offset: usize,
    pub(super) overlay_viewport_height: usize,
    pub(super) selected_index: Option<usize>,
    pub(super) extension_selected_index: Option<usize>,
    pub(super) panel_focus: PanelFocus,
    pub(super) detail_context: DetailContext,
    pub(super) module_params: Option<ModuleParamsState>,
    pub(super) help_scroll_offset: usize,
    pub(super) config_cache: Option<BarConfig>,
    pub(super) logs_scroll_offset: usize,
    pub(super) logs_selected_index: Option<usize>,
    pub(super) logs_follow: bool,
    pub(super) logs_viewport_height: usize,
    pub(super) logs_loaded_from: usize,
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
            config_dir: None,
            active_preset: None,
            config_path: None,
            modules_dir: None,
            extensions_dir: None,
            separator: None,
            last_separator_error: None,
            modules: None,
            installed_extensions: Vec::new(),
            extension_overlay_labels: Vec::new(),
            required_extensions_by_kind: HashMap::new(),
            requirement_lines_by_kind: HashMap::new(),
            metadata_by_kind: HashMap::new(),
            metadata_failed: HashSet::new(),
            metadata_needs_stable: HashSet::new(),
            schema_by_kind: HashMap::new(),
            schema_failed: HashSet::new(),
            schema_needs_stable: HashSet::new(),
            schema_probe: None,
            last_modules_error: None,
            module_scroll_offset: 0,
            modules_viewport_height: 0,
            extensions_viewport_height: 0,
            params_viewport_height: 0,
            extension_scroll_offset: 0,
            overlay_viewport_height: 0,
            selected_index: None,
            extension_selected_index: None,
            panel_focus: PanelFocus::default(),
            detail_context: DetailContext::default(),
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
                app.config_dir = Some(config_dir.clone());
                app.modules_dir = Some(config_dir.join("modules"));
                app.extensions_dir = Some(config_dir.join("extensions"));

                match active_config_path(&config_dir) {
                    Ok(path) => {
                        app.config_path = Some(path.clone());
                        app.active_preset = read_active_name(&config_dir).ok();
                        let log_days = BarConfig::load(&path)
                            .map(|config| config.log_days())
                            .unwrap_or(7);
                        if let Err(err) = crate::logging::init(log_days) {
                            crate::logging::to_stderr(
                                log::Level::Error,
                                &format!("failed to initialize logging: {err}"),
                            );
                        }
                    }
                    Err(err) => {
                        app.push_action_message(format!("{err}"));
                        if let Err(init_err) = crate::logging::init(7) {
                            crate::logging::to_stderr(
                                log::Level::Error,
                                &format!("failed to initialize logging: {init_err}"),
                            );
                        }
                    }
                }
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
            Mode::ChoosingInstallKind { .. } => self.handle_key_choosing_install_kind(key),
            Mode::EnteringInstallSource { .. } => self.handle_key_entering_install_source(key),
            Mode::ChoosingPreset { .. } => self.handle_key_choosing_preset(key),
            Mode::NamingPreset { .. } => self.handle_key_naming_preset(key),
            Mode::ConfirmingRemovePreset { .. } => self.handle_key_confirming_remove_preset(key),
            Mode::Help => self.handle_key_help(key),
        }
    }

    fn handle_key_normal(&mut self, key: KeyEvent) {
        self.ensure_preset_pointer_current();
        if is_quit(key) {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('p') => {
                self.begin_manage_presets();
                return;
            }
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
            PanelFocus::Extensions => self.handle_key_normal_extensions(key),
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
            KeyCode::Char('x') => self.focus_extensions(),
            KeyCode::Char('i') => self.begin_install(),
            KeyCode::Enter | KeyCode::Right => self.focus_params(),
            KeyCode::Tab => self.focus_extensions(),
            _ => {}
        }
    }

    fn handle_key_normal_params(&mut self, key: KeyEvent) {
        if self.detail_context == DetailContext::Extension {
            match key.code {
                KeyCode::Esc | KeyCode::Left => self.panel_focus = PanelFocus::Extensions,
                KeyCode::Tab => self.focus_logs(),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('a') => self.begin_add_param(),
            KeyCode::Char('d') => self.begin_remove_param(),
            KeyCode::Char('e') | KeyCode::Enter => self.begin_edit_param_value(),
            KeyCode::Char('r') => self.begin_rename_param(),
            KeyCode::Esc | KeyCode::Left => {
                self.panel_focus = match self.detail_context {
                    DetailContext::Extension => PanelFocus::Extensions,
                    DetailContext::Module => PanelFocus::Modules,
                };
            }
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
