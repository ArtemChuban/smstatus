mod app;
mod event;
mod terminal;
mod ui;

use std::process::ExitCode;
use std::time::Duration;

use app::App;
use terminal::TerminalGuard;

const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(250);

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
    let mut app = App::default();

    while !app.should_quit {
        guard.terminal.draw(|frame| ui::draw(frame, &app))?;
        if let Some(key) = event::next_key_event(EVENT_POLL_TIMEOUT)? {
            app.handle_key(key);
        }
    }

    Ok(())
}
