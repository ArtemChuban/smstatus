use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::clamped_scroll_offset;
use super::{App, Mode};

impl App {
    pub(super) fn refresh_installed_extensions(&mut self) {
        let Some(extensions_dir) = self.extensions_dir.as_ref() else {
            self.installed_extensions.clear();
            self.refresh_extension_display_cache();
            return;
        };
        match crate::install::list_extensions_in(extensions_dir) {
            Ok(names) => self.installed_extensions = names,
            Err(err) => {
                self.push_action_message(format!("Failed to list extensions: {err}"));
                self.installed_extensions.clear();
            }
        }
        self.refresh_extension_display_cache();
    }

    pub(super) fn begin_browse_extensions(&mut self) {
        self.refresh_installed_extensions();
        self.mode = Mode::BrowsingExtensions {
            selected: 0,
            scroll_offset: 0,
        };
    }

    pub(super) fn handle_key_browsing_extensions(&mut self, key: KeyEvent) {
        let Mode::BrowsingExtensions {
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
                    clamped_scroll_offset(*scroll_offset, *selected, self.overlay_viewport_height);
            }
            KeyCode::Down => {
                if !self.installed_extensions.is_empty() {
                    *selected = (*selected + 1).min(self.installed_extensions.len() - 1);
                }
                *scroll_offset =
                    clamped_scroll_offset(*scroll_offset, *selected, self.overlay_viewport_height);
            }
            _ => {}
        }
    }

    pub(in crate::tui) fn extension_overlay_labels(&self) -> &[String] {
        &self.extension_overlay_labels
    }
}
