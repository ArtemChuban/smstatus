use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::is_quit;
use super::{App, Mode};

impl App {
    pub(super) fn handle_key_help(&mut self, key: KeyEvent) {
        if is_quit(key) {
            self.should_quit = true;
            return;
        }
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.help_scroll_offset = 0;
            }
            KeyCode::Char('s') => self.start_daemon(),
            KeyCode::Char('k') => self.stop_daemon(),
            KeyCode::Up => {
                self.help_scroll_offset = self.help_scroll_offset.saturating_sub(1);
            }
            KeyCode::Down => {
                let total = super::super::ui::help_lines(self).len();
                let max_offset = total.saturating_sub(self.modules_viewport_height);
                self.help_scroll_offset =
                    (self.help_scroll_offset.saturating_add(1)).min(max_offset);
            }
            _ => {}
        }
    }
}
