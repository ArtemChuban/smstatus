use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn clamped_scroll_offset(offset: usize, idx: usize, viewport_height: usize) -> usize {
    let viewport = viewport_height.max(1);
    if idx < offset {
        idx
    } else if idx >= offset + viewport {
        idx + 1 - viewport
    } else {
        offset
    }
}

pub(super) fn is_hard_quit(key: KeyEvent) -> bool {
    (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('d'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub(super) fn is_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q')) || is_hard_quit(key)
}

pub(super) fn is_valid_param_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(super) fn apply_text_edit(buffer: &mut String, cursor: &mut usize, key: KeyEvent) {
    match key.code {
        KeyCode::Left => {
            *cursor = cursor.saturating_sub(1);
        }
        KeyCode::Right => {
            *cursor = (*cursor + 1).min(buffer.chars().count());
        }
        KeyCode::Backspace => {
            if *cursor > 0 {
                let byte = super::super::util::char_byte_offset(buffer, *cursor - 1);
                buffer.remove(byte);
                *cursor -= 1;
            }
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            let byte = super::super::util::char_byte_offset(buffer, *cursor);
            buffer.insert(byte, c);
            *cursor += 1;
        }
        _ => {}
    }
}
