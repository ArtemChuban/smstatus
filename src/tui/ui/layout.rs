use ratatui::layout::Rect;

pub(super) const OUTER_BORDER_ROWS: u16 = 2;
pub(super) const SETTINGS_BLOCK_HEIGHT: u16 = 3;
pub(super) const MODULES_BORDER_ROWS: u16 = 2;
pub(super) const HINT_HEIGHT: u16 = 1;
pub(super) const LOGS_BORDER_ROWS: u16 = 2;
const OVERLAY_MARGIN_X: u16 = 4;
const OVERLAY_MARGIN_Y: u16 = 1;

pub(super) struct FixedHeights {
    pub(super) settings: u16,
    pub(super) modules_border: u16,
    pub(super) hint: u16,
    pub(super) logs: u16,
    pub(super) modules_content: u16,
}

pub(super) fn take(remaining: &mut u16, want: u16) -> u16 {
    if *remaining >= want {
        *remaining -= want;
        want
    } else {
        0
    }
}

pub(super) fn compute_fixed_heights(outer_inner_height: u16) -> FixedHeights {
    let mut remaining = outer_inner_height;
    let settings = take(&mut remaining, SETTINGS_BLOCK_HEIGHT);
    let hint = take(&mut remaining, HINT_HEIGHT);

    let mut modules_region = remaining.div_ceil(2);
    let mut logs = remaining.saturating_sub(modules_region);

    if logs > 0 && logs < LOGS_BORDER_ROWS {
        modules_region = modules_region.saturating_add(logs);
        logs = 0;
    }
    if modules_region > 0 && modules_region < MODULES_BORDER_ROWS {
        logs = logs.saturating_add(modules_region);
        modules_region = 0;
        if logs > 0 && logs < LOGS_BORDER_ROWS {
            logs = 0;
        }
    }

    let mut modules_budget = modules_region;
    let modules_border = take(&mut modules_budget, MODULES_BORDER_ROWS);
    let modules_content = if modules_border > 0 {
        modules_budget
    } else {
        0
    };

    FixedHeights {
        settings,
        modules_border,
        hint,
        logs,
        modules_content,
    }
}

pub(in crate::tui) fn modules_viewport_height(frame_height: u16) -> usize {
    compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS)).modules_content as usize
}

pub(in crate::tui) fn logs_viewport_height(frame_height: u16) -> usize {
    compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS))
        .logs
        .saturating_sub(LOGS_BORDER_ROWS) as usize
}

pub(in crate::tui) fn overlay_viewport_height(frame_height: u16) -> usize {
    let heights = compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS));
    let region_height = heights
        .modules_border
        .saturating_add(heights.modules_content);
    let margin_y = OVERLAY_MARGIN_Y.min(region_height.saturating_sub(2) / 2);
    let overlay_height = region_height.saturating_sub(margin_y.saturating_mul(2));
    overlay_height.saturating_sub(2) as usize
}

pub(super) struct Areas {
    pub(super) settings: Rect,
    pub(super) modules: Rect,
    pub(super) params: Rect,
    pub(super) logs: Rect,
    pub(super) hint: Rect,
}

pub(super) fn layout_areas(outer_inner: Rect, heights: &FixedHeights) -> Areas {
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

    let used = heights.settings
        + heights.modules_border
        + heights.modules_content
        + heights.logs
        + heights.hint;
    let spacer = outer_inner.height.saturating_sub(used);
    y += spacer;

    let hint = Rect::new(outer_inner.x, y, outer_inner.width, heights.hint);

    Areas {
        settings,
        modules,
        params,
        logs,
        hint,
    }
}

pub(super) fn modules_region(areas: &Areas) -> Rect {
    Rect::new(
        areas.modules.x,
        areas.modules.y,
        areas.modules.width.saturating_add(areas.params.width),
        areas.modules.height,
    )
}

pub(super) fn overlay_rect(region: Rect) -> Rect {
    let margin_x = OVERLAY_MARGIN_X.min(region.width.saturating_sub(2) / 2);
    let margin_y = OVERLAY_MARGIN_Y.min(region.height.saturating_sub(2) / 2);
    Rect {
        x: region.x.saturating_add(margin_x),
        y: region.y.saturating_add(margin_y),
        width: region.width.saturating_sub(margin_x.saturating_mul(2)),
        height: region.height.saturating_sub(margin_y.saturating_mul(2)),
    }
}
