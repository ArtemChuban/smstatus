use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::clamped_scroll_offset;
use super::{App, LOGS_CHUNK_LINES, PanelFocus};
use crate::logging::LogLevelVisibility;

fn log_path() -> Option<std::path::PathBuf> {
    crate::logging::current_log_path()
}

impl App {
    fn log_level_visibility(&self) -> LogLevelVisibility {
        LogLevelVisibility {
            error: self.logs_show_error,
            warn: self.logs_show_warn,
            info: self.logs_show_info,
        }
    }

    pub(in crate::tui) fn logs_filter_active(&self) -> bool {
        !self.log_level_visibility().all_enabled()
    }

    fn clear_log_state(&mut self) {
        self.log_history.clear();
        self.logs_loaded_from = 0;
        self.logs_total = 0;
        self.logs_file_total = 0;
    }

    fn refresh_log_counts(&mut self, path: &std::path::Path) {
        let visibility = self.log_level_visibility();
        let (visible, file_total) = crate::logging::count_visible_and_file_lines(path, visibility);
        self.logs_total = visible;
        self.logs_file_total = file_total;
    }

    fn load_log_window(&mut self, from: usize, count: usize) {
        let Some(path) = log_path() else {
            self.clear_log_state();
            return;
        };
        let visibility = self.log_level_visibility();
        self.refresh_log_counts(&path);
        let from = from.min(self.logs_total);
        let count = count.min(self.logs_total.saturating_sub(from));
        self.logs_loaded_from = from;
        self.log_history = crate::logging::lines_in_range_filtered(&path, from, count, visibility);
    }

    fn reload_follow_window(&mut self) {
        let viewport = self.logs_viewport_height.max(1);
        let chunk = LOGS_CHUNK_LINES.max(viewport.saturating_mul(2));
        let Some(path) = log_path() else {
            self.clear_log_state();
            return;
        };
        let visibility = self.log_level_visibility();
        self.refresh_log_counts(&path);
        let count = chunk.min(self.logs_total);
        let from = self.logs_total.saturating_sub(count);
        self.logs_loaded_from = from;
        self.log_history = crate::logging::lines_in_range_filtered(&path, from, count, visibility);
    }

    /// Ensure absolute indices in `[abs_start, abs_end)` are present in `log_history`.
    fn ensure_logs_cover(&mut self, abs_start: usize, abs_end: usize) {
        if self.logs_total == 0 {
            return;
        }
        let abs_start = abs_start.min(self.logs_total);
        let abs_end = abs_end.min(self.logs_total).max(abs_start);
        if abs_start >= abs_end {
            return;
        }

        let loaded_end = self.logs_loaded_from + self.log_history.len();
        if abs_start >= self.logs_loaded_from && abs_end <= loaded_end {
            return;
        }

        let Some(path) = log_path() else {
            return;
        };
        let visibility = self.log_level_visibility();

        if abs_start < self.logs_loaded_from {
            let need = self.logs_loaded_from - abs_start;
            let chunk = need.max(LOGS_CHUNK_LINES);
            let new_from = self.logs_loaded_from.saturating_sub(chunk);
            let older = crate::logging::lines_in_range_filtered(
                &path,
                new_from,
                self.logs_loaded_from - new_from,
                visibility,
            );
            self.log_history.splice(0..0, older);
            self.logs_loaded_from = new_from;
        }

        let loaded_end = self.logs_loaded_from + self.log_history.len();
        if abs_end > loaded_end {
            let newer = crate::logging::lines_in_range_filtered(
                &path,
                loaded_end,
                abs_end - loaded_end,
                visibility,
            );
            self.log_history.extend(newer);
        }
    }

    fn restore_selection_to_line(&mut self, line: &str, fallback: usize) {
        if let Some(rel) = self.log_history.iter().position(|l| l == line) {
            self.logs_selected_index = Some(self.logs_loaded_from + rel);
            return;
        }
        let Some(path) = log_path() else {
            self.logs_selected_index = Some(fallback.min(self.logs_total.saturating_sub(1)));
            return;
        };
        let all = crate::logging::lines_in_range_filtered(
            &path,
            0,
            self.logs_total,
            self.log_level_visibility(),
        );
        if let Some(abs) = all.iter().position(|l| l == line) {
            self.logs_selected_index = Some(abs);
            let viewport = self.logs_viewport_height.max(1);
            let chunk = LOGS_CHUNK_LINES.max(viewport.saturating_mul(2));
            let from = abs.saturating_sub(chunk / 2);
            self.load_log_window(from, chunk);
        } else {
            self.logs_selected_index = Some(fallback.min(self.logs_total.saturating_sub(1)));
        }
    }

