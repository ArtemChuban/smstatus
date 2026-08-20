mod app;
mod event;
mod terminal;
mod ui;

use std::process::ExitCode;
use std::time::Duration;

use app::App;
use terminal::TerminalGuard;

const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn char_byte_offset(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

pub(crate) fn run() -> ExitCode {
    match run_inner() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("smstatus tui exited with an error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_inner() -> crate::error::Result<()> {
    let mut guard = TerminalGuard::new()?;
    let mut app = App::new();

    while !app.should_quit {
        app.refresh_daemon_status(crate::daemon::status());
        app.poll_pending_start();
        app.poll_config_changes();
        let completed = guard.terminal.draw(|frame| ui::draw(frame, &app))?;
        app.modules_viewport_height = ui::modules_viewport_height(completed.area.height);
        if let Some(key) = event::next_key_event(EVENT_POLL_TIMEOUT)? {
            app.handle_key(key);
        }
    }

    Ok(())
}
