use std::borrow::Cow;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::bindings::Metadata;
use crate::config::{BarConfig, ModuleParamValue};

use super::super::app::{
    App, LOGS_PANEL_LINES, Mode, ModuleParamsState, ModuleParamsStatus, PanelFocus,
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
        | Mode::Help => match &app.separator {
            Some(sep) => format!("separator: {sep:?}"),
            None => "separator: unknown".to_string(),
        },
    }
}

pub(super) fn text_edit_cursor_column(prefix: &str, buffer: &str, cursor: usize) -> u16 {
    let byte_idx = super::super::util::char_byte_offset(buffer, cursor);
    let escaped_prefix_quoted = format!("{:?}", &buffer[..byte_idx]);
    let escaped_prefix = &escaped_prefix_quoted[..escaped_prefix_quoted.len() - 1];
    (prefix.chars().count() + escaped_prefix.chars().count()) as u16
}

pub(super) fn hint_line(app: &App) -> Cow<'static, str> {
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
        meta.display_name.clone()
    } else {
        format!("{} ({entry})", meta.display_name)
    }
}

pub(super) fn params_title_text(entry: &str, meta: Option<&Metadata>) -> String {
    let Some(meta) = meta else {
        return format!("config {entry}");
    };
    let kind = BarConfig::split_module_entry(entry).0;
    if entry == kind {
        format!(
            "config {} {} by {}",
            meta.display_name, meta.version, meta.author
        )
    } else {
        format!(
            "config {} ({entry}) {} by {}",
            meta.display_name, meta.version, meta.author
        )
    }
}

pub(super) fn selected_module_entry(app: &App) -> Option<&str> {
    let idx = app.selected_index?;
    app.modules.as_ref()?.get(idx).map(String::as_str)
}

pub(super) fn styled_list_lines(
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

pub(super) fn param_display_lines(state: &ModuleParamsState) -> Vec<String> {
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

pub(super) fn visible_params_lines(
    app: &App,
    viewport_height: usize,
    width: u16,
) -> Vec<Line<'static>> {
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

pub(super) fn log_panel_lines(lines: Vec<String>) -> Vec<String> {
    let mut lines = lines;
    while lines.len() < LOGS_PANEL_LINES {
        lines.push(String::new());
    }
    if lines.len() > LOGS_PANEL_LINES {
        lines = lines.split_off(lines.len() - LOGS_PANEL_LINES);
    }
    lines
}

#[cfg(test)]
mod label_tests {
    use super::{module_list_label, params_title_text};
    use crate::bindings::Metadata;

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
    fn module_list_label_undecorated_uses_display_name() {
        assert_eq!(module_list_label("cpu", Some(&sample_meta())), "CPU");
    }

    #[test]
    fn module_list_label_instance_keeps_entry_in_parens() {
        assert_eq!(
            module_list_label("cpu#home", Some(&sample_meta())),
            "CPU (cpu#home)"
        );
    }

    #[test]
    fn params_title_text_without_metadata_keeps_entry() {
        assert_eq!(params_title_text("cpu", None), "config cpu");
        assert_eq!(params_title_text("disk#root", None), "config disk#root");
    }

    #[test]
    fn params_title_text_undecorated_includes_version_and_author() {
        assert_eq!(
            params_title_text("cpu", Some(&sample_meta())),
            "config CPU 0.1.0 by ArtemChuban"
        );
    }

    #[test]
    fn params_title_text_instance_includes_entry_version_and_author() {
        assert_eq!(
            params_title_text("cpu#home", Some(&sample_meta())),
            "config CPU (cpu#home) 0.1.0 by ArtemChuban"
        );
    }
}
