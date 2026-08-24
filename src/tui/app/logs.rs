use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::clamped_scroll_offset;
use super::{App, LOGS_HISTORY_LINES, PanelFocus};

pub(in crate::tui) fn log_history_lines() -> Vec<String> {
    crate::logging::current_log_path()
        .map(|path| crate::logging::tail_lines(&path, LOGS_HISTORY_LINES))
        .unwrap_or_default()
}

fn dropped_prefix_len(previous: &[String], next: &[String]) -> Option<usize> {
    if previous.is_empty() {
        return Some(0);
    }
    if next.is_empty() {
        return Some(previous.len());
    }
    // next is previous[k..] or previous[k..] plus a suffix (ring drop + append).
    for k in 0..=previous.len() {
        let suffix = &previous[k..];
        if next.len() >= suffix.len() && next[..suffix.len()] == *suffix {
            return Some(k);
        }
    }
    None
}

impl App {
    pub(in crate::tui) fn refresh_log_history(&mut self) {
        let previous = std::mem::take(&mut self.log_history);
        let previous_selected = self.logs_selected_index;
        let previous_line = previous_selected.and_then(|i| previous.get(i).cloned());
        self.log_history = log_history_lines();

        if self.logs_follow {
            return;
        }

        let Some(old_idx) = previous_selected else {
            return;
        };

        if let Some(dropped) = dropped_prefix_len(&previous, &self.log_history) {
            self.logs_selected_index = Some(old_idx.saturating_sub(dropped));
            self.logs_scroll_offset = self.logs_scroll_offset.saturating_sub(dropped);
        } else if let Some(line) = previous_line.as_ref()
            && let Some(new_idx) = self.log_history.iter().position(|l| l == line)
        {
            let delta = new_idx as isize - old_idx as isize;
            self.logs_selected_index = Some(new_idx);
            self.logs_scroll_offset = (self.logs_scroll_offset as isize + delta).max(0) as usize;
        }

        let len = self.log_history.len();
        if let Some(idx) = self.logs_selected_index
            && idx >= len
        {
            self.logs_selected_index = len.checked_sub(1);
        }
        let viewport = self.logs_viewport_height.max(1);
        self.logs_scroll_offset = self.logs_scroll_offset.min(len.saturating_sub(viewport));
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
        let len = self.log_history.len();
        if len == 0 {
            self.logs_selected_index = None;
            self.logs_scroll_offset = 0;
            return;
        }
        self.logs_selected_index = Some(len - 1);
        self.ensure_selected_log_visible();
    }

    pub(super) fn ensure_selected_log_visible(&mut self) {
        let Some(idx) = self.logs_selected_index else {
            return;
        };
        let viewport = self.logs_viewport_height.max(1);
        self.logs_scroll_offset = clamped_scroll_offset(self.logs_scroll_offset, idx, viewport);
    }

    pub(super) fn select_previous_log(&mut self) {
        let len = self.log_history.len();
        if len == 0 {
            return;
        }
        if self.logs_follow || self.logs_selected_index.is_none() {
            self.logs_selected_index = Some(len - 1);
        }
        let Some(idx) = self.logs_selected_index else {
            return;
        };
        if idx == 0 {
            return;
        }
        self.logs_selected_index = Some(idx - 1);
        self.logs_follow = false;
        self.ensure_selected_log_visible();
    }

    pub(super) fn select_next_log(&mut self) {
        let len = self.log_history.len();
        if len == 0 {
            return;
        }
        let idx = self.logs_selected_index.unwrap_or(len - 1);
        if idx + 1 >= len {
            self.logs_selected_index = Some(len - 1);
            self.logs_follow = true;
            self.ensure_selected_log_visible();
            return;
        }
        let next = idx + 1;
        self.logs_selected_index = Some(next);
        self.logs_follow = next + 1 == len;
        self.ensure_selected_log_visible();
    }

    pub(super) fn handle_key_normal_logs(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.select_previous_log(),
            KeyCode::Down => self.select_next_log(),
            KeyCode::Esc | KeyCode::Left => self.leave_logs_focus(PanelFocus::Modules),
            KeyCode::Tab => self.leave_logs_focus(PanelFocus::Modules),
            _ => {}
        }
    }
}
