use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Mode};

impl App {
    pub(super) fn handle_key_editing_separator(&mut self, key: KeyEvent) {
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
                    let byte_idx = super::super::util::char_byte_offset(buffer, *cursor - 1);
                    buffer.remove(byte_idx);
                    *cursor -= 1;
                }
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                if let Mode::EditingSeparator { buffer, cursor } = &mut self.mode {
                    let byte_idx = super::super::util::char_byte_offset(buffer, *cursor);
                    buffer.insert(byte_idx, c);
                    *cursor += 1;
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
}
