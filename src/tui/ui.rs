use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::App;

pub(super) fn draw(frame: &mut Frame, app: &App) {
    let block = Block::default().title("smstatus").borders(Borders::ALL);
    let paragraph = Paragraph::new(vec![
        Line::from(format!("smstatus v{}", env!("CARGO_PKG_VERSION"))),
        Line::from(daemon_status_line(app.daemon_status)),
        Line::from("Press q to quit"),
    ])
    .alignment(Alignment::Center)
    .block(block);

    frame.render_widget(paragraph, frame.area());
}

fn daemon_status_line(status: Option<crate::daemon::DaemonStatus>) -> String {
    use crate::daemon::DaemonStatus;
    match status {
        Some(DaemonStatus::Running { pid }) => format!("smstatus daemon: running (pid {pid})"),
        Some(DaemonStatus::RunningPidUnknown) => {
            "smstatus daemon: running (pid unknown)".to_string()
        }
        Some(DaemonStatus::Stopped) => "smstatus daemon: stopped".to_string(),
        None => "smstatus daemon: status unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::daemon::DaemonStatus;
    use crate::tui::app::App;

    fn centered(content: &str, width: usize) -> String {
        let content_width = content.chars().count();
        let left = width / 2 - content_width / 2;
        let right = width - content_width - left;
        format!("{}{content}{}", " ".repeat(left), " ".repeat(right))
    }

    fn render(app: &App) -> Buffer {
        let backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn expected_for(status_text: &str) -> Buffer {
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered(status_text, 38));
        Buffer::with_lines([
            "┌smstatus──────────────────────────────┐",
            &version_line,
            &status_line,
            "│            Press q to quit           │",
            "│                                      │",
            "└──────────────────────────────────────┘",
        ])
    }

    #[test]
    fn daemon_status_line_running() {
        assert_eq!(
            daemon_status_line(Some(DaemonStatus::Running { pid: 42 })),
            "smstatus daemon: running (pid 42)"
        );
    }

    #[test]
    fn daemon_status_line_running_pid_unknown() {
        assert_eq!(
            daemon_status_line(Some(DaemonStatus::RunningPidUnknown)),
            "smstatus daemon: running (pid unknown)"
        );
    }

    #[test]
    fn daemon_status_line_stopped() {
        assert_eq!(
            daemon_status_line(Some(DaemonStatus::Stopped)),
            "smstatus daemon: stopped"
        );
    }

    #[test]
    fn daemon_status_line_unknown() {
        assert_eq!(daemon_status_line(None), "smstatus daemon: status unknown");
    }

    #[test]
    fn draw_renders_stopped_status() {
        let app = App {
            should_quit: false,
            daemon_status: Some(DaemonStatus::Stopped),
        };
        assert_eq!(render(&app), expected_for("smstatus daemon: stopped"));
    }

    #[test]
    fn draw_renders_running_status() {
        let app = App {
            should_quit: false,
            daemon_status: Some(DaemonStatus::Running { pid: 12345 }),
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: running (pid 12345)")
        );
    }

    #[test]
    fn draw_renders_running_pid_unknown_status() {
        let app = App {
            should_quit: false,
            daemon_status: Some(DaemonStatus::RunningPidUnknown),
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: running (pid unknown)")
        );
    }

    #[test]
    fn draw_renders_unknown_status() {
        let app = App {
            should_quit: false,
            daemon_status: None,
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: status unknown")
        );
    }
}
