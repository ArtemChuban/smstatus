use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
pub(super) struct App {
    pub(super) should_quit: bool,
}

impl App {
    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if is_quit(key) {
            self.should_quit = true;
        }
    }
}

fn is_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        || (key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn q_key_requests_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_c_requests_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_d_requests_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn unrelated_key_does_not_request_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(!app.should_quit);
    }

    #[test]
    fn plain_c_without_control_does_not_request_quit() {
        let mut app = App::default();
        app.handle_key(key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!app.should_quit);
    }
}
