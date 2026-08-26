use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::config::{BarConfig, ModuleParamValue, ParamWriteExpect};

use super::text::{apply_text_edit, clamped_scroll_offset};
use super::{App, DetailContext, Mode, PanelFocus, ParamEntry, ParamOrigin};

impl App {
    pub(super) fn focus_params(&mut self) {
        match self.panel_focus {
            PanelFocus::Extensions => {
                if self.extension_selected_index.is_some() {
                    self.detail_context = DetailContext::Extension;
                    self.panel_focus = PanelFocus::Params;
                }
            }
            PanelFocus::Modules if self.selected_index.is_some() => {
                self.detail_context = DetailContext::Module;
                self.panel_focus = PanelFocus::Params;
            }
            _ => {}
        }
    }

    pub(super) fn ensure_selected_param_visible(&mut self) {
        let viewport_height = self
            .params_viewport_height
            .saturating_sub(self.detail_header_line_count());
        let Some(params) = self.module_params.as_mut() else {
            return;
        };
        let Some(idx) = params.selected_index else {
            return;
        };
        params.scroll_offset = clamped_scroll_offset(params.scroll_offset, idx, viewport_height);
    }

    pub(super) fn select_previous_param(&mut self) {
        let Some(params) = self.module_params.as_mut() else {
            return;
        };
        let Some(idx) = params.selected_index else {
            return;
        };
        if idx > 0 {
            params.selected_index = Some(idx - 1);
            self.ensure_selected_param_visible();
        }
    }

    pub(super) fn select_next_param(&mut self) {
        let Some(params) = self.module_params.as_mut() else {
            return;
        };
        let Some(idx) = params.selected_index else {
            return;
        };
        if idx + 1 >= params.entries.len() {
            return;
        }
        params.selected_index = Some(idx + 1);
        self.ensure_selected_param_visible();
    }

    pub(super) fn selected_section_name(&self) -> Option<String> {
        let idx = self.selected_index?;
        let entry = self.modules.as_ref()?.get(idx)?;
        Some(BarConfig::split_module_entry(entry).1.to_string())
    }

    pub(super) fn selected_param_entry(&self) -> Option<&ParamEntry> {
        let params = self.module_params.as_ref()?;
        let idx = params.selected_index?;
        params.entries.get(idx)
    }

    fn reject_default_origin_mutation(&mut self, key: &str) {
        self.push_action_message(format!("{key} is not set in config yet; edit it first"));
    }

    pub(super) fn begin_add_param(&mut self) {
        let Some(section) = self.selected_section_name() else {
            return;
        };
        if self.config_path.is_none() {
            self.push_action_message("cannot add param: config path unknown".to_string());
            return;
        }
        self.mode = Mode::AddingParamKey {
            section,
            buffer: String::new(),
            cursor: 0,
        };
    }

    pub(super) fn begin_edit_param_value(&mut self) {
        let Some(section) = self.selected_section_name() else {
            return;
        };
        let Some(ParamEntry { key, value, origin }) = self.selected_param_entry().cloned() else {
            return;
        };
        if self.config_path.is_none() {
            self.push_action_message("cannot edit param: config path unknown".to_string());
            return;
        }
        let (buffer, expect) = match origin {
            ParamOrigin::Default => {
                let ModuleParamValue::String(default) = value else {
                    return;
                };
                (default, ParamWriteExpect::KeyAbsent)
            }
            ParamOrigin::Explicit => match value {
                ModuleParamValue::String(s) => {
                    let expect = ParamWriteExpect::ExistingString(s.clone());
                    (s, expect)
                }
                ModuleParamValue::NonString => (String::new(), ParamWriteExpect::ExistingNonString),
            },
        };
        let cursor = buffer.chars().count();
        self.mode = Mode::EditingParamValue {
            section,
            key,
            buffer,
            cursor,
            expect,
        };
    }

