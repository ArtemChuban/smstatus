use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent, KeyEventKind};

pub(super) fn next_key_event(timeout: Duration) -> crate::error::Result<Option<KeyEvent>> {
    if !event::poll(timeout)? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => Ok(Some(key)),
        _ => Ok(None),
    }
}
