use crate::config::{BarConfig, ModuleSectionView, ParamWriteExpect};

use super::text::clamped_scroll_offset;
use super::{App, Mode, ModuleParamsState, ModuleParamsStatus, PanelFocus};

impl App {
    pub(in crate::tui) fn poll_config_changes(&mut self) {
        let reloaded = self
            .config_watcher
            .as_mut()
            .map(|watcher| watcher.try_reload())
            .unwrap_or(false);
        if reloaded {
            self.refresh_config();
        }
    }

    pub(super) fn refresh_config(&mut self) {
        let Some(path) = self.config_path.as_deref() else {
            return;
        };
        match BarConfig::load(path) {
            Ok(config) => {
                self.separator = Some(config.separator());
                self.last_separator_error = None;
                match config.module_names() {
                    Ok(names) => {
                        self.module_scroll_offset = self
                            .module_scroll_offset
                            .min(names.len().saturating_sub(self.modules_viewport_height));
                        let previous_entry = self
                            .selected_index
                            .and_then(|i| self.modules.as_ref().and_then(|m| m.get(i).cloned()));
                        self.selected_index = if names.is_empty() {
                            None
                        } else {
                            Some(self.selected_index.unwrap_or(0).min(names.len() - 1))
                        };
                        self.modules = Some(names);
                        self.last_modules_error = None;
                        self.ensure_selected_visible();
                        let new_entry = self
                            .selected_index
                            .and_then(|i| self.modules.as_ref().and_then(|m| m.get(i).cloned()));
                        let selection_changed = previous_entry != new_entry;
                        self.rebuild_module_params_from(&config, selection_changed);
                        if self.selected_index.is_none() {
                            self.panel_focus = PanelFocus::Modules;
                        }
                    }
                    Err(err) => {
                        self.modules = None;
                        self.selected_index = None;
                        self.module_params = None;
                        self.panel_focus = PanelFocus::Modules;
                        let message = err.to_string();
                        if self.last_modules_error.as_deref() != Some(message.as_str()) {
                            self.push_action_message(format!("Failed to read modules: {message}"));
                            self.last_modules_error = Some(message);
                        }
                    }
                }
                self.config_cache = Some(config);
            }
            Err(err) => {
                self.separator = None;
                self.modules = None;
                self.selected_index = None;
                self.module_params = None;
                self.config_cache = None;
                self.panel_focus = PanelFocus::Modules;
                let message = err.to_string();
                if self.last_separator_error.as_deref() != Some(message.as_str()) {
                    self.push_action_message(format!("Failed to read config: {message}"));
                    self.last_separator_error = Some(message);
                }
            }
        }
        self.drop_stale_confirming_remove_mode();
        self.drop_stale_param_modes();
    }

    pub(super) fn rebuild_module_params_from(&mut self, config: &BarConfig, reset_selection: bool) {
        let Some(idx) = self.selected_index else {
            self.module_params = None;
            return;
        };
        let Some(entry) = self.modules.as_ref().and_then(|m| m.get(idx)) else {
            self.module_params = None;
            return;
        };
        let section_name = BarConfig::split_module_entry(entry).1.to_string();
        let view = config.module_section_string_entries(&section_name);
        let (status, entries) = match view {
            ModuleSectionView::Missing => (
                ModuleParamsStatus::Missing {
                    section: section_name,
                },
                Vec::new(),
            ),
            ModuleSectionView::Empty => (ModuleParamsStatus::Empty, Vec::new()),
            ModuleSectionView::Entries(raw) => (ModuleParamsStatus::Entries, raw),
        };
        let selected_index = if entries.is_empty() {
            None
        } else if reset_selection {
            Some(0)
        } else {
            let prev = self
                .module_params
                .as_ref()
                .and_then(|p| p.selected_index)
                .unwrap_or(0);
            Some(prev.min(entries.len() - 1))
        };
        let scroll_offset = if reset_selection {
            0
        } else {
            self.module_params
                .as_ref()
                .map(|p| p.scroll_offset)
                .unwrap_or(0)
        };
        let mut state = ModuleParamsState {
            status,
            entries,
            selected_index,
            scroll_offset,
        };
        if let Some(sel) = state.selected_index {
            state.scroll_offset =
                clamped_scroll_offset(state.scroll_offset, sel, self.modules_viewport_height);
        }
        self.module_params = Some(state);
    }

    pub(super) fn rebuild_module_params(&mut self, reset_selection: bool) {
        let Some(config) = self.config_cache.take() else {
            self.module_params = None;
            return;
        };
        self.rebuild_module_params_from(&config, reset_selection);
        self.config_cache = Some(config);
    }

    pub(super) fn drop_stale_confirming_remove_mode(&mut self) {
        let Mode::ConfirmingRemove { index, name } = &self.mode else {
            return;
        };
        let still_armed = self
            .modules
            .as_ref()
            .and_then(|modules| modules.get(*index))
            .is_some_and(|current| current == name);
        if !still_armed {
            self.mode = Mode::Normal;
        }
    }

    pub(super) fn drop_stale_param_modes(&mut self) {
        let current_section = self.selected_section_name();
        let key_still_present = |key: &str| {
            self.module_params
                .as_ref()
                .is_some_and(|p| p.entries.iter().any(|(k, _)| k == key))
        };
        let should_drop = match &self.mode {
            Mode::AddingParamKey { section, .. } => {
                current_section.as_deref() != Some(section.as_str())
            }
            Mode::EditingParamValue {
                section,
                key,
                expect,
                ..
            } => {
                if current_section.as_deref() != Some(section.as_str()) {
                    true
                } else if matches!(expect, ParamWriteExpect::KeyAbsent) {
                    key_still_present(key)
                } else {
                    !key_still_present(key)
                }
            }
            Mode::ConfirmingRemoveParam { section, key }
            | Mode::RenamingParamKey {
                section,
                old_key: key,
                ..
            } => current_section.as_deref() != Some(section.as_str()) || !key_still_present(key),
            _ => false,
        };
        if should_drop {
            self.mode = Mode::Normal;
        }
    }
}
