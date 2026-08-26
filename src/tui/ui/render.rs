use std::borrow::Cow;

use log::Level;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::config::{BarConfig, ModuleParamValue};
use crate::logging::parse_log_level;
use crate::manifest::Metadata;

use super::super::app::{
    App, Mode, ModuleParamsState, ModuleParamsStatus, PanelFocus, ParamEntry, ParamOrigin,
};

pub(super) const SEPARATOR_EDIT_PREFIX: &str = "New separator: ";

pub(super) fn draw_modules_column(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    viewport_height: usize,
) {
    let modules_block = Block::default()
        .title(modules_title(app, viewport_height))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let modules_inner = modules_block.inner(area);
    frame.render_widget(modules_block, area);
    let module_lines = visible_module_lines(app, viewport_height, modules_inner.width);
    frame.render_widget(Paragraph::new(module_lines), modules_inner);
}

pub(super) fn draw_params_column(frame: &mut Frame, app: &App, area: Rect, viewport_height: usize) {
    let params_block = Block::default()
        .title(params_title(app))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let params_inner = params_block.inner(area);
    frame.render_widget(params_block, area);
    let lines = visible_params_lines(app, viewport_height, params_inner.width);
    frame.render_widget(Paragraph::new(lines), params_inner);
}

pub(super) fn boxed_title(text: &str) -> String {
    format!("─{text}")
}

pub(super) fn outer_title(app: &App) -> String {
    boxed_title(&format!(
        "smstatus v{} {}",
        env!("CARGO_PKG_VERSION"),
        daemon_status_phrase(app.daemon_status)
    ))
}

pub(super) fn daemon_status_phrase(status: Option<crate::daemon::DaemonStatus>) -> String {
    use crate::daemon::DaemonStatus;
    match status {
        Some(DaemonStatus::Running { pid }) => format!("running (pid {pid})"),
        Some(DaemonStatus::RunningPidUnknown) => "running (pid unknown)".to_string(),
        Some(DaemonStatus::Stopped) => "stopped".to_string(),
        None => "status unknown".to_string(),
    }
}

pub(super) fn separator_line(app: &App) -> String {
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
        | Mode::BrowsingExtensions { .. }
        | Mode::ChoosingInstallKind { .. }
        | Mode::EnteringInstallSource { .. }
        | Mode::Help => match &app.separator {
            Some(sep) => format!("separator: {sep:?}"),
            None => "separator: unknown".to_string(),
        },
    }
}

pub(super) fn text_edit_cursor_column(prefix: &str, buffer: &str, cursor: usize) -> u16 {
    let byte_idx = super::super::util::char_byte_offset(buffer, cursor);
    let visible = buffer.get(..byte_idx).unwrap_or_default();
    let escaped_prefix_quoted = format!("{visible:?}");
    let escaped_prefix = escaped_prefix_quoted
        .get(..escaped_prefix_quoted.len() - 1)
        .unwrap_or_default();
    (prefix.chars().count() + escaped_prefix.chars().count()) as u16
}

