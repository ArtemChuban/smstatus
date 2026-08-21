use std::borrow::Cow;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::config::ModuleParamValue;

use super::app::{
    ACTION_LOG_CAPACITY, App, Mode, ModuleParamsState, ModuleParamsStatus, PanelFocus,
};

const OUTER_BORDER_ROWS: u16 = 2;
const SETTINGS_BLOCK_HEIGHT: u16 = 3;
const MODULES_BORDER_ROWS: u16 = 2;
const HINT_HEIGHT: u16 = 1;
const LOGS_BLOCK_HEIGHT: u16 = 2 + ACTION_LOG_CAPACITY as u16;
const SEPARATOR_EDIT_PREFIX: &str = "New separator: ";
const OVERLAY_MARGIN_X: u16 = 4;
const OVERLAY_MARGIN_Y: u16 = 1;

struct FixedHeights {
    settings: u16,
    modules_border: u16,
    hint: u16,
    logs: u16,
    modules_content: u16,
}

fn take(remaining: &mut u16, want: u16) -> u16 {
    if *remaining >= want {
        *remaining -= want;
        want
    } else {
        0
    }
}

fn compute_fixed_heights(outer_inner_height: u16) -> FixedHeights {
    let mut remaining = outer_inner_height;
    let settings = take(&mut remaining, SETTINGS_BLOCK_HEIGHT);
    let modules_border = take(&mut remaining, MODULES_BORDER_ROWS);
    let hint = take(&mut remaining, HINT_HEIGHT);
    let logs = take(&mut remaining, LOGS_BLOCK_HEIGHT);
    let modules_content = if modules_border > 0 { remaining } else { 0 };
    FixedHeights {
        settings,
        modules_border,
        hint,
        logs,
        modules_content,
    }
}

pub(super) fn modules_viewport_height(frame_height: u16) -> usize {
    compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS)).modules_content as usize
}

struct Areas {
    settings: Rect,
    modules: Rect,
    params: Rect,
    logs: Rect,
    hint: Rect,
}

fn layout_areas(outer_inner: Rect, heights: &FixedHeights) -> Areas {
    let mut y = outer_inner.y;

    let settings = Rect::new(outer_inner.x, y, outer_inner.width, heights.settings);
    y += heights.settings;

    let modules_height = heights.modules_border + heights.modules_content;
    let left_w = outer_inner.width / 2;
    let right_w = outer_inner.width.saturating_sub(left_w);
    let modules = Rect::new(outer_inner.x, y, left_w, modules_height);
    let params = Rect::new(outer_inner.x + left_w, y, right_w, modules_height);
    y += modules_height;

    let logs = Rect::new(outer_inner.x, y, outer_inner.width, heights.logs);
    y += heights.logs;

    let hint = Rect::new(outer_inner.x, y, outer_inner.width, heights.hint);

    Areas {
        settings,
        modules,
        params,
        logs,
        hint,
    }
}

fn modules_region(areas: &Areas) -> Rect {
    Rect::new(
        areas.modules.x,
        areas.modules.y,
        areas.modules.width.saturating_add(areas.params.width),
        areas.modules.height,
    )
}

fn overlay_rect(region: Rect) -> Rect {
    let margin_x = OVERLAY_MARGIN_X.min(region.width.saturating_sub(2) / 2);
    let margin_y = OVERLAY_MARGIN_Y.min(region.height.saturating_sub(2) / 2);
    Rect {
        x: region.x.saturating_add(margin_x),
        y: region.y.saturating_add(margin_y),
        width: region.width.saturating_sub(margin_x.saturating_mul(2)),
        height: region.height.saturating_sub(margin_y.saturating_mul(2)),
    }
}

pub(super) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer_block = Block::default()
        .title(outer_title(app))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let outer_inner = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let heights = compute_fixed_heights(outer_inner.height);
    let areas = layout_areas(outer_inner, &heights);
    let viewport_height = heights.modules_content as usize;

    if areas.settings.height > 0 {
        let settings_block = Block::default()
            .title(boxed_title("settings"))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded);
        let settings_inner = settings_block.inner(areas.settings);
        frame.render_widget(settings_block, areas.settings);
        frame.render_widget(Paragraph::new(separator_line(app)), settings_inner);
        if let Mode::EditingSeparator { buffer, cursor } = &app.mode {
            let col =
                settings_inner.x + text_edit_cursor_column(SEPARATOR_EDIT_PREFIX, buffer, *cursor);
            frame.set_cursor_position((col, settings_inner.y));
        }
    }

    if areas.modules.height > 0 {
        draw_modules_column(frame, app, areas.modules, viewport_height);
        draw_params_column(frame, app, areas.params, viewport_height);
    }

    if areas.logs.height > 0 {
        let logs_block = Block::default()
            .title(boxed_title("logs"))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded);
        let logs_inner = logs_block.inner(areas.logs);
        frame.render_widget(logs_block, areas.logs);
        let log_lines: Vec<Line> = action_log_lines(&app.action_log)
            .into_iter()
            .map(Line::from)
            .collect();
        frame.render_widget(Paragraph::new(log_lines), logs_inner);
    }

    if areas.hint.height > 0 {
        frame.render_widget(Paragraph::new(hint_line(app)), areas.hint);
    }

    if areas.modules.height > 0 {
        let region = modules_region(&areas);
        match &app.mode {
            Mode::Help => draw_help_overlay(frame, app, region),
            Mode::AddingModule {
                available,
                selected,
                scroll_offset,
            } => draw_add_overlay(frame, region, available, *selected, *scroll_offset),
            Mode::NamingModuleInstance {
                kind,
                buffer,
                cursor,
            } => draw_naming_overlay(frame, region, kind, buffer, *cursor),
            Mode::AddingParamKey { buffer, cursor, .. } => {
                draw_param_text_overlay(frame, region, "add param key", buffer, *cursor)
            }
            Mode::EditingParamValue {
                key,
                buffer,
                cursor,
                ..
            } => {
                draw_param_text_overlay(frame, region, &format!("value for {key}"), buffer, *cursor)
            }
            Mode::RenamingParamKey {
                old_key,
                buffer,
                cursor,
                ..
            } => draw_param_text_overlay(
                frame,
                region,
                &format!("rename {old_key}"),
                buffer,
                *cursor,
            ),
            Mode::Normal
            | Mode::EditingSeparator { .. }
            | Mode::ConfirmingRemove { .. }
            | Mode::ConfirmingRemoveParam { .. } => {}
        }
    }
}

