use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
pub(super) struct App {
    pub(super) should_quit: bool,
    pub(super) daemon_status: Option<crate::daemon::DaemonStatus>,
}

impl App {
    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if is_quit(key) {
            self.should_quit = true;
        }
    }

    pub(super) fn refresh_daemon_status(
        &mut self,
        status: crate::error::Result<crate::daemon::DaemonStatus>,
    ) {
        self.daemon_status = status.ok();
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

    #[test]
    fn refresh_daemon_status_stores_ok_value() {
        let mut app = App::default();
        app.refresh_daemon_status(Ok(crate::daemon::DaemonStatus::Running { pid: 42 }));
        assert_eq!(
            app.daemon_status,
            Some(crate::daemon::DaemonStatus::Running { pid: 42 })
        );
    }

    #[test]
    fn refresh_daemon_status_maps_err_to_none() {
        let mut app = App::default();
        app.refresh_daemon_status(Err("boom".into()));
        assert_eq!(app.daemon_status, None);
    }
}
