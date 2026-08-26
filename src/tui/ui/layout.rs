use ratatui::layout::Rect;

pub(super) const OUTER_BORDER_ROWS: u16 = 2;
pub(super) const SETTINGS_BLOCK_HEIGHT: u16 = 3;
pub(super) const MODULES_BORDER_ROWS: u16 = 2;
pub(super) const EXTENSIONS_BORDER_ROWS: u16 = 2;
pub(super) const HINT_HEIGHT: u16 = 1;
pub(super) const LOGS_BORDER_ROWS: u16 = 2;
const OVERLAY_MARGIN_X: u16 = 4;
const OVERLAY_MARGIN_Y: u16 = 1;

pub(super) struct FixedHeights {
    pub(super) settings: u16,
    pub(super) modules_border: u16,
    pub(super) modules_content: u16,
    pub(super) extensions_border: u16,
    pub(super) extensions_content: u16,
    pub(super) hint: u16,
    pub(super) logs: u16,
}

pub(super) fn take(remaining: &mut u16, want: u16) -> u16 {
    if *remaining >= want {
        *remaining -= want;
        want
    } else {
        0
    }
}

fn split_stack_content(stack_content: u16) -> (u16, u16) {
    if stack_content == 0 {
        return (0, 0);
    }
    if stack_content == 1 {
        return (1, 0);
    }
    let extensions = stack_content / 2;
    (stack_content - extensions, extensions)
}

fn allocate_left_stack(middle: u16) -> (u16, u16, u16, u16) {
    let min_for_both = MODULES_BORDER_ROWS + EXTENSIONS_BORDER_ROWS + 1;
    if middle < min_for_both {
        let mut budget = middle;
        let modules_border = take(&mut budget, MODULES_BORDER_ROWS);
        return (modules_border, budget, 0, 0);
    }
    let mut budget = middle;
    let modules_border = take(&mut budget, MODULES_BORDER_ROWS);
    let extensions_border = take(&mut budget, EXTENSIONS_BORDER_ROWS);
    let (modules_content, extensions_content) = split_stack_content(budget);
    (
        modules_border,
        modules_content,
        extensions_border,
        extensions_content,
    )
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

    let (modules_border, modules_content, extensions_border, extensions_content) =
        allocate_left_stack(modules_region);

    FixedHeights {
        settings,
        modules_border,
        modules_content,
        extensions_border,
        extensions_content,
        hint,
        logs,
    }
}

fn heights_from_frame(frame_height: u16) -> FixedHeights {
    compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS))
}

pub(in crate::tui) fn modules_viewport_height(frame_height: u16) -> usize {
    heights_from_frame(frame_height).modules_content as usize
}

pub(in crate::tui) fn extensions_viewport_height(frame_height: u16) -> usize {
    heights_from_frame(frame_height).extensions_content as usize
}

pub(in crate::tui) fn params_viewport_height(frame_height: u16) -> usize {
    let h = heights_from_frame(frame_height);
    (h.modules_content + h.extensions_border + h.extensions_content) as usize
}

pub(in crate::tui) fn logs_viewport_height(frame_height: u16) -> usize {
    heights_from_frame(frame_height)
        .logs
        .saturating_sub(LOGS_BORDER_ROWS) as usize
}

pub(in crate::tui) fn overlay_viewport_height(frame_height: u16) -> usize {
    let heights = heights_from_frame(frame_height);
    let region_height = heights.modules_border
        + heights.modules_content
        + heights.extensions_border
        + heights.extensions_content;
    let margin_y = OVERLAY_MARGIN_Y.min(region_height.saturating_sub(2) / 2);
    let overlay_height = region_height.saturating_sub(margin_y.saturating_mul(2));
    overlay_height.saturating_sub(2) as usize
}

pub(super) struct Areas {
    pub(super) settings: Rect,
    pub(super) modules: Rect,
    pub(super) extensions: Rect,
    pub(super) params: Rect,
    pub(super) logs: Rect,
    pub(super) hint: Rect,
}

pub(super) fn layout_areas(outer_inner: Rect, heights: &FixedHeights) -> Areas {
    let mut y = outer_inner.y;

    let settings = Rect::new(outer_inner.x, y, outer_inner.width, heights.settings);
    y += heights.settings;

    let left_w = outer_inner.width / 2;
    let right_w = outer_inner.width.saturating_sub(left_w);
    let stack_height = heights.modules_border
        + heights.modules_content
        + heights.extensions_border
        + heights.extensions_content;

    let modules = Rect::new(
        outer_inner.x,
        y,
        left_w,
        heights.modules_border + heights.modules_content,
    );
    let extensions = Rect::new(
        outer_inner.x,
        y + heights.modules_border + heights.modules_content,
        left_w,
        heights.extensions_border + heights.extensions_content,
    );
    let params = Rect::new(outer_inner.x + left_w, y, right_w, stack_height);
    y += stack_height;

    let logs = Rect::new(outer_inner.x, y, outer_inner.width, heights.logs);
    y += heights.logs;

    let used = heights.settings + stack_height + heights.logs + heights.hint;
    let spacer = outer_inner.height.saturating_sub(used);
    y += spacer;

    let hint = Rect::new(outer_inner.x, y, outer_inner.width, heights.hint);

    Areas {
        settings,
        modules,
        extensions,
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
        areas.params.height,
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
