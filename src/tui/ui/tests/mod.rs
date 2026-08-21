use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::*;
use crate::config::{ModuleParamValue, ParamWriteExpect};
use crate::daemon::DaemonStatus;
use crate::tui::app::{App, Mode, ModuleParamsState, ModuleParamsStatus, PanelFocus};

pub(super) fn pad(content: &str, width: usize) -> String {
    let chars: Vec<char> = content.chars().collect();
    if chars.len() > width {
        chars[..width].iter().collect()
    } else {
        format!("{content:<width$}")
    }
}

pub(super) fn wrap(content: &str) -> String {
    format!("│{content}│")
}

pub(super) fn top_border(title: &str, width: usize) -> String {
    let available = width.saturating_sub(2);
    let title_chars: Vec<char> = title.chars().collect();
    let truncated: String = if title_chars.len() > available {
        title_chars[..available].iter().collect()
    } else {
        title.to_string()
    };
    let dashes = available.saturating_sub(truncated.chars().count());
    format!("╭{truncated}{}╮", "─".repeat(dashes))
}

pub(super) fn bottom_border(width: usize) -> String {
    format!("╰{}╯", "─".repeat(width.saturating_sub(2)))
}

pub(super) fn outer_content_row(content: &str, width: usize) -> String {
    wrap(&pad(content, width - 2))
}

pub(super) fn nested_top_row(title: &str, width: usize) -> String {
    wrap(&top_border(&boxed_title(title), width - 2))
}

pub(super) fn nested_bottom_row(width: usize) -> String {
    wrap(&bottom_border(width - 2))
}

pub(super) fn nested_content_row(content: &str, width: usize) -> String {
    wrap(&wrap(&pad(content, width - 4)))
}

pub(super) fn split_widths(frame_width: usize) -> (usize, usize) {
    let inner = frame_width - 2;
    let left = inner / 2;
    let right = inner - left;
    (left, right)
}

pub(super) fn two_col_top_row(left_title: &str, right_title: &str, width: usize) -> String {
    let (lw, rw) = split_widths(width);
    wrap(&(top_border(&boxed_title(left_title), lw) + &top_border(&boxed_title(right_title), rw)))
}

pub(super) fn two_col_bottom_row(width: usize) -> String {
    let (lw, rw) = split_widths(width);
    wrap(&(bottom_border(lw) + &bottom_border(rw)))
}

pub(super) fn two_col_content_row(left: &str, right: &str, width: usize) -> String {
    let (lw, rw) = split_widths(width);
    let left_inner = wrap(&pad(left, lw.saturating_sub(2)));
    let right_inner = wrap(&pad(right, rw.saturating_sub(2)));
    wrap(&(left_inner + &right_inner))
}

