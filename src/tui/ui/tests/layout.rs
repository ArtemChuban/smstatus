use ratatui::widgets::{Block, Borders};

use super::*;

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
    assert_eq!(h.hint, 1);
    assert_eq!(h.modules_border, 0);
    assert_eq!(h.logs, 0);
    assert_eq!(h.modules_content, 0);
}

#[test]
fn compute_fixed_heights_settings_fits_at_its_threshold() {
    let h = compute_fixed_heights(3);
    assert_eq!(h.settings, 3);
    assert_eq!(h.hint, 0);
    assert_eq!(h.modules_border, 0);
    assert_eq!(h.logs, 0);
    assert_eq!(h.modules_content, 0);
}

#[test]
fn compute_fixed_heights_hint_fits_after_settings() {
    let h = compute_fixed_heights(4);
    assert_eq!(h.settings, 3);
    assert_eq!(h.hint, 1);
    assert_eq!(h.modules_border, 0);
    assert_eq!(h.logs, 0);
    assert_eq!(h.modules_content, 0);
}

#[test]
fn compute_fixed_heights_splits_flexible_space_evenly() {
    let h = compute_fixed_heights(14);
    assert_eq!(h.settings, 3);
    assert_eq!(h.hint, 1);
    assert_eq!(h.logs, 5);
    assert_eq!(h.modules_border, 2);
    assert_eq!(h.modules_content, 1);
    assert_eq!(h.extensions_border, 2);
    assert_eq!(h.extensions_content, 0);
}

#[test]
fn compute_fixed_heights_modules_get_ceil_when_flexible_odd() {
    let h = compute_fixed_heights(11);
    assert_eq!(h.settings, 3);
    assert_eq!(h.hint, 1);
    assert_eq!(h.logs, 3);
    assert_eq!(h.modules_border, 2);
    assert_eq!(h.modules_content, 2);
    assert_eq!(h.extensions_border, 0);
    assert_eq!(h.extensions_content, 0);
}

#[test]
fn compute_fixed_heights_grows_both_halves_together() {
    let a = compute_fixed_heights(14);
    let b = compute_fixed_heights(16);
    let stack_a = a.modules_content + a.extensions_content;
    let stack_b = b.modules_content + b.extensions_content;
    assert_eq!(stack_b, stack_a + 1);
    assert_eq!(b.logs, a.logs + 1);
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
fn logs_viewport_height_matches_inner_logs_rows() {
    for frame_height in [0u16, 1, 2, 5, 7, 13, 14, 20, 30] {
        let expected = compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS))
            .logs
            .saturating_sub(2) as usize;
        assert_eq!(logs_viewport_height(frame_height), expected);
    }
}

#[test]
fn overlay_viewport_height_matches_overlay_inner_geometry() {
    for frame_height in [0u16, 1, 2, 5, 7, 13, 14, 20, 24, 40] {
        let heights = compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS));
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: heights.modules_border
                + heights.modules_content
                + heights.extensions_border
                + heights.extensions_content,
        };
        let expected = Block::default()
            .borders(Borders::ALL)
            .inner(overlay_rect(region))
            .height as usize;
        assert_eq!(
            overlay_viewport_height(frame_height),
            expected,
            "frame_height={frame_height}"
        );
    }
}