    pub(in crate::tui) fn refresh_log_history(&mut self) {
        let previous_line = self
            .logs_selected_index
            .and_then(|abs| abs.checked_sub(self.logs_loaded_from))
            .and_then(|rel| self.log_history.get(rel).cloned());
        let previous_selected = self.logs_selected_index;

        if self.logs_follow {
            self.reload_follow_window();
            return;
        }

        let Some(path) = log_path() else {
            self.clear_log_state();
            return;
        };

        self.refresh_log_counts(&path);
        let new_total = self.logs_total;
        if new_total == 0 {
            let file_total = self.logs_file_total;
            self.clear_log_state();
            self.logs_file_total = file_total;
            self.logs_selected_index = None;
            self.logs_scroll_offset = 0;
            return;
        }

        let viewport = self.logs_viewport_height.max(1);
        let chunk = LOGS_CHUNK_LINES.max(viewport.saturating_mul(2));
        let anchor = previous_selected
            .unwrap_or(self.logs_scroll_offset)
            .min(new_total.saturating_sub(1));
        let from = self
            .logs_scroll_offset
            .saturating_sub(chunk / 2)
            .min(anchor.saturating_sub(chunk / 2))
            .min(new_total.saturating_sub(1));
        self.load_log_window(from, chunk);

        if let Some(line) = previous_line.as_ref() {
            let still_same = previous_selected
                .and_then(|abs| abs.checked_sub(self.logs_loaded_from))
                .and_then(|rel| self.log_history.get(rel))
                .is_some_and(|current| current == line);
            if !still_same {
                self.restore_selection_to_line(line, anchor);
            }
        } else if let Some(idx) = self.logs_selected_index
            && idx >= self.logs_total
        {
            self.logs_selected_index = self.logs_total.checked_sub(1);
        }

        self.logs_scroll_offset = self
            .logs_scroll_offset
            .min(self.logs_total.saturating_sub(viewport));
        self.ensure_selected_log_visible();
    }

    fn apply_log_level_filter_change(&mut self) {
        let follow = self.logs_follow;
        self.refresh_log_history();
        if follow {
            self.sync_logs_follow();
        }
    }

    pub(super) fn focus_logs(&mut self) {
        self.panel_focus = PanelFocus::Logs;
        self.logs_follow = true;
        self.refresh_log_history();
        self.sync_logs_follow();
    }

    pub(super) fn leave_logs_focus(&mut self, to: PanelFocus) {
        self.panel_focus = to;
    }

    pub(in crate::tui) fn sync_logs_follow(&mut self) {
        if !self.logs_follow {
            return;
        }
        if self.logs_total == 0 {
            self.logs_selected_index = None;
            self.logs_scroll_offset = 0;
            return;
        }
        self.logs_selected_index = Some(self.logs_total - 1);
        self.ensure_selected_log_visible();
    }

    pub(super) fn ensure_selected_log_visible(&mut self) {
        let Some(idx) = self.logs_selected_index else {
            return;
        };
        let viewport = self.logs_viewport_height.max(1);
        self.logs_scroll_offset = clamped_scroll_offset(self.logs_scroll_offset, idx, viewport);
        let vis_end = self
            .logs_scroll_offset
            .saturating_add(viewport)
            .min(self.logs_total);
        self.ensure_logs_cover(self.logs_scroll_offset, vis_end);
    }

    pub(super) fn select_previous_log(&mut self) {
        if self.logs_total == 0 {
            return;
        }
        if self.logs_follow || self.logs_selected_index.is_none() {
            self.logs_selected_index = Some(self.logs_total - 1);
        }
        let Some(idx) = self.logs_selected_index else {
            return;
        };
        if idx == 0 {
            return;
        }
        let next = idx - 1;
        self.ensure_logs_cover(next, idx);
        self.logs_selected_index = Some(next);
        self.logs_follow = false;
        self.ensure_selected_log_visible();
    }

    pub(super) fn select_next_log(&mut self) {
        if self.logs_total == 0 {
            return;
        }
        let idx = self.logs_selected_index.unwrap_or(self.logs_total - 1);
        if idx + 1 >= self.logs_total {
            self.logs_selected_index = Some(self.logs_total - 1);
            self.logs_follow = true;
            self.ensure_selected_log_visible();
            return;
        }
        let next = idx + 1;
        self.ensure_logs_cover(idx, next + 1);
        self.logs_selected_index = Some(next);
        self.logs_follow = next + 1 == self.logs_total;
        self.ensure_selected_log_visible();
    }

    pub(super) fn handle_key_normal_logs(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.select_previous_log(),
            KeyCode::Down => self.select_next_log(),
            KeyCode::Esc | KeyCode::Left => self.leave_logs_focus(PanelFocus::Modules),
            KeyCode::Tab => self.leave_logs_focus(PanelFocus::Modules),
            KeyCode::Char('e') => {
                self.logs_show_error = !self.logs_show_error;
                self.apply_log_level_filter_change();
            }
            KeyCode::Char('w') => {
                self.logs_show_warn = !self.logs_show_warn;
                self.apply_log_level_filter_change();
            }
            KeyCode::Char('i') => {
                self.logs_show_info = !self.logs_show_info;
                self.apply_log_level_filter_change();
            }
            _ => {}
        }
    }
}