pub(super) fn outer_title_str(status: Option<DaemonStatus>) -> String {
    outer_title(&App {
        daemon_status: status,
        ..App::default()
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn expected(
    width: u16,
    height: u16,
    status: Option<DaemonStatus>,
    separator_text: &str,
    modules_title_text: &str,
    module_lines: &[&str],
    params_title_text: &str,
    params_lines: &[&str],
    action_log: &[&str],
    hint_text: &str,
) -> Buffer {
    let w = width as usize;
    let heights = compute_fixed_heights(height.saturating_sub(OUTER_BORDER_ROWS));
    let mut rows = vec![top_border(&outer_title_str(status), w)];

    if heights.settings > 0 {
        rows.push(nested_top_row("settings", w));
        rows.push(nested_content_row(separator_text, w));
        rows.push(nested_bottom_row(w));
    }

    let modules_height = heights.modules_border + heights.modules_content;
    if modules_height > 0 {
        rows.push(two_col_top_row(modules_title_text, params_title_text, w));
        let vh = heights.modules_content as usize;
        let mut left: Vec<String> = module_lines.iter().map(|s| s.to_string()).collect();
        let mut right: Vec<String> = params_lines.iter().map(|s| s.to_string()).collect();
        while left.len() < vh {
            left.push(String::new());
        }
        while right.len() < vh {
            right.push(String::new());
        }
        for i in 0..vh {
            rows.push(two_col_content_row(&left[i], &right[i], w));
        }
        rows.push(two_col_bottom_row(w));
    }

    if heights.logs > 0 {
        rows.push(nested_top_row("logs", w));
        let mut log_lines: Vec<String> = action_log.iter().map(|s| s.to_string()).collect();
        while log_lines.len() < ACTION_LOG_CAPACITY {
            log_lines.push(String::new());
        }
        for line in &log_lines {
            rows.push(nested_content_row(line, w));
        }
        rows.push(nested_bottom_row(w));
    }

    if heights.hint > 0 {
        rows.push(outer_content_row(hint_text, w));
    }

    rows.push(bottom_border(w));
    Buffer::with_lines(rows)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn expected_overlay(
    width: u16,
    height: u16,
    status: Option<DaemonStatus>,
    separator_text: &str,
    overlay_title_text: &str,
    overlay_lines: &[&str],
    action_log: &[&str],
    hint_text: &str,
) -> Buffer {
    use ratatui::widgets::Widget;

    let mut buf = expected(
        width,
        height,
        status,
        separator_text,
        "modules unknown",
        &[],
        "config",
        &[],
        action_log,
        hint_text,
    );

    let outer_inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
    let heights = compute_fixed_heights(outer_inner.height);
    let areas = layout_areas(outer_inner, &heights);
    if areas.modules.height == 0 {
        return buf;
    }
    let area = overlay_rect(modules_region(&areas));
    Clear.render(area, &mut buf);
    let block = Block::default()
        .title(boxed_title(overlay_title_text))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    block.render(area, &mut buf);
    let lines: Vec<Line> = overlay_lines
        .iter()
        .map(|s| Line::from((*s).to_string()))
        .collect();
    Paragraph::new(lines).render(inner, &mut buf);
    buf
}

pub(super) fn overlay_area_for_frame(width: u16, height: u16) -> Rect {
    let outer_inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
    let heights = compute_fixed_heights(outer_inner.height);
    let areas = layout_areas(outer_inner, &heights);
    overlay_rect(modules_region(&areas))
}

pub(super) fn param_text_overlay_area_for_frame(width: u16, height: u16) -> Rect {
    let outer_inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
    let heights = compute_fixed_heights(outer_inner.height);
    let areas = layout_areas(outer_inner, &heights);
    param_text_overlay_rect(modules_region(&areas))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn expected_param_text_overlay(
    width: u16,
    height: u16,
    status: Option<DaemonStatus>,
    separator_text: &str,
    overlay_title_text: &str,
    overlay_line: &str,
    action_log: &[&str],
    hint_text: &str,
) -> Buffer {
    use ratatui::widgets::Widget;

    let mut buf = expected(
        width,
        height,
        status,
        separator_text,
        "modules unknown",
        &[],
        "config",
        &[],
        action_log,
        hint_text,
    );

    let outer_inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
    let heights = compute_fixed_heights(outer_inner.height);
    let areas = layout_areas(outer_inner, &heights);
    if areas.modules.height == 0 {
        return buf;
    }
    let area = param_text_overlay_rect(modules_region(&areas));
    Clear.render(area, &mut buf);
    let block = Block::default()
        .title(boxed_title(overlay_title_text))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    block.render(area, &mut buf);
    Paragraph::new(Line::from(overlay_line.to_string())).render(inner, &mut buf);
    buf
}

const NORMAL_HINT_MODULES: &str =
    "Select: \u{2191}/\u{2193} | Params: Enter/\u{2192} | Quit: q | Start: s | Kill: k | Help: ?";
const NORMAL_HINT_PARAMS: &str = "Select: \u{2191}/\u{2193} | Edit: e/Enter | Add: a | Del: d | Rename: r | Back: Esc/\u{2190} | Quit: q | Start: s | Kill: k | Help: ?";
const HELP_HINT: &str = "Close: ?/Esc | Scroll: \u{2191}/\u{2193} | Quit: q | Start: s | Kill: k";

const BASELINE_HEIGHT: u16 = 13;

pub(super) fn render_terminal(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| draw(frame, app)).unwrap();
    terminal
}

pub(super) fn render(app: &App, width: u16, height: u16) -> Buffer {
    render_terminal(app, width, height)
        .backend()
        .buffer()
        .clone()
}

pub(super) fn with_reversed_modules_row(mut buffer: Buffer, y: u16, width: u16) -> Buffer {
    let (lw, _) = split_widths(width as usize);
    let content_w = (lw as u16).saturating_sub(2);
    buffer.set_style(
        Rect::new(2, y, content_w, 1),
        Style::default().add_modifier(Modifier::REVERSED),
    );
    buffer
}

pub(super) fn with_reversed_params_row(mut buffer: Buffer, y: u16, width: u16) -> Buffer {
    let (lw, rw) = split_widths(width as usize);
    let x = 1 + lw as u16 + 1;
    let content_w = (rw as u16).saturating_sub(2);
    buffer.set_style(
        Rect::new(x, y, content_w, 1),
        Style::default().add_modifier(Modifier::REVERSED),
    );
    buffer
}

pub(super) fn with_reversed_overlay_row(
    mut buffer: Buffer,
    y: u16,
    frame_width: u16,
    frame_height: u16,
) -> Buffer {
    let area = overlay_area_for_frame(frame_width, frame_height);
    let inner = Block::default().borders(Borders::ALL).inner(area);
    buffer.set_style(
        Rect::new(inner.x, y, inner.width, 1),
        Style::default().add_modifier(Modifier::REVERSED),
    );
    buffer
}

pub(super) fn params_missing(section: &str) -> ModuleParamsState {
    ModuleParamsState {
        status: ModuleParamsStatus::Missing {
            section: section.to_string(),
        },
        entries: vec![],
        selected_index: None,
        scroll_offset: 0,
    }
}

pub(super) fn params_empty() -> ModuleParamsState {
    ModuleParamsState {
        status: ModuleParamsStatus::Empty,
        entries: vec![],
        selected_index: None,
        scroll_offset: 0,
    }
}

pub(super) fn params_entries(entries: Vec<(String, ModuleParamValue)>) -> ModuleParamsState {
    let selected_index = if entries.is_empty() { None } else { Some(0) };
    ModuleParamsState {
        status: ModuleParamsStatus::Entries,
        entries,
        selected_index,
        scroll_offset: 0,
    }
}

mod cursor;
mod daemon;
mod draw;
mod hints;
mod layout;
mod selection;