pub(super) fn hint_line(app: &App) -> Cow<'static, str> {
    match &app.mode {
        Mode::Normal => match app.panel_focus {
            PanelFocus::Modules => Cow::Borrowed(
                "Select: \u{2191}/\u{2193} | Params: Enter/\u{2192} | Logs: Tab | Ext: x | Install: i | Quit: q | Start: s | Kill: k | Help: ?",
            ),
            PanelFocus::Params => Cow::Borrowed(
                "Select: \u{2191}/\u{2193} | Edit: e/Enter | Add: a | Del: d | Rename: r | Logs: Tab | Back: Esc/\u{2190} | Quit: q | Start: s | Kill: k | Help: ?",
            ),
            PanelFocus::Logs => Cow::Borrowed(
                "Scroll: \u{2191}/\u{2193} | Levels: e/w/i | Back: Esc/\u{2190}/Tab | Quit: q | Start: s | Kill: k | Help: ?",
            ),
        },
        Mode::EditingSeparator { .. } => Cow::Borrowed("Save: Enter | Cancel: Esc"),
        Mode::AddingModule { .. } => {
            Cow::Borrowed("Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc")
        }
        Mode::BrowsingExtensions { .. } => {
            Cow::Borrowed("Select: \u{2191}/\u{2193} | Install: i | Close: Esc")
        }
        Mode::ChoosingInstallKind { .. } => {
            Cow::Borrowed("Select: \u{2191}/\u{2193} | Next: Enter | Cancel: Esc")
        }
        Mode::EnteringInstallSource { .. } => Cow::Borrowed("Confirm: Enter | Cancel: Esc"),
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

pub(super) fn instance_name_prefix(kind: &str) -> String {
    format!("instance name for {kind}: ")
}

pub(super) fn module_window(
    total: usize,
    offset: usize,
    viewport_height: usize,
) -> (usize, usize, usize) {
    if total == 0 {
        return (0, 0, 0);
    }
    let offset = offset.min(total.saturating_sub(viewport_height));
    let end = (offset + viewport_height).min(total);
    (offset, end, total)
}

pub(super) fn modules_title(app: &App, viewport_height: usize) -> String {
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

pub(super) fn logs_title(app: &App, viewport_height: usize) -> String {
    let total = app.logs_total;
    let text = if total == 0 {
        if app.logs_filter_active() && app.logs_file_total > 0 {
            format!("logs 0 of {}", app.logs_file_total)
        } else {
            "logs".to_string()
        }
    } else if viewport_height == 0 {
        if app.logs_filter_active() {
            format!("logs {total} of {}", app.logs_file_total)
        } else {
            format!("logs {total}")
        }
    } else {
        let (start, end, _) = module_window(
            total,
            logs_view_offset(app, total, viewport_height),
            viewport_height,
        );
        if app.logs_filter_active() {
            format!(
                "logs {}-{end}/{total} of {}",
                start + 1,
                app.logs_file_total
            )
        } else {
            format!("logs {}-{end}/{total}", start + 1)
        }
    };
    boxed_title(&text)
}

pub(super) fn params_title(app: &App) -> String {
    let text = match selected_module_entry(app) {
        Some(entry) => {
            let kind = BarConfig::split_module_entry(entry).0;
            params_title_text(entry, app.metadata_by_kind.get(kind))
        }
        None => "config".to_string(),
    };
    boxed_title(&text)
}

pub(super) fn module_list_label(entry: &str, meta: Option<&Metadata>) -> String {
    let Some(meta) = meta else {
        return entry.to_string();
    };
    let kind = BarConfig::split_module_entry(entry).0;
    if entry == kind {
        format!("{} {} by {}", meta.display_name, meta.version, meta.author)
    } else {
        format!(
            "{} ({entry}) {} by {}",
            meta.display_name, meta.version, meta.author
        )
    }
}

pub(super) fn params_title_text(entry: &str, meta: Option<&Metadata>) -> String {
    let Some(meta) = meta else {
        return format!("config {entry}");
    };
    let kind = BarConfig::split_module_entry(entry).0;
    if entry == kind {
        format!("config {}", meta.display_name)
    } else {
        format!("config {} ({entry})", meta.display_name)
    }
}

pub(super) fn selected_module_entry(app: &App) -> Option<&str> {
    let idx = app.selected_index?;
    app.modules.as_ref()?.get(idx).map(String::as_str)
}

fn styled_dimmable_lines(
    entries: &[(String, bool)],
    selected: Option<usize>,
    offset: usize,
    viewport_height: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let (start, end, _total) = module_window(entries.len(), offset, viewport_height);
    entries
        .get(start..end)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(i, (text, dim))| {
            let mut modifier = Modifier::empty();
            if *dim {
                modifier |= Modifier::DIM;
            }
            if Some(start + i) == selected {
                modifier |= Modifier::REVERSED;
                let display_width = text.width();
                let padding = " ".repeat((width as usize).saturating_sub(display_width));
                Line::styled(
                    format!("{text}{padding}"),
                    Style::default().add_modifier(modifier),
                )
            } else if modifier.is_empty() {
                Line::from(text.clone())
            } else {
                Line::styled(text.clone(), Style::default().add_modifier(modifier))
            }
        })
        .collect()
}

pub(super) fn styled_list_lines(
    entries: &[String],
    selected: Option<usize>,
    offset: usize,
    viewport_height: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let entries: Vec<(String, bool)> = entries.iter().map(|name| (name.clone(), false)).collect();
    styled_dimmable_lines(&entries, selected, offset, viewport_height, width)
}

pub(super) fn visible_module_lines(
    app: &App,
    viewport_height: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let Some(modules) = &app.modules else {
        return Vec::new();
    };
    let labels: Vec<String> = modules
        .iter()
        .map(|entry| {
            let kind = BarConfig::split_module_entry(entry).0;
            module_list_label(entry, app.metadata_by_kind.get(kind))
        })
        .collect();
    let selected = if app.panel_focus == PanelFocus::Modules {
        app.selected_index
    } else {
        None
    };
    styled_list_lines(
        &labels,
        selected,
        app.module_scroll_offset,
        viewport_height,
        width,
    )
}

pub(super) fn param_display_lines(state: &ModuleParamsState) -> Vec<(String, bool)> {
    match &state.status {
        ModuleParamsStatus::Missing { section } => {
            vec![(format!("(no [{section}] section)"), false)]
        }
        ModuleParamsStatus::Empty => vec![("(empty)".to_string(), false)],
        ModuleParamsStatus::Entries => state
            .entries
            .iter()
            .map(|ParamEntry { key, value, origin }| {
                let text = match value {
                    ModuleParamValue::String(s) => format!("{key} = {s:?}"),
                    ModuleParamValue::NonString => format!("{key} = <non-string>"),
                };
                match origin {
                    ParamOrigin::Default => (format!("{text} (default)"), true),
                    ParamOrigin::Explicit => (text, false),
                }
            })
            .collect(),
    }
}

pub(super) fn requirement_header_lines(app: &App) -> Vec<(String, bool)> {
    let Some(entry) = selected_module_entry(app) else {
        return Vec::new();
    };
    let kind = BarConfig::split_module_entry(entry).0;
    app.requirement_lines_by_kind
        .get(kind)
        .map(|lines| {
            lines
                .iter()
                .map(|line| (line.clone(), true))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn visible_params_lines(
    app: &App,
    viewport_height: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let Some(state) = &app.module_params else {
        return Vec::new();
    };
    let header = requirement_header_lines(app);
    let header_len = header.len().min(viewport_height);
    let mut lines = styled_dimmable_lines(&header[..header_len], None, 0, header_len, width);
    let param_viewport = viewport_height.saturating_sub(header_len);
    if param_viewport == 0 {
        return lines;
    }
    let param_lines = param_display_lines(state);
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
    lines.extend(styled_dimmable_lines(
        &param_lines,
        selected,
        offset,
        param_viewport,
        width,
    ));
    lines
}

pub(in crate::tui) fn help_lines(app: &App) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push("--- Local ---".to_string());
    match &app.mode {
        Mode::Normal => match app.panel_focus {
            PanelFocus::Modules => {
                lines.push("Select module: \u{2191}/\u{2193}".to_string());
                lines.push("Move module: Ctrl+\u{2191}/\u{2193}".to_string());
                lines.push("Add module: a".to_string());
                lines.push("Remove module: d".to_string());
                lines.push("Browse extensions: x".to_string());
                lines.push("Install module/extension: i".to_string());
                lines.push("Edit separator: e".to_string());
                lines.push("Focus params: Enter/\u{2192}".to_string());
                lines.push("Focus logs: Tab".to_string());
            }
            PanelFocus::Params => {
                lines.push("Select param: \u{2191}/\u{2193}".to_string());
                lines.push("Edit value: e/Enter".to_string());
                lines.push("Add param: a".to_string());
                lines.push("Remove param: d".to_string());
                lines.push("Rename key: r".to_string());
                lines.push("Focus logs: Tab".to_string());
                lines.push("Back to modules: Esc/\u{2190}".to_string());
            }
            PanelFocus::Logs => {
                lines.push("Scroll logs: \u{2191}/\u{2193}".to_string());
                lines.push("Toggle ERROR/WARN/INFO: e/w/i".to_string());
                lines.push("Back to modules: Esc/\u{2190}/Tab".to_string());
            }
        },
        Mode::Help => {
            lines.push("Close help: ? or Esc".to_string());
            lines.push("Scroll help: \u{2191}/\u{2193}".to_string());
        }
        Mode::BrowsingExtensions { .. } => {
            lines.push("Select extension: \u{2191}/\u{2193}".to_string());
            lines.push("Install: i".to_string());
            lines.push("Close: Esc".to_string());
        }
        Mode::ChoosingInstallKind { .. } => {
            lines.push("Select install kind: \u{2191}/\u{2193}".to_string());
            lines.push("Next: Enter".to_string());
            lines.push("Cancel: Esc".to_string());
        }
        Mode::EnteringInstallSource { .. } => {
            lines.push("Confirm source: Enter".to_string());
            lines.push("Cancel: Esc".to_string());
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

pub(super) fn logs_view_offset(app: &App, history_len: usize, viewport_height: usize) -> usize {
    if history_len == 0 {
        0
    } else {
        app.logs_scroll_offset
            .min(history_len.saturating_sub(viewport_height.max(1)))
    }
}

fn style_for_log_level(level: Option<Level>) -> Style {
    match level {
        Some(Level::Error) => Style::default().fg(Color::Red),
        Some(Level::Warn) => Style::default().fg(Color::Yellow),
        Some(Level::Info) => Style::default().fg(Color::Cyan),
        Some(Level::Debug) | Some(Level::Trace) => Style::default().add_modifier(Modifier::DIM),
        None => Style::default(),
    }
}

pub(super) fn visible_log_lines(
    app: &App,
    history: &[String],
    width: u16,
    viewport_height: usize,
) -> Vec<Line<'static>> {
    let total = app.logs_total;
    let abs_offset = logs_view_offset(app, total, viewport_height);
    let loaded_from = app.logs_loaded_from;
    let rel_offset = abs_offset.saturating_sub(loaded_from);
    let selected = if app.panel_focus == PanelFocus::Logs {
        app.logs_selected_index.and_then(|abs| {
            abs.checked_sub(loaded_from)
                .filter(|rel| *rel < history.len())
        })
    } else {
        None
    };
    let (start, end, _total) = module_window(history.len(), rel_offset, viewport_height);
    let mut lines: Vec<Line<'static>> = history
        .get(start..end)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let mut style = style_for_log_level(parse_log_level(text));
            if Some(start + i) == selected {
                style = style.add_modifier(Modifier::REVERSED);
                let display_width = text.width();
                let padding = " ".repeat((width as usize).saturating_sub(display_width));
                Line::styled(format!("{text}{padding}"), style)
            } else if style == Style::default() {
                Line::from(text.clone())
            } else {
                Line::styled(text.clone(), style)
            }
        })
        .collect();
    while lines.len() < viewport_height {
        lines.push(Line::from(String::new()));
    }
    lines
}

#[cfg(test)]
mod label_tests {
    use super::{module_list_label, params_title_text};
    use crate::manifest::Metadata;

    fn sample_meta() -> Metadata {
        Metadata {
            display_name: "CPU".to_string(),
            version: "0.1.0".to_string(),
            author: "ArtemChuban".to_string(),
        }
    }

    #[test]
    fn module_list_label_without_metadata_returns_entry() {
        assert_eq!(module_list_label("cpu", None), "cpu");
        assert_eq!(module_list_label("disk#root", None), "disk#root");
    }

    #[test]
    fn module_list_label_undecorated_includes_version_and_author() {
        assert_eq!(
            module_list_label("cpu", Some(&sample_meta())),
            "CPU 0.1.0 by ArtemChuban"
        );
    }

    #[test]
    fn module_list_label_instance_keeps_entry_in_parens() {
        assert_eq!(
            module_list_label("cpu#home", Some(&sample_meta())),
            "CPU (cpu#home) 0.1.0 by ArtemChuban"
        );
    }

    #[test]
    fn params_title_text_without_metadata_keeps_entry() {
        assert_eq!(params_title_text("cpu", None), "config cpu");
        assert_eq!(params_title_text("disk#root", None), "config disk#root");
    }

    #[test]
    fn params_title_text_undecorated_uses_display_name() {
        assert_eq!(params_title_text("cpu", Some(&sample_meta())), "config CPU");
    }

    #[test]
    fn params_title_text_instance_includes_entry() {
        assert_eq!(
            params_title_text("cpu#home", Some(&sample_meta())),
            "config CPU (cpu#home)"
        );
    }
}

#[cfg(test)]
mod param_display_lines_tests {
    use super::*;

    #[test]
    fn explicit_entry_has_no_default_marker() {
        let state = ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![ParamEntry {
                key: "path".to_string(),
                value: ModuleParamValue::String("/sys".to_string()),
                origin: ParamOrigin::Explicit,
            }],
            selected_index: Some(0),
            scroll_offset: 0,
        };
        assert_eq!(
            param_display_lines(&state),
            vec![("path = \"/sys\"".to_string(), false)]
        );
    }

    #[test]
    fn default_entry_is_marked_and_flagged() {
        let state = ModuleParamsState {
            status: ModuleParamsStatus::Entries,
            entries: vec![ParamEntry {
                key: "path".to_string(),
                value: ModuleParamValue::String("/sys".to_string()),
                origin: ParamOrigin::Default,
            }],
            selected_index: Some(0),
            scroll_offset: 0,
        };
        assert_eq!(
            param_display_lines(&state),
            vec![("path = \"/sys\" (default)".to_string(), true)]
        );
    }
}

#[cfg(test)]
mod visible_log_lines_tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn visible_log_lines_styles_error_warn_info_distinctly() {
        let history = vec![
            "2026-08-24T08:41:00.000Z ERROR boom".to_string(),
            "2026-08-24T08:41:00.000Z WARN careful".to_string(),
            "2026-08-24T08:41:00.000Z INFO hello".to_string(),
        ];
        let app = App {
            logs_total: history.len(),
            logs_loaded_from: 0,
            logs_scroll_offset: 0,
            logs_follow: false,
            panel_focus: PanelFocus::Modules,
            ..App::default()
        };
        let lines = visible_log_lines(&app, &history, 80, 3);
        assert_eq!(lines[0].style.fg, Some(Color::Red));
        assert_eq!(lines[1].style.fg, Some(Color::Yellow));
        assert_eq!(lines[2].style.fg, Some(Color::Cyan));
    }
}