fn draw_modules_column(frame: &mut Frame, app: &App, area: Rect, viewport_height: usize) {
    let modules_block = Block::default()
        .title(modules_title(app, viewport_height))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let modules_inner = modules_block.inner(area);
    frame.render_widget(modules_block, area);
    let module_lines = visible_module_lines(app, viewport_height, modules_inner.width);
    frame.render_widget(Paragraph::new(module_lines), modules_inner);
}

fn draw_params_column(frame: &mut Frame, app: &App, area: Rect, viewport_height: usize) {
    let params_block = Block::default()
        .title(params_title(app))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let params_inner = params_block.inner(area);
    frame.render_widget(params_block, area);
    let lines = visible_params_lines(app, viewport_height, params_inner.width);
    frame.render_widget(Paragraph::new(lines), params_inner);
}

fn draw_help_overlay(frame: &mut Frame, app: &App, region: Rect) {
    let area = overlay_rect(region);
    let block = Block::default()
        .title(boxed_title("help"))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let viewport_height = inner.height as usize;
    let lines = styled_list_lines(
        &help_lines(app),
        None,
        app.help_scroll_offset,
        viewport_height,
        inner.width,
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_add_overlay(
    frame: &mut Frame,
    region: Rect,
    available: &[String],
    selected: usize,
    scroll_offset: usize,
) {
    let area = overlay_rect(region);
    let viewport_height = Block::default().borders(Borders::ALL).inner(area).height as usize;
    let (start, end, total) = module_window(available.len(), scroll_offset, viewport_height);
    let title = if viewport_height == 0 || total == 0 {
        format!("add module {total} available")
    } else {
        format!("add module {}-{end}/{total}", start + 1)
    };
    let block = Block::default()
        .title(boxed_title(&title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let lines = styled_list_lines(
        available,
        Some(selected),
        scroll_offset,
        viewport_height,
        inner.width,
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_naming_overlay(frame: &mut Frame, region: Rect, kind: &str, buffer: &str, cursor: usize) {
    let area = overlay_rect(region);
    let block = Block::default()
        .title(boxed_title(&format!("name instance of {kind}")))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let prefix = instance_name_prefix(kind);
    frame.render_widget(Paragraph::new(format!("{prefix}{buffer:?}")), inner);
    let col = inner.x + text_edit_cursor_column(&prefix, buffer, cursor);
    frame.set_cursor_position((col, inner.y));
}

const PARAM_TEXT_OVERLAY_HEIGHT: u16 = 3;

fn param_text_overlay_rect(region: Rect) -> Rect {
    let full = overlay_rect(region);
    let height = PARAM_TEXT_OVERLAY_HEIGHT.min(full.height);
    Rect {
        x: full.x,
        y: full.y + full.height.saturating_sub(height) / 2,
        width: full.width,
        height,
    }
}

fn draw_param_text_overlay(
    frame: &mut Frame,
    region: Rect,
    title: &str,
    buffer: &str,
    cursor: usize,
) {
    let area = param_text_overlay_rect(region);
    let block = Block::default()
        .title(boxed_title(title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(format!("{buffer:?}")), inner);
    let col = inner.x + text_edit_cursor_column("", buffer, cursor);
    frame.set_cursor_position((col, inner.y));
}

fn boxed_title(text: &str) -> String {
    format!("─{text}")
}

fn outer_title(app: &App) -> String {
    boxed_title(&format!(
        "smstatus v{} {}",
        env!("CARGO_PKG_VERSION"),
        daemon_status_phrase(app.daemon_status)
    ))
}

fn daemon_status_phrase(status: Option<crate::daemon::DaemonStatus>) -> String {
    use crate::daemon::DaemonStatus;
    match status {
        Some(DaemonStatus::Running { pid }) => format!("running (pid {pid})"),
        Some(DaemonStatus::RunningPidUnknown) => "running (pid unknown)".to_string(),
        Some(DaemonStatus::Stopped) => "stopped".to_string(),
        None => "status unknown".to_string(),
    }
}

fn separator_line(app: &App) -> String {
    match &app.mode {
        Mode::EditingSeparator { buffer, .. } => format!("{SEPARATOR_EDIT_PREFIX}{buffer:?}"),
        Mode::Normal
        | Mode::AddingModule { .. }
        | Mode::NamingModuleInstance { .. }
        | Mode::ConfirmingRemove { .. }
        | Mode::AddingParamKey { .. }
        | Mode::EditingParamValue { .. }
        | Mode::ConfirmingRemoveParam { .. }
        | Mode::RenamingParamKey { .. }
        | Mode::Help => match &app.separator {
            Some(sep) => format!("separator: {sep:?}"),
            None => "separator: unknown".to_string(),
        },
    }
}

fn text_edit_cursor_column(prefix: &str, buffer: &str, cursor: usize) -> u16 {
    let byte_idx = super::char_byte_offset(buffer, cursor);
    let escaped_prefix_quoted = format!("{:?}", &buffer[..byte_idx]);
    let escaped_prefix = &escaped_prefix_quoted[..escaped_prefix_quoted.len() - 1];
    (prefix.chars().count() + escaped_prefix.chars().count()) as u16
}

fn hint_line(app: &App) -> Cow<'static, str> {
    match &app.mode {
        Mode::Normal => match app.panel_focus {
            PanelFocus::Modules => Cow::Borrowed(
                "Select: \u{2191}/\u{2193} | Params: Enter/\u{2192} | Quit: q | Start: s | Kill: k | Help: ?",
            ),
            PanelFocus::Params => Cow::Borrowed(
                "Select: \u{2191}/\u{2193} | Edit: e/Enter | Add: a | Del: d | Rename: r | Back: Esc/\u{2190} | Quit: q | Start: s | Kill: k | Help: ?",
            ),
        },
        Mode::EditingSeparator { .. } => Cow::Borrowed("Save: Enter | Cancel: Esc"),
        Mode::AddingModule { .. } => {
            Cow::Borrowed("Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc")
        }
        Mode::NamingModuleInstance { .. } => Cow::Borrowed("Confirm: Enter | Cancel: Esc"),
        Mode::ConfirmingRemove { name, .. } => {
            Cow::Owned(format!("Remove {name}? Confirm: d | Cancel: any key"))
        }
        Mode::AddingParamKey { .. }
        | Mode::EditingParamValue { .. }
        | Mode::RenamingParamKey { .. } => Cow::Borrowed("Confirm: Enter | Cancel: Esc"),
        Mode::ConfirmingRemoveParam { key, .. } => {
            Cow::Owned(format!("Remove {key}? Confirm: d | Cancel: any key"))
        }
        Mode::Help => {
            Cow::Borrowed("Close: ?/Esc | Scroll: \u{2191}/\u{2193} | Quit: q | Start: s | Kill: k")
        }
    }
}

fn instance_name_prefix(kind: &str) -> String {
    format!("instance name for {kind}: ")
}

fn module_window(total: usize, offset: usize, viewport_height: usize) -> (usize, usize, usize) {
    if total == 0 {
        return (0, 0, 0);
    }
    let offset = offset.min(total.saturating_sub(viewport_height));
    let end = (offset + viewport_height).min(total);
    (offset, end, total)
}

fn modules_title(app: &App, viewport_height: usize) -> String {
    let text = match &app.modules {
        None => "modules unknown".to_string(),
        Some(modules) if modules.is_empty() => "modules (none configured)".to_string(),
        Some(modules) => {
            let (start, end, total) =
                module_window(modules.len(), app.module_scroll_offset, viewport_height);
            if viewport_height == 0 {
                format!("modules {total} configured")
            } else {
                format!("modules {}-{end}/{total}", start + 1)
            }
        }
    };
    boxed_title(&text)
}

fn params_title(app: &App) -> String {
    let text = match selected_module_entry(app) {
        Some(entry) => format!("config {entry}"),
        None => "config".to_string(),
    };
    boxed_title(&text)
}

fn selected_module_entry(app: &App) -> Option<&str> {
    let idx = app.selected_index?;
    app.modules.as_ref()?.get(idx).map(String::as_str)
}

fn styled_list_lines(
    entries: &[String],
    selected: Option<usize>,
    offset: usize,
    viewport_height: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let (start, end, _total) = module_window(entries.len(), offset, viewport_height);
    entries[start..end]
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if Some(start + i) == selected {
                let display_width = name.width();
                let padding = " ".repeat((width as usize).saturating_sub(display_width));
                Line::styled(
                    format!("{name}{padding}"),
                    Style::default().add_modifier(Modifier::REVERSED),
                )
            } else {
                Line::from(name.clone())
            }
        })
        .collect()
}

fn visible_module_lines(app: &App, viewport_height: usize, width: u16) -> Vec<Line<'static>> {
    let Some(modules) = &app.modules else {
        return Vec::new();
    };
    let selected = if app.panel_focus == PanelFocus::Modules {
        app.selected_index
    } else {
        None
    };
    styled_list_lines(
        modules,
        selected,
        app.module_scroll_offset,
        viewport_height,
        width,
    )
}

fn param_display_lines(state: &ModuleParamsState) -> Vec<String> {
    match &state.status {
        ModuleParamsStatus::Missing { section } => {
            vec![format!("(no [{section}] section)")]
        }
        ModuleParamsStatus::Empty => vec!["(empty)".to_string()],
        ModuleParamsStatus::Entries => state
            .entries
            .iter()
            .map(|(key, value)| match value {
                ModuleParamValue::String(s) => format!("{key} = {s:?}"),
                ModuleParamValue::NonString => format!("{key} = <non-string>"),
            })
            .collect(),
    }
}

fn visible_params_lines(app: &App, viewport_height: usize, width: u16) -> Vec<Line<'static>> {
    let Some(state) = &app.module_params else {
        return Vec::new();
    };
    let lines = param_display_lines(state);
    let selected = if app.panel_focus == PanelFocus::Params
        && matches!(state.status, ModuleParamsStatus::Entries)
    {
        state.selected_index
    } else {
        None
    };
    let offset = if matches!(state.status, ModuleParamsStatus::Entries) {
        state.scroll_offset
    } else {
        0
    };
    styled_list_lines(&lines, selected, offset, viewport_height, width)
}

pub(super) fn help_lines(app: &App) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push("--- Local ---".to_string());
    match &app.mode {
        Mode::Normal => match app.panel_focus {
            PanelFocus::Modules => {
                lines.push("Select module: \u{2191}/\u{2193}".to_string());
                lines.push("Move module: Ctrl+\u{2191}/\u{2193}".to_string());
                lines.push("Add module: a".to_string());
                lines.push("Remove module: d".to_string());
                lines.push("Edit separator: e".to_string());
                lines.push("Focus params: Enter/\u{2192}".to_string());
            }
            PanelFocus::Params => {
                lines.push("Select param: \u{2191}/\u{2193}".to_string());
                lines.push("Edit value: e/Enter".to_string());
                lines.push("Add param: a".to_string());
                lines.push("Remove param: d".to_string());
                lines.push("Rename key: r".to_string());
                lines.push("Back to modules: Esc/\u{2190}".to_string());
            }
        },
        Mode::Help => {
            lines.push("Close help: ? or Esc".to_string());
            lines.push("Scroll help: \u{2191}/\u{2193}".to_string());
        }
        _ => {}
    }
    lines.push("--- Global ---".to_string());
    match &app.mode {
        Mode::Normal | Mode::Help => {
            lines.push("Quit: q".to_string());
            lines.push("Hard quit: Ctrl+c or Ctrl+d".to_string());
            lines.push("Start daemon: s".to_string());
            lines.push("Kill daemon: k".to_string());
            if matches!(app.mode, Mode::Normal) {
                lines.push("Help: ?".to_string());
            }
        }
        _ => {}
    }
    lines
}

fn action_log_lines(action_log: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = action_log.to_vec();
    while lines.len() < ACTION_LOG_CAPACITY {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Position;

    use super::*;
    use crate::config::{ModuleParamValue, ParamWriteExpect};
    use crate::daemon::DaemonStatus;
    use crate::tui::app::{App, ModuleParamsState, ModuleParamsStatus, PanelFocus};

    fn pad(content: &str, width: usize) -> String {
        let chars: Vec<char> = content.chars().collect();
        if chars.len() > width {
            chars[..width].iter().collect()
        } else {
            format!("{content:<width$}")
        }
    }

    fn wrap(content: &str) -> String {
        format!("│{content}│")
    }

    fn top_border(title: &str, width: usize) -> String {
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

    fn bottom_border(width: usize) -> String {
        format!("╰{}╯", "─".repeat(width.saturating_sub(2)))
    }

    fn outer_content_row(content: &str, width: usize) -> String {
        wrap(&pad(content, width - 2))
    }

    fn nested_top_row(title: &str, width: usize) -> String {
        wrap(&top_border(&boxed_title(title), width - 2))
    }

    fn nested_bottom_row(width: usize) -> String {
        wrap(&bottom_border(width - 2))
    }

    fn nested_content_row(content: &str, width: usize) -> String {
        wrap(&wrap(&pad(content, width - 4)))
    }

    fn split_widths(frame_width: usize) -> (usize, usize) {
        let inner = frame_width - 2;
        let left = inner / 2;
        let right = inner - left;
        (left, right)
    }

    fn two_col_top_row(left_title: &str, right_title: &str, width: usize) -> String {
        let (lw, rw) = split_widths(width);
        wrap(
            &(top_border(&boxed_title(left_title), lw)
                + &top_border(&boxed_title(right_title), rw)),
        )
    }

    fn two_col_bottom_row(width: usize) -> String {
        let (lw, rw) = split_widths(width);
        wrap(&(bottom_border(lw) + &bottom_border(rw)))
    }

    fn two_col_content_row(left: &str, right: &str, width: usize) -> String {
        let (lw, rw) = split_widths(width);
        let left_inner = wrap(&pad(left, lw.saturating_sub(2)));
        let right_inner = wrap(&pad(right, rw.saturating_sub(2)));
        wrap(&(left_inner + &right_inner))
    }

    fn outer_title_str(status: Option<DaemonStatus>) -> String {
        outer_title(&App {
            daemon_status: status,
            ..App::default()
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn expected(
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
    fn expected_overlay(
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

    fn overlay_area_for_frame(width: u16, height: u16) -> Rect {
        let outer_inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
        let heights = compute_fixed_heights(outer_inner.height);
        let areas = layout_areas(outer_inner, &heights);
        overlay_rect(modules_region(&areas))
    }

    fn param_text_overlay_area_for_frame(width: u16, height: u16) -> Rect {
        let outer_inner = Rect::new(1, 1, width.saturating_sub(2), height.saturating_sub(2));
        let heights = compute_fixed_heights(outer_inner.height);
        let areas = layout_areas(outer_inner, &heights);
        param_text_overlay_rect(modules_region(&areas))
    }

    #[allow(clippy::too_many_arguments)]
    fn expected_param_text_overlay(
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

    const NORMAL_HINT_MODULES: &str = "Select: \u{2191}/\u{2193} | Params: Enter/\u{2192} | Quit: q | Start: s | Kill: k | Help: ?";
    const NORMAL_HINT_PARAMS: &str = "Select: \u{2191}/\u{2193} | Edit: e/Enter | Add: a | Del: d | Rename: r | Back: Esc/\u{2190} | Quit: q | Start: s | Kill: k | Help: ?";
    const HELP_HINT: &str =
        "Close: ?/Esc | Scroll: \u{2191}/\u{2193} | Quit: q | Start: s | Kill: k";

    const BASELINE_HEIGHT: u16 = 13;

    fn render_terminal(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
    }

    fn render(app: &App, width: u16, height: u16) -> Buffer {
        render_terminal(app, width, height)
            .backend()
            .buffer()
            .clone()
    }

    fn with_reversed_modules_row(mut buffer: Buffer, y: u16, width: u16) -> Buffer {
        let (lw, _) = split_widths(width as usize);
        let content_w = (lw as u16).saturating_sub(2);
        buffer.set_style(
            Rect::new(2, y, content_w, 1),
            Style::default().add_modifier(Modifier::REVERSED),
        );
        buffer
    }

    fn with_reversed_params_row(mut buffer: Buffer, y: u16, width: u16) -> Buffer {
        let (lw, rw) = split_widths(width as usize);
        let x = 1 + lw as u16 + 1;
        let content_w = (rw as u16).saturating_sub(2);
        buffer.set_style(
            Rect::new(x, y, content_w, 1),
            Style::default().add_modifier(Modifier::REVERSED),
        );
        buffer
    }

    fn with_reversed_overlay_row(
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

    #[test]
    fn daemon_status_phrase_running() {
        assert_eq!(
            daemon_status_phrase(Some(DaemonStatus::Running { pid: 42 })),
            "running (pid 42)"
        );
    }

    #[test]
    fn daemon_status_phrase_running_pid_unknown() {
        assert_eq!(
            daemon_status_phrase(Some(DaemonStatus::RunningPidUnknown)),
            "running (pid unknown)"
        );
    }

    #[test]
    fn daemon_status_phrase_stopped() {
        assert_eq!(daemon_status_phrase(Some(DaemonStatus::Stopped)), "stopped");
    }

    #[test]
    fn daemon_status_phrase_unknown() {
        assert_eq!(daemon_status_phrase(None), "status unknown");
    }

    #[test]
    fn draw_renders_stopped_status_in_outer_title() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_running_status_in_outer_title() {
        let app = App {
            daemon_status: Some(DaemonStatus::Running { pid: 12345 }),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Running { pid: 12345 }),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_running_pid_unknown_status_in_outer_title() {
        let app = App {
            daemon_status: Some(DaemonStatus::RunningPidUnknown),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::RunningPidUnknown),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_unknown_status_in_outer_title() {
        let app = App {
            daemon_status: None,
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                None,
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_separator_value() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            separator: Some(" | ".to_string()),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: \" | \"",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_empty_separator_as_quoted_empty_string() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            separator: Some(String::new()),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: \"\"",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_editing_prompt_with_buffer_contents() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator {
                buffer: "::".to_string(),
                cursor: 2,
            },
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "New separator: \"::\"",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                "Save: Enter | Cancel: Esc",
            )
        );
    }

    #[test]
    fn draw_renders_editing_prompt_with_empty_buffer() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator {
                buffer: String::new(),
                cursor: 0,
            },
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "New separator: \"\"",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                "Save: Enter | Cancel: Esc",
            )
        );
    }

    #[test]
    fn hint_line_normal_mode_modules_focus() {
        let app = App::default();
        assert_eq!(hint_line(&app).as_ref(), NORMAL_HINT_MODULES);
    }

    #[test]
    fn hint_line_normal_mode_params_focus() {
        let app = App {
            panel_focus: PanelFocus::Params,
            ..App::default()
        };
        assert_eq!(hint_line(&app).as_ref(), NORMAL_HINT_PARAMS);
    }

    #[test]
    fn hint_line_editing_separator_mode() {
        let app = App {
            mode: Mode::EditingSeparator {
                buffer: String::new(),
                cursor: 0,
            },
            ..App::default()
        };
        assert_eq!(hint_line(&app).as_ref(), "Save: Enter | Cancel: Esc");
    }

    #[test]
    fn hint_line_adding_module_mode() {
        let app = App {
            mode: Mode::AddingModule {
                available: vec![],
                selected: 0,
                scroll_offset: 0,
            },
            ..App::default()
        };
        assert_eq!(
            hint_line(&app).as_ref(),
            "Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc"
        );
    }

    #[test]
    fn hint_line_naming_module_instance_mode() {
        let app = App {
            mode: Mode::NamingModuleInstance {
                kind: "cpu".to_string(),
                buffer: String::new(),
                cursor: 0,
            },
            ..App::default()
        };
        assert_eq!(hint_line(&app).as_ref(), "Confirm: Enter | Cancel: Esc");
    }

    #[test]
    fn hint_line_confirming_remove_mode_includes_module_name() {
        let app = App {
            mode: Mode::ConfirmingRemove {
                index: 0,
                name: "cpu".to_string(),
            },
            ..App::default()
        };
        assert_eq!(
            hint_line(&app).as_ref(),
            "Remove cpu? Confirm: d | Cancel: any key"
        );
    }

    #[test]
    fn hint_line_help_mode() {
        let app = App {
            mode: Mode::Help,
            ..App::default()
        };
        assert_eq!(hint_line(&app).as_ref(), HELP_HINT);
    }

    #[test]
    fn draw_renders_empty_action_log_as_blank_rows() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            action_log: vec![],
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_single_action_message_padded_to_capacity() {
        let app = App {
            daemon_status: Some(DaemonStatus::Running { pid: 42 }),
            action_log: vec!["Starting smstatus...".to_string()],
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Running { pid: 42 }),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &["Starting smstatus..."],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_action_log_at_capacity() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            action_log: vec![
                "Starting smstatus...".to_string(),
                "smstatus is already running".to_string(),
                "Sent stop signal to smstatus (pid 42)".to_string(),
            ],
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[
                    "Starting smstatus...",
                    "smstatus is already running",
                    "Sent stop signal to smstatus (pid 42)",
                ],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_empty_module_list_title_with_zero_rows() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![]),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules (none configured)",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_degraded_title_when_viewport_height_is_zero_with_modules_present() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "cpu".to_string(),
                "disk#root".to_string(),
                "battery".to_string(),
            ]),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 3 configured",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_modules_that_fully_fit_in_the_viewport() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "cpu".to_string(),
                "disk#root".to_string(),
                "battery".to_string(),
            ]),
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 3;
        assert_eq!(
            render(&app, 70, height),
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-3/3",
                &["cpu", "disk#root", "battery"],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_scrolled_slice_of_modules_when_more_than_fit() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "m0".to_string(),
                "m1".to_string(),
                "m2".to_string(),
                "m3".to_string(),
                "m4".to_string(),
                "m5".to_string(),
            ]),
            module_scroll_offset: 2,
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 2;
        assert_eq!(
            render(&app, 70, height),
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 3-4/6",
                &["m2", "m3"],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_renders_instance_suffixed_module_entry_verbatim() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec!["disk#root".to_string()]),
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 1;
        assert_eq!(
            render(&app, 70, height),
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["disk#root"],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    fn params_missing(section: &str) -> ModuleParamsState {
        ModuleParamsState {
            status: ModuleParamsStatus::Missing {
                section: section.to_string(),
            },
            entries: vec![],
            selected_index: None,
            scroll_offset: 0,
        }
    }

    fn params_empty() -> ModuleParamsState {
        ModuleParamsState {
            status: ModuleParamsStatus::Empty,
            entries: vec![],
            selected_index: None,
            scroll_offset: 0,
        }
    }

    fn params_entries(entries: Vec<(String, ModuleParamValue)>) -> ModuleParamsState {
        let selected_index = if entries.is_empty() { None } else { Some(0) };
        ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries,
            selected_index,
            scroll_offset: 0,
        }
    }

    #[test]
    fn draw_renders_missing_params_section_message() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec!["cpu".to_string()]),
            selected_index: Some(0),
            module_params: Some(params_missing("cpu")),
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 1;
        assert_eq!(
            render(&app, 70, height),
            with_reversed_modules_row(
                expected(
                    70,
                    height,
                    Some(DaemonStatus::Stopped),
                    "separator: unknown",
                    "modules 1-1/1",
                    &["cpu"],
                    "config cpu",
                    &["(no [cpu] section)"],
                    &[],
                    NORMAL_HINT_MODULES,
                ),
                5,
                70,
            )
        );
    }

    #[test]
    fn draw_renders_empty_params_section_message() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec!["cpu".to_string()]),
            selected_index: Some(0),
            module_params: Some(params_empty()),
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 1;
        assert_eq!(
            render(&app, 70, height),
            with_reversed_modules_row(
                expected(
                    70,
                    height,
                    Some(DaemonStatus::Stopped),
                    "separator: unknown",
                    "modules 1-1/1",
                    &["cpu"],
                    "config cpu",
                    &["(empty)"],
                    &[],
                    NORMAL_HINT_MODULES,
                ),
                5,
                70,
            )
        );
    }

    #[test]
    fn draw_renders_string_and_non_string_param_entries() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec!["disk#root".to_string()]),
            selected_index: Some(0),
            module_params: Some(params_entries(vec![
                (
                    "path".to_string(),
                    ModuleParamValue::String("/".to_string()),
                ),
                ("interval".to_string(), ModuleParamValue::NonString),
            ])),
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 2;
        assert_eq!(
            render(&app, 70, height),
            with_reversed_modules_row(
                expected(
                    70,
                    height,
                    Some(DaemonStatus::Stopped),
                    "separator: unknown",
                    "modules 1-1/1",
                    &["disk#root"],
                    "config disk#root",
                    &["path = \"/\"", "interval = <non-string>"],
                    &[],
                    NORMAL_HINT_MODULES,
                ),
                5,
                70,
            )
        );
    }

    #[test]
    fn selected_module_row_is_rendered_with_reversed_style() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "cpu".to_string(),
                "disk".to_string(),
                "battery".to_string(),
            ]),
            selected_index: Some(1),
            module_params: Some(params_missing("disk")),
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 3;
        let buffer = render(&app, 70, height);
        let expected_buf = with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-3/3",
                &["cpu", "disk", "battery"],
                "config disk",
                &["(no [disk] section)"],
                &[],
                NORMAL_HINT_MODULES,
            ),
            6,
            70,
        );
        assert_eq!(buffer, expected_buf);
    }

    #[test]
    fn selected_param_row_reversed_only_when_params_focused() {
        let height = BASELINE_HEIGHT + 2;

        let modules_focus = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec!["cpu".to_string()]),
            selected_index: Some(0),
            panel_focus: PanelFocus::Modules,
            module_params: Some(params_entries(vec![
                ("a".to_string(), ModuleParamValue::String("1".to_string())),
                ("b".to_string(), ModuleParamValue::String("2".to_string())),
            ])),
            ..App::default()
        };
        let buf = render(&modules_focus, 70, height);
        let exp = with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["cpu"],
                "config cpu",
                &["a = \"1\"", "b = \"2\""],
                &[],
                NORMAL_HINT_MODULES,
            ),
            5,
            70,
        );
        assert_eq!(buf, exp);

        let params_focus = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec!["cpu".to_string()]),
            selected_index: Some(0),
            panel_focus: PanelFocus::Params,
            module_params: Some(params_entries(vec![
                ("a".to_string(), ModuleParamValue::String("1".to_string())),
                ("b".to_string(), ModuleParamValue::String("2".to_string())),
            ])),
            ..App::default()
        };
        let buf = render(&params_focus, 70, height);
        let exp = with_reversed_params_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-1/1",
                &["cpu"],
                "config cpu",
                &["a = \"1\"", "b = \"2\""],
                &[],
                NORMAL_HINT_PARAMS,
            ),
            5,
            70,
        );
        assert_eq!(buf, exp);
    }

    #[test]
    fn selected_module_row_style_follows_selection_when_scrolled() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "m0".to_string(),
                "m1".to_string(),
                "m2".to_string(),
                "m3".to_string(),
                "m4".to_string(),
                "m5".to_string(),
            ]),
            module_scroll_offset: 2,
            selected_index: Some(3),
            module_params: Some(params_missing("m3")),
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 2;
        let buffer = render(&app, 70, height);
        let expected_buf = with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 3-4/6",
                &["m2", "m3"],
                "config m3",
                &["(no [m3] section)"],
                &[],
                NORMAL_HINT_MODULES,
            ),
            6,
            70,
        );
        assert_eq!(buffer, expected_buf);
    }

    #[test]
    fn no_module_row_is_reversed_styled_when_selection_is_none() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "cpu".to_string(),
                "disk".to_string(),
                "battery".to_string(),
            ]),
            selected_index: None,
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 3;
        let buffer = render(&app, 70, height);
        for y in 5..=7u16 {
            for x in 0..70 {
                assert!(
                    !buffer[(x, y)].modifier.contains(Modifier::REVERSED),
                    "expected no reversed styling at ({x},{y}) when nothing is selected"
                );
            }
        }
    }

    #[test]
    fn help_lines_modules_focus_lists_local_module_bindings() {
        let app = App {
            panel_focus: PanelFocus::Modules,
            ..App::default()
        };
        let lines = help_lines(&app);
        assert_eq!(lines[0], "--- Local ---");
        assert!(lines.iter().any(|l| l.contains("Add module: a")));
        assert!(lines.iter().any(|l| l.contains("Focus params")));
        assert!(lines.iter().any(|l| l == "--- Global ---"));
        assert!(lines.iter().any(|l| l == "Quit: q"));
    }

    #[test]
    fn help_lines_params_focus_lists_edit_add_del_rename() {
        let app = App {
            panel_focus: PanelFocus::Params,
            ..App::default()
        };
        let lines = help_lines(&app);
        assert!(lines.iter().any(|l| l.contains("Select param")));
        assert!(lines.iter().any(|l| l.contains("Edit value")));
        assert!(lines.iter().any(|l| l.contains("Add param")));
        assert!(lines.iter().any(|l| l.contains("Remove param")));
        assert!(lines.iter().any(|l| l.contains("Rename key")));
        assert!(lines.iter().any(|l| l.contains("Back to modules")));
        assert!(!lines.iter().any(|l| l.contains("Add module")));
        assert!(!lines.iter().any(|l| l.contains("Edit separator")));
    }

    #[test]
    fn compute_fixed_heights_settings_collapses_below_its_threshold() {
        let h = compute_fixed_heights(0);
        assert_eq!(h.settings, 0);
        assert_eq!(h.modules_border, 0);
        assert_eq!(h.hint, 0);
        assert_eq!(h.logs, 0);
        assert_eq!(h.modules_content, 0);
    }

    #[test]
    fn compute_fixed_heights_settings_still_collapses_one_row_below_its_threshold() {
        let h = compute_fixed_heights(2);
        assert_eq!(h.settings, 0);
        assert_eq!(h.modules_border, 2);
        assert_eq!(h.hint, 0);
        assert_eq!(h.logs, 0);
        assert_eq!(h.modules_content, 0);
    }

    #[test]
    fn compute_fixed_heights_settings_fits_at_its_threshold() {
        let h = compute_fixed_heights(3);
        assert_eq!(h.settings, 3);
        assert_eq!(h.modules_border, 0);
        assert_eq!(h.hint, 0);
        assert_eq!(h.logs, 0);
        assert_eq!(h.modules_content, 0);
    }

    #[test]
    fn compute_fixed_heights_modules_border_collapses_below_its_threshold() {
        let h = compute_fixed_heights(4);
        assert_eq!(h.settings, 3);
        assert_eq!(h.modules_border, 0);
        assert_eq!(h.hint, 1);
        assert_eq!(h.logs, 0);
        assert_eq!(h.modules_content, 0);
    }

    #[test]
    fn compute_fixed_heights_modules_border_fits_at_its_threshold() {
        let h = compute_fixed_heights(5);
        assert_eq!(h.settings, 3);
        assert_eq!(h.modules_border, 2);
        assert_eq!(h.hint, 0);
        assert_eq!(h.logs, 0);
        assert_eq!(h.modules_content, 0);
    }

    #[test]
    fn compute_fixed_heights_hint_collapses_below_its_threshold() {
        let h = compute_fixed_heights(5);
        assert_eq!(h.hint, 0);
    }

    #[test]
    fn compute_fixed_heights_hint_fits_at_its_threshold() {
        let h = compute_fixed_heights(6);
        assert_eq!(h.settings, 3);
        assert_eq!(h.modules_border, 2);
        assert_eq!(h.hint, 1);
        assert_eq!(h.logs, 0);
        assert_eq!(h.modules_content, 0);
    }

    #[test]
    fn compute_fixed_heights_logs_collapses_below_its_threshold() {
        let h = compute_fixed_heights(10);
        assert_eq!(h.settings, 3);
        assert_eq!(h.modules_border, 2);
        assert_eq!(h.hint, 1);
        assert_eq!(h.logs, 0);
        assert_eq!(h.modules_content, 4);
    }

    #[test]
    fn compute_fixed_heights_logs_fits_at_its_threshold_and_modules_content_drops_to_zero() {
        let h = compute_fixed_heights(11);
        assert_eq!(h.settings, 3);
        assert_eq!(h.modules_border, 2);
        assert_eq!(h.hint, 1);
        assert_eq!(h.logs, 5);
        assert_eq!(h.modules_content, 0);
    }

    #[test]
    fn compute_fixed_heights_modules_content_grows_linearly_once_everything_else_fits() {
        assert_eq!(compute_fixed_heights(11).modules_content, 0);
        assert_eq!(compute_fixed_heights(12).modules_content, 1);
        assert_eq!(compute_fixed_heights(13).modules_content, 2);
        assert_eq!(compute_fixed_heights(14).modules_content, 3);
    }

    #[test]
    fn modules_viewport_height_matches_compute_fixed_heights_via_frame_height_offset() {
        for frame_height in [0u16, 1, 2, 5, 7, 13, 14, 20] {
            let expected = compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS))
                .modules_content as usize;
            assert_eq!(modules_viewport_height(frame_height), expected);
        }
    }

    #[test]
    fn draw_at_very_short_height_renders_only_what_fits_without_corrupting_borders() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, 4),
            expected(
                70,
                4,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn draw_at_height_with_only_settings_and_hint_present() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, 8),
            expected(
                70,
                8,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules unknown",
                &[],
                "config",
                &[],
                &[],
                NORMAL_HINT_MODULES,
            )
        );
    }

    #[test]
    fn cursor_hidden_in_normal_mode() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(!terminal.backend().cursor_visible());
    }

    #[test]
    fn cursor_visible_and_positioned_at_start_of_empty_buffer() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator {
                buffer: String::new(),
                cursor: 0,
            },
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(terminal.backend().cursor_visible());
        assert_eq!(terminal.backend().cursor_position(), Position::new(18, 2));
    }

    #[test]
    fn cursor_visible_and_positioned_mid_buffer() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator {
                buffer: "ab".to_string(),
                cursor: 1,
            },
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(terminal.backend().cursor_visible());
        assert_eq!(terminal.backend().cursor_position(), Position::new(19, 2));
    }

    #[test]
    fn cursor_visible_and_positioned_at_end_of_buffer() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator {
                buffer: "ab".to_string(),
                cursor: 2,
            },
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(terminal.backend().cursor_visible());
        assert_eq!(terminal.backend().cursor_position(), Position::new(20, 2));
    }

    #[test]
    fn cursor_column_accounts_for_debug_escaped_characters() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator {
                buffer: "a\"b".to_string(),
                cursor: 2,
            },
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(terminal.backend().cursor_visible());
        assert_eq!(terminal.backend().cursor_position(), Position::new(21, 2));
    }

    #[test]
    fn draw_renders_add_picker_title_list_and_hint() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::AddingModule {
                available: vec!["battery".to_string(), "cpu".to_string(), "disk".to_string()],
                selected: 0,
                scroll_offset: 0,
            },
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 5;
        let expected_buf = expected_overlay(
            70,
            height,
            Some(DaemonStatus::Stopped),
            "separator: unknown",
            "add module 1-3/3",
            &["battery", "cpu", "disk"],
            &[],
            "Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc",
        );
        let overlay = overlay_area_for_frame(70, height);
        let inner_y = Block::default().borders(Borders::ALL).inner(overlay).y;
        let expected_buf = with_reversed_overlay_row(expected_buf, inner_y, 70, height);
        assert_eq!(render(&app, 70, height), expected_buf);
    }

    #[test]
    fn draw_renders_add_picker_with_empty_available_list() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::AddingModule {
                available: vec![],
                selected: 0,
                scroll_offset: 0,
            },
            ..App::default()
        };
        assert_eq!(
            render(&app, 70, BASELINE_HEIGHT),
            expected_overlay(
                70,
                BASELINE_HEIGHT,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "add module 0 available",
                &[],
                &[],
                "Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc",
            )
        );
    }

    #[test]
    fn draw_renders_naming_instance_prompt_and_cursor() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::NamingModuleInstance {
                kind: "disk".to_string(),
                buffer: "root".to_string(),
                cursor: 4,
            },
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 1;
        let terminal = render_terminal(&app, 70, height);
        assert_eq!(
            terminal.backend().buffer().clone(),
            expected_overlay(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "name instance of disk",
                &["instance name for disk: \"root\""],
                &[],
                "Confirm: Enter | Cancel: Esc",
            )
        );
        assert!(terminal.backend().cursor_visible());
        let overlay = overlay_area_for_frame(70, height);
        let inner = Block::default().borders(Borders::ALL).inner(overlay);
        let prefix = instance_name_prefix("disk");
        let col = inner.x + text_edit_cursor_column(&prefix, "root", 4);
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::new(col, inner.y)
        );
    }

    #[test]
    fn draw_renders_compact_param_value_overlay_without_prefix() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingParamValue {
                section: "cpu".to_string(),
                key: "format".to_string(),
                buffer: "hi".to_string(),
                cursor: 2,
                expect: ParamWriteExpect::ExistingString("old".to_string()),
            },
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 4;
        let terminal = render_terminal(&app, 70, height);
        assert_eq!(
            terminal.backend().buffer().clone(),
            expected_param_text_overlay(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "value for format",
                "\"hi\"",
                &[],
                "Confirm: Enter | Cancel: Esc",
            )
        );
        let overlay = param_text_overlay_area_for_frame(70, height);
        assert_eq!(overlay.height, PARAM_TEXT_OVERLAY_HEIGHT);
        assert!(terminal.backend().cursor_visible());
        let inner = Block::default().borders(Borders::ALL).inner(overlay);
        let col = inner.x + text_edit_cursor_column("", "hi", 2);
        assert_eq!(
            terminal.backend().cursor_position(),
            Position::new(col, inner.y)
        );
    }

    #[test]
    fn draw_renders_confirming_remove_hint_with_unchanged_list_and_title() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "cpu".to_string(),
                "disk".to_string(),
                "battery".to_string(),
            ]),
            selected_index: Some(1),
            module_params: Some(params_missing("disk")),
            mode: Mode::ConfirmingRemove {
                index: 1,
                name: "disk".to_string(),
            },
            ..App::default()
        };
        let height = BASELINE_HEIGHT + 3;
        let expected_buf = with_reversed_modules_row(
            expected(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "modules 1-3/3",
                &["cpu", "disk", "battery"],
                "config disk",
                &["(no [disk] section)"],
                &[],
                "Remove disk? Confirm: d | Cancel: any key",
            ),
            6,
            70,
        );
        assert_eq!(render(&app, 70, height), expected_buf);
    }

    #[test]
    fn draw_renders_help_title_content_and_hint() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::Help,
            ..App::default()
        };
        let lines = help_lines(&app);
        let height = BASELINE_HEIGHT + lines.len() as u16 + 2;
        let line_refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        assert_eq!(
            render(&app, 70, height),
            expected_overlay(
                70,
                height,
                Some(DaemonStatus::Stopped),
                "separator: unknown",
                "help",
                &line_refs,
                &[],
                HELP_HINT,
            )
        );
    }

    #[test]
    fn cursor_hidden_in_help_mode() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::Help,
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(!terminal.backend().cursor_visible());
    }

    #[test]
    fn cursor_hidden_in_adding_module_mode() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::AddingModule {
                available: vec!["cpu".to_string()],
                selected: 0,
                scroll_offset: 0,
            },
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(!terminal.backend().cursor_visible());
    }

    #[test]
    fn cursor_hidden_in_confirming_remove_mode() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec!["cpu".to_string()]),
            selected_index: Some(0),
            mode: Mode::ConfirmingRemove {
                index: 0,
                name: "cpu".to_string(),
            },
            ..App::default()
        };
        let terminal = render_terminal(&app, 70, BASELINE_HEIGHT);
        assert!(!terminal.backend().cursor_visible());
    }
}
