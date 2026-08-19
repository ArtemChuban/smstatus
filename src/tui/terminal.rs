use std::io::{self, IsTerminal, Stdout};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

pub(super) struct TerminalGuard {
    pub(super) terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    pub(super) fn new() -> crate::error::Result<Self> {
        if !io::stdout().is_terminal() {
            return Err("smstatus tui requires an interactive terminal".into());
        }

        install_panic_hook();

        enable_raw_mode()?;
        if let Err(err) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(err.into());
        }

        match Terminal::new(CrosstermBackend::new(io::stdout())) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(err) => {
                let _ = restore_terminal();
                Err(err.into())
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));
}