    pub(super) fn begin_remove_param(&mut self) {
        let Some(section) = self.selected_section_name() else {
            return;
        };
        let Some(ParamEntry { key, origin, .. }) = self.selected_param_entry().cloned() else {
            return;
        };
        if origin == ParamOrigin::Default {
            self.reject_default_origin_mutation(&key);
            return;
        }
        if self.config_path.is_none() {
            self.push_action_message("cannot remove param: config path unknown".to_string());
            return;
        }
        self.mode = Mode::ConfirmingRemoveParam { section, key };
    }

    pub(super) fn begin_rename_param(&mut self) {
        let Some(section) = self.selected_section_name() else {
            return;
        };
        let Some(ParamEntry {
            key: old_key,
            origin,
            ..
        }) = self.selected_param_entry().cloned()
        else {
            return;
        };
        if origin == ParamOrigin::Default {
            self.reject_default_origin_mutation(&old_key);
            return;
        }
        if self.config_path.is_none() {
            self.push_action_message("cannot rename param: config path unknown".to_string());
            return;
        }
        let buffer = old_key.clone();
        let cursor = buffer.chars().count();
        self.mode = Mode::RenamingParamKey {
            section,
            old_key,
            buffer,
            cursor,
        };
    }

    pub(super) fn handle_key_adding_param_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => self.commit_adding_param_key(),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Char(_) => {
                if let Mode::AddingParamKey { buffer, cursor, .. } = &mut self.mode {
                    apply_text_edit(buffer, cursor, key);
                }
            }
            _ => {}
        }
    }

    pub(super) fn commit_adding_param_key(&mut self) {
        let Mode::AddingParamKey {
            section,
            buffer,
            cursor: _,
        } = &self.mode
        else {
            return;
        };
        if buffer.is_empty() {
            self.push_action_message("Param key cannot be empty".to_string());
            return;
        }
        if !crate::schema::is_valid_name(buffer) {
            self.push_action_message(format!(
                "Invalid param key `{buffer}`: use only A-Z, a-z, 0-9, _, -"
            ));
            return;
        }
        if self
            .module_params
            .as_ref()
            .is_some_and(|p| p.entries.iter().any(|e| e.key == *buffer))
        {
            self.push_action_message(format!("Param key `{buffer}` already exists"));
            return;
        }
        let section = section.clone();
        let key = buffer.clone();
        self.mode = Mode::EditingParamValue {
            section,
            key,
            buffer: String::new(),
            cursor: 0,
            expect: ParamWriteExpect::KeyAbsent,
        };
    }

    pub(super) fn handle_key_editing_param_value(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_editing_param_value(),
            KeyCode::Enter => self.commit_editing_param_value(),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Char(_) => {
                if let Mode::EditingParamValue { buffer, cursor, .. } = &mut self.mode {
                    apply_text_edit(buffer, cursor, key);
                }
            }
            _ => {}
        }
    }

    pub(super) fn cancel_editing_param_value(&mut self) {
        let Mode::EditingParamValue {
            section,
            key,
            expect,
            ..
        } = std::mem::take(&mut self.mode)
        else {
            return;
        };
        if matches!(expect, ParamWriteExpect::KeyAbsent) {
            let cursor = key.chars().count();
            self.mode = Mode::AddingParamKey {
                section,
                buffer: key,
                cursor,
            };
        } else {
            self.mode = Mode::Normal;
        }
    }

    pub(super) fn commit_editing_param_value(&mut self) {
        let Mode::EditingParamValue {
            section,
            key,
            buffer,
            expect,
            ..
        } = std::mem::take(&mut self.mode)
        else {
            return;
        };
        self.ensure_preset_pointer_current();
        let Some(path) = self.config_path.clone() else {
            self.push_action_message("cannot save param: config path unknown".to_string());
            return;
        };
        let is_add = matches!(expect, ParamWriteExpect::KeyAbsent);
        let keep_key = if is_add {
            self.selected_param_entry().map(|e| e.key.clone())
        } else {
            Some(key.clone())
        };
        match BarConfig::write_module_param_set(&path, &section, &key, &buffer, &expect) {
            Ok(()) => {
                let log = if is_add {
                    format!("Added {key}")
                } else {
                    format!("Updated {key}")
                };
                self.reload_params_after_write();
                if let Some(prefer) = keep_key {
                    self.select_param_by_key(&prefer);
                } else {
                    self.select_param_by_key(&key);
                }
                self.notify_daemon_config_reload();
                self.push_action_message(log);
            }
            Err(err) => {
                let verb = if is_add { "add" } else { "update" };
                self.push_action_message(format!("Failed to {verb} {key}: {err}"));
            }
        }
    }

    pub(super) fn handle_key_confirming_remove_param(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('d') => self.commit_remove_param(),
            _ => self.mode = Mode::Normal,
        }
    }

    pub(super) fn commit_remove_param(&mut self) {
        let Mode::ConfirmingRemoveParam { section, key } = std::mem::take(&mut self.mode) else {
            return;
        };
        self.ensure_preset_pointer_current();
        let Some(path) = self.config_path.clone() else {
            self.push_action_message("cannot remove param: config path unknown".to_string());
            return;
        };
        match BarConfig::write_module_param_remove(&path, &section, &key) {
            Ok(()) => {
                self.reload_params_after_write();
                self.notify_daemon_config_reload();
                self.push_action_message(format!("Removed {key}"));
            }
            Err(err) => self.push_action_message(format!("Failed to remove {key}: {err}")),
        }
    }

    pub(super) fn handle_key_renaming_param_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => self.commit_rename_param(),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Char(_) => {
                if let Mode::RenamingParamKey { buffer, cursor, .. } = &mut self.mode {
                    apply_text_edit(buffer, cursor, key);
                }
            }
            _ => {}
        }
    }

    pub(super) fn commit_rename_param(&mut self) {
        let (section, old_key, new_key) = match &self.mode {
            Mode::RenamingParamKey {
                section,
                old_key,
                buffer,
                ..
            } => (section.clone(), old_key.clone(), buffer.clone()),
            _ => return,
        };
        if new_key.is_empty() {
            self.push_action_message("Param key cannot be empty".to_string());
            return;
        }
        if !crate::schema::is_valid_name(&new_key) {
            self.push_action_message(format!(
                "Invalid param key `{new_key}`: use only A-Z, a-z, 0-9, _, -"
            ));
            return;
        }
        if new_key == old_key {
            self.mode = Mode::Normal;
            return;
        }
        if self
            .module_params
            .as_ref()
            .is_some_and(|p| p.entries.iter().any(|e| e.key == new_key))
        {
            self.push_action_message(format!("Param key `{new_key}` already exists"));
            return;
        }
        self.mode = Mode::Normal;
        self.ensure_preset_pointer_current();
        let Some(path) = self.config_path.clone() else {
            self.push_action_message("cannot rename param: config path unknown".to_string());
            return;
        };
        match BarConfig::write_module_param_rename(&path, &section, &old_key, &new_key) {
            Ok(()) => {
                self.reload_params_after_write();
                self.select_param_by_key(&new_key);
                self.notify_daemon_config_reload();
                self.push_action_message(format!("Renamed {old_key} → {new_key}"));
            }
            Err(err) => self.push_action_message(format!("Failed to rename {old_key}: {err}")),
        }
    }

    pub(super) fn reload_params_after_write(&mut self) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        match BarConfig::load(&path) {
            Ok(config) => {
                self.rebuild_module_params_from(&config, false);
                self.config_cache = Some(config);
            }
            Err(err) => {
                self.push_action_message(format!("Failed to reload config: {err}"));
            }
        }
    }

    pub(super) fn select_param_by_key(&mut self, key: &str) {
        let Some(params) = self.module_params.as_mut() else {
            return;
        };
        if let Some(i) = params.entries.iter().position(|e| e.key == key) {
            params.selected_index = Some(i);
            self.ensure_selected_param_visible();
        }
    }
}
