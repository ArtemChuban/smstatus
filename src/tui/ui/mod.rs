use ratatui::Frame;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use super::app::{App, Mode};

mod layout;
mod overlay;
mod render;

pub(in crate::tui) use layout::{
    logs_viewport_height, modules_viewport_height, overlay_viewport_height,
};
pub(in crate::tui) use render::help_lines;

use layout::{compute_fixed_heights, layout_areas, modules_region};
use overlay::{
    draw_add_overlay, draw_extensions_overlay, draw_help_overlay, draw_naming_overlay,
    draw_param_text_overlay,
};
use render::{
    SEPARATOR_EDIT_PREFIX, boxed_title, draw_modules_column, draw_params_column, hint_line,
    logs_title, outer_title, separator_line, text_edit_cursor_column, visible_log_lines,
};

#[cfg(test)]
use layout::{OUTER_BORDER_ROWS, overlay_rect};
#[cfg(test)]
use overlay::{PARAM_TEXT_OVERLAY_HEIGHT, param_text_overlay_rect};
#[cfg(test)]
use render::{daemon_status_phrase, instance_name_prefix, visible_params_lines};

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
        let logs_viewport = areas.logs.height.saturating_sub(2) as usize;
        let logs_block = Block::default()
            .title(logs_title(app, logs_viewport))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded);
        let logs_inner = logs_block.inner(areas.logs);
        frame.render_widget(logs_block, areas.logs);
        let log_lines = visible_log_lines(app, &app.log_history, logs_inner.width, logs_viewport);
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
            Mode::BrowsingExtensions {
                selected,
                scroll_offset,
            } => {
                let labels = app.extension_overlay_labels();
                draw_extensions_overlay(frame, region, labels, *selected, *scroll_offset)
            }
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

#[cfg(test)]
mod tests;
