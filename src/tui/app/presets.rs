use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::config::{list_preset_names, read_active_name};
use crate::preset::{UsePresetOutcome, remove_preset_in, save_preset_in, use_preset_in};

use super::text::{apply_text_edit, clamped_scroll_offset};
use super::{App, Mode};

impl App {
    pub(super) fn begin_manage_presets(&mut self) {
        self.ensure_preset_pointer_current();
        let Some(config_dir) = self.config_dir.clone() else {
            self.push_action_message("cannot manage presets: config directory unknown".to_string());
            return;
        };
        match list_preset_names(&config_dir) {
            Ok(names) if names.is_empty() => {
                self.push_action_message(
                    "no presets found; run smstatus init or save one here".to_string(),
                );
            }
            Ok(names) => {
                self.mode = Mode::ChoosingPreset {
                    names,
                    selected: 0,
                    scroll_offset: 0,
                };
            }
            Err(err) => self.push_action_message(format!("Failed to list presets: {err}")),
        }
    }

    pub(super) fn handle_key_choosing_preset(&mut self, key: KeyEvent) {
        self.ensure_preset_pointer_current();
        let Mode::ChoosingPreset {
            names,
            selected,
            scroll_offset,
        } = &mut self.mode
        else {
            return;
        };
        let active = self
            .config_dir
            .as_deref()
            .and_then(|dir| read_active_name(dir).ok());
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Up => {
                *selected = selected.saturating_sub(1);
                *scroll_offset =
                    clamped_scroll_offset(*scroll_offset, *selected, self.overlay_viewport_height);
            }
            KeyCode::Down => {
                if !names.is_empty() {
                    *selected = (*selected + 1).min(names.len() - 1);
                }
                *scroll_offset =
                    clamped_scroll_offset(*scroll_offset, *selected, self.overlay_viewport_height);
            }
            KeyCode::Enter => {
                let name = names.get(*selected).cloned();
                if let Some(name) = name {
                    self.switch_preset(&name);
                }
            }
            KeyCode::Char('a') => {
                self.mode = Mode::NamingPreset {
                    buffer: String::new(),
                    cursor: 0,
                };
            }
            KeyCode::Char('d') => {
                let Some(name) = names.get(*selected).cloned() else {
                    return;
                };
                if active.as_deref() == Some(name.as_str()) {
                    self.push_action_message(format!("cannot remove active preset `{name}`"));
                    return;
                }
                self.mode = Mode::ConfirmingRemovePreset { name };
            }
            _ => {}
        }
    }

    pub(super) fn handle_key_naming_preset(&mut self, key: KeyEvent) {
        let Mode::NamingPreset { buffer, cursor } = &mut self.mode else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.return_to_choosing_preset_list();
                return;
            }
            KeyCode::Enter => {}
            _ => {
                apply_text_edit(buffer, cursor, key);
                return;
            }
        }
        self.commit_save_preset();
    }

    pub(super) fn handle_key_confirming_remove_preset(&mut self, key: KeyEvent) {
        let Mode::ConfirmingRemovePreset { name } = &self.mode else {
            return;
        };
        let name = name.clone();
        match key.code {
            KeyCode::Char('d') => self.remove_preset(&name),
            _ => self.return_to_choosing_preset_list(),
        }
    }

    fn return_to_choosing_preset_list(&mut self) {
        let Some(config_dir) = self.config_dir.as_deref() else {
            self.mode = Mode::Normal;
            return;
        };
        match list_preset_names(config_dir) {
            Ok(names) if names.is_empty() => self.mode = Mode::Normal,
            Ok(names) => {
                self.mode = Mode::ChoosingPreset {
                    selected: 0,
                    scroll_offset: 0,
                    names,
                };
            }
            Err(_) => self.mode = Mode::Normal,
        }
    }

    fn commit_save_preset(&mut self) {
        let Mode::NamingPreset { buffer, .. } = std::mem::take(&mut self.mode) else {
            return;
        };
        let name = buffer.trim();
        if name.is_empty() {
            self.push_action_message("preset name cannot be empty".to_string());
            self.return_to_choosing_preset_list();
            return;
        }
        let Some(config_dir) = self.config_dir.clone() else {
            self.push_action_message("cannot save preset: config directory unknown".to_string());
            self.mode = Mode::Normal;
            return;
        };
        match save_preset_in(&config_dir, name) {
            Ok(()) => {
                self.push_action_message(format!("saved preset `{name}`"));
                self.return_to_choosing_preset_list();
            }
            Err(err) => {
                self.push_action_message(format!("Failed to save preset: {err}"));
                self.return_to_choosing_preset_list();
            }
        }
    }

    pub(super) fn switch_preset(&mut self, name: &str) {
        let Some(config_dir) = self.config_dir.clone() else {
            self.push_action_message("cannot switch preset: config directory unknown".to_string());
            return;
        };
        match use_preset_in(&config_dir, name, true) {
            Ok(outcome) => {
                self.sync_preset_pointer_from_disk();
                self.refresh_config();
                self.push_action_message(format!("active preset set to `{name}`"));
                if matches!(outcome, UsePresetOutcome::ReloadNotRunning) {
                    self.push_action_message(
                        "smstatus is not running; config saved but bar not updated".to_string(),
                    );
                }
            }
            Err(err) => self.push_action_message(format!("Failed to switch preset: {err}")),
        }
        if matches!(self.mode, Mode::ChoosingPreset { .. }) {
            self.return_to_choosing_preset_list();
        }
    }

    pub(super) fn remove_preset(&mut self, name: &str) {
        let Some(config_dir) = self.config_dir.clone() else {
            self.push_action_message("cannot remove preset: config directory unknown".to_string());
            self.mode = Mode::Normal;
            return;
        };
        match remove_preset_in(&config_dir, name) {
            Ok(()) => {
                self.push_action_message(format!("removed preset `{name}`"));
                self.return_to_choosing_preset_list();
            }
            Err(err) => {
                self.push_action_message(format!("Failed to remove preset: {err}"));
                self.return_to_choosing_preset_list();
            }
        }
    }
}
