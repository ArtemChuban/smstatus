use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::clamped_scroll_offset;
use super::{App, Mode, PanelFocus};

impl App {
    pub(super) fn ensure_selected_visible(&mut self) {
        let Some(idx) = self.selected_index else {
            return;
        };
        self.module_scroll_offset =
            clamped_scroll_offset(self.module_scroll_offset, idx, self.modules_viewport_height);
    }

    pub(super) fn select_previous_module(&mut self) {
        let Some(modules) = self.modules.as_ref() else {
            return;
        };
        let Some(idx) = self.selected_index else {
            return;
        };
        if !modules.is_empty() && idx > 0 {
            self.selected_index = Some(idx - 1);
            self.ensure_selected_visible();
            self.rebuild_module_params(true);
        }
    }

    pub(super) fn select_next_module(&mut self) {
        let Some(modules) = self.modules.as_ref() else {
            return;
        };
        let Some(idx) = self.selected_index else {
            return;
        };
        if idx + 1 < modules.len() {
            self.selected_index = Some(idx + 1);
            self.ensure_selected_visible();
            self.rebuild_module_params(true);
        }
    }

    pub(super) fn move_module_up(&mut self) {
        self.move_module(-1);
    }

    pub(super) fn move_module_down(&mut self) {
        self.move_module(1);
    }

    pub(super) fn move_module(&mut self, delta: isize) {
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
                self.rebuild_module_params(true);
                let direction = if delta < 0 { "up" } else { "down" };
                self.push_action_message(format!("Moved {name} {direction}"));
            }
            Err(err) => self.push_action_message(format!("Failed to reorder modules: {err}")),
        }
    }

    pub(super) fn begin_add_module(&mut self) {
        let Some(modules_dir) = self.modules_dir.clone() else {
            self.push_action_message("cannot add module: modules directory unknown".to_string());
            return;
        };
        let available = match crate::config::discover_module_kinds(&modules_dir) {
            Ok(list) => list,
            Err(err) => {
                self.push_action_message(format!("cannot list available modules: {err}"));
                Vec::new()
            }
        };
        self.mode = Mode::AddingModule {
            available,
            selected: 0,
            scroll_offset: 0,
        };
    }

    pub(super) fn handle_key_adding_module(&mut self, key: KeyEvent) {
        let Mode::AddingModule {
            available,
            selected,
            scroll_offset,
        } = &mut self.mode
        else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Up => {
                *selected = selected.saturating_sub(1);
                *scroll_offset =
                    clamped_scroll_offset(*scroll_offset, *selected, self.modules_viewport_height);
            }
            KeyCode::Down => {
                if !available.is_empty() {
                    *selected = (*selected + 1).min(available.len() - 1);
                }
                *scroll_offset =
                    clamped_scroll_offset(*scroll_offset, *selected, self.modules_viewport_height);
            }
            KeyCode::Enter => self.begin_naming_module_instance(),
            _ => {}
        }
    }

    pub(super) fn begin_naming_module_instance(&mut self) {
        let Mode::AddingModule {
            available,
            selected,
            ..
        } = &self.mode
        else {
            return;
        };
        let Some(kind) = available.get(*selected).cloned() else {
            return;
        };
        self.mode = Mode::NamingModuleInstance {
            kind,
            buffer: String::new(),
            cursor: 0,
        };
    }

    pub(super) fn handle_key_naming_module_instance(&mut self, key: KeyEvent) {
        let Mode::NamingModuleInstance { buffer, cursor, .. } = &mut self.mode else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Left => {
                *cursor = cursor.saturating_sub(1);
                return;
            }
            KeyCode::Right => {
                *cursor = (*cursor + 1).min(buffer.chars().count());
                return;
            }
            KeyCode::Backspace => {
                if *cursor > 0 {
                    let byte = super::super::util::char_byte_offset(buffer, *cursor - 1);
                    buffer.remove(byte);
                    *cursor -= 1;
                }
                return;
            }
            KeyCode::Char(c) => {
                let byte = super::super::util::char_byte_offset(buffer, *cursor);
                buffer.insert(byte, c);
                *cursor += 1;
                return;
            }
            KeyCode::Enter => {}
            _ => return,
        }
        self.commit_add_module();
    }

    pub(super) fn commit_add_module(&mut self) {
        let Mode::NamingModuleInstance { kind, buffer, .. } = std::mem::take(&mut self.mode) else {
            return;
        };
        let new_entry = if buffer.is_empty() {
            kind
        } else {
            format!("{kind}#{buffer}")
        };
        let (Some(modules), Some(path)) = (self.modules.clone(), self.config_path.clone()) else {
            self.push_action_message("cannot add module: config state unknown".to_string());
            return;
        };
        match crate::config::BarConfig::write_module_add(&path, &modules, &new_entry) {
            Ok(()) => {
                let mut new_modules = modules;
                new_modules.push(new_entry.clone());
                self.selected_index = Some(new_modules.len() - 1);
                self.modules = Some(new_modules);
                self.ensure_selected_visible();
                self.rebuild_module_params(true);
                self.push_action_message(format!("Added {new_entry}"));
            }
            Err(err) => self.push_action_message(format!("Failed to add module: {err}")),
        }
    }

    pub(super) fn begin_remove_module(&mut self) {
        let (Some(modules), Some(idx)) = (self.modules.as_ref(), self.selected_index) else {
            self.push_action_message("no module selected to remove".to_string());
            return;
        };
        self.mode = Mode::ConfirmingRemove {
            index: idx,
            name: modules[idx].clone(),
        };
    }

    pub(super) fn handle_key_confirming_remove(&mut self, key: KeyEvent) {
        let Mode::ConfirmingRemove { index, .. } = &self.mode else {
            return;
        };
        let index = *index;
        match key.code {
            KeyCode::Char('d') => self.commit_remove_module(index),
            _ => self.mode = Mode::Normal,
        }
    }

    pub(super) fn commit_remove_module(&mut self, index: usize) {
        self.mode = Mode::Normal;
        let Some(modules) = self.modules.clone() else {
            return;
        };
        let Some(path) = self.config_path.clone() else {
            self.push_action_message("cannot remove module: config path unknown".to_string());
            return;
        };
        let name = modules[index].clone();
        match crate::config::BarConfig::write_module_remove(&path, &modules, index) {
            Ok(()) => {
                let mut new_modules = modules;
                new_modules.remove(index);
                let new_len = new_modules.len();
                self.selected_index = if new_len == 0 {
                    None
                } else {
                    Some(index.min(new_len - 1))
                };
                self.modules = Some(new_modules);
                self.ensure_selected_visible();
                if self.selected_index.is_none() {
                    self.panel_focus = PanelFocus::Modules;
                }
                self.rebuild_module_params(true);
                self.push_action_message(format!("Removed {name}"));
            }
            Err(err) => self.push_action_message(format!("Failed to remove module: {err}")),
        }
    }
}
