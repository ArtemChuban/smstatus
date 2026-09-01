use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::apply_text_edit;
use super::{App, Mode};

impl App {
    pub(super) fn handle_key_editing_separator(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.cancel_edit_separator(),
            KeyCode::Enter => self.commit_edit_separator(),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Char(_) => {
                if let Mode::EditingSeparator { buffer, cursor } = &mut self.mode {
                    apply_text_edit(buffer, cursor, key);
                }
            }
            _ => {}
        }
    }

    pub(super) fn begin_edit_separator(&mut self) {
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

    pub(super) fn cancel_edit_separator(&mut self) {
        self.mode = Mode::Normal;
    }

    pub(super) fn commit_edit_separator(&mut self) {
        let Mode::EditingSeparator { buffer: value, .. } = std::mem::take(&mut self.mode) else {
            return;
        };
        self.ensure_preset_pointer_current();
        let Some(path) = self.config_path.clone() else {
            self.push_action_message("cannot save separator: config path unknown".to_string());
            return;
        };
        match crate::config::BarConfig::write_separator(&path, &value) {
            Ok(()) => {
                self.separator = Some(value);
                self.notify_daemon_config_reload();
                self.push_action_message("Separator updated".to_string());
            }
            Err(err) => self.push_action_message(format!("Failed to update separator: {err}")),
        }
    }
}
