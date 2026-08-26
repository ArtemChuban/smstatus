use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::super::app::App;
use super::layout::overlay_rect;
use super::render::{
    boxed_title, help_lines, instance_name_prefix, module_window, styled_list_lines,
    text_edit_cursor_column,
};

pub(super) fn draw_help_overlay(frame: &mut Frame, app: &App, region: Rect) {
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

pub(super) fn draw_list_overlay(
    frame: &mut Frame,
    region: Rect,
    title_prefix: &str,
    count_noun: &str,
    labels: &[String],
    selected: usize,
    scroll_offset: usize,
) {
    let area = overlay_rect(region);
    let viewport_height = Block::default().borders(Borders::ALL).inner(area).height as usize;
    let (start, end, total) = module_window(labels.len(), scroll_offset, viewport_height);
    let title = if viewport_height == 0 || total == 0 {
        format!("{title_prefix} {total} {count_noun}")
    } else {
        format!("{title_prefix} {}-{end}/{total}", start + 1)
    };
    let block = Block::default()
        .title(boxed_title(&title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let lines = styled_list_lines(
        labels,
        Some(selected),
        scroll_offset,
        viewport_height,
        inner.width,
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw_add_overlay(
    frame: &mut Frame,
    region: Rect,
    available: &[String],
    selected: usize,
    scroll_offset: usize,
) {
    draw_list_overlay(
        frame,
        region,
        "add module",
        "available",
        available,
        selected,
        scroll_offset,
    );
}

pub(super) fn draw_install_kind_overlay(
    frame: &mut Frame,
    region: Rect,
    labels: &[String],
    selected: usize,
) {
    let area = overlay_rect(region);
    let viewport_height = Block::default().borders(Borders::ALL).inner(area).height as usize;
    let title = "install";
    let block = Block::default()
        .title(boxed_title(title))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let lines = styled_list_lines(labels, Some(selected), 0, viewport_height, inner.width);
    frame.render_widget(Paragraph::new(lines), inner);
}

pub(super) fn draw_naming_overlay(
    frame: &mut Frame,
    region: Rect,
    kind: &str,
    buffer: &str,
    cursor: usize,
) {
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

pub(super) const PARAM_TEXT_OVERLAY_HEIGHT: u16 = 3;

pub(super) fn param_text_overlay_rect(region: Rect) -> Rect {
    let full = overlay_rect(region);
    let height = PARAM_TEXT_OVERLAY_HEIGHT.min(full.height);
    Rect {
        x: full.x,
        y: full.y + full.height.saturating_sub(height) / 2,
        width: full.width,
        height,
    }
}

pub(super) fn draw_param_text_overlay(
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

pub(super) fn draw_preset_list_overlay(
    frame: &mut Frame,
    region: Rect,
    names: &[String],
    selected: usize,
    scroll_offset: usize,
    active: Option<&str>,
) {
    let labels: Vec<String> = names
        .iter()
        .map(|name| {
            if active == Some(name.as_str()) {
                format!("{name} (active)")
            } else {
                name.clone()
            }
        })
        .collect();
    draw_list_overlay(
        frame,
        region,
        "presets",
        "presets",
        &labels,
        selected,
        scroll_offset,
    );
}
