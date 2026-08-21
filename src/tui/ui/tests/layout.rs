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
fn overlay_viewport_height_matches_overlay_inner_geometry() {
    for frame_height in [0u16, 1, 2, 5, 7, 13, 14, 20, 24, 40] {
        let heights = compute_fixed_heights(frame_height.saturating_sub(OUTER_BORDER_ROWS));
        let region = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: heights
                .modules_border
                .saturating_add(heights.modules_content),
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
