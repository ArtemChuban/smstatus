use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::{ACTION_LOG_CAPACITY, App};

pub(super) fn draw(frame: &mut Frame, app: &App) {
    let block = Block::default().title("smstatus").borders(Borders::ALL);
    let mut lines = vec![
        Line::from(format!("smstatus v{}", env!("CARGO_PKG_VERSION"))),
        Line::from(daemon_status_line(app.daemon_status)),
        Line::from("Press q to quit, s to start, k to stop"),
    ];
    for line in action_log_lines(&app.action_log) {
        lines.push(Line::from(line));
    }
    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(block);

    frame.render_widget(paragraph, frame.area());
}

fn action_log_lines(action_log: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = action_log.to_vec();
    while lines.len() < ACTION_LOG_CAPACITY {
        lines.push(String::new());
    }
    lines
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
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn expected_for(status_text: &str, action_log: &[&str]) -> Buffer {
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered(status_text, 38));
        let hint_line = format!(
            "│{}│",
            centered("Press q to quit, s to start, k to stop", 38)
        );
        let mut action_lines: Vec<String> = action_log
            .iter()
            .map(|text| format!("│{}│", centered(text, 38)))
            .collect();
        while action_lines.len() < ACTION_LOG_CAPACITY {
            action_lines.push(format!("│{}│", centered("", 38)));
        }
        Buffer::with_lines([
            "┌smstatus──────────────────────────────┐".to_string(),
            version_line,
            status_line,
            hint_line,
            action_lines[0].clone(),
            action_lines[1].clone(),
            action_lines[2].clone(),
            "└──────────────────────────────────────┘".to_string(),
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
            daemon_status: Some(DaemonStatus::Stopped),
            ..App::default()
        };
        assert_eq!(render(&app), expected_for("smstatus daemon: stopped", &[]));
    }

    #[test]
    fn draw_renders_running_status() {
        let app = App {
            daemon_status: Some(DaemonStatus::Running { pid: 12345 }),
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: running (pid 12345)", &[])
        );
    }

    #[test]
    fn draw_renders_running_pid_unknown_status() {
        let app = App {
            daemon_status: Some(DaemonStatus::RunningPidUnknown),
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: running (pid unknown)", &[])
        );
    }

    #[test]
    fn draw_renders_unknown_status() {
        let app = App {
            daemon_status: None,
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: status unknown", &[])
        );
    }

    #[test]
    fn draw_renders_empty_action_log_as_blank_rows() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            action_log: vec![],
            ..App::default()
        };
        assert_eq!(render(&app), expected_for("smstatus daemon: stopped", &[]));
    }

    #[test]
    fn draw_renders_single_action_message() {
        let app = App {
            daemon_status: Some(DaemonStatus::Running { pid: 42 }),
            action_log: vec!["Starting smstatus...".to_string()],
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for(
                "smstatus daemon: running (pid 42)",
                &["Starting smstatus..."]
            )
        );
    }

    #[test]
    fn draw_renders_action_log_at_capacity() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            action_log: vec![
                "Starting smstatus...".to_string(),
                "smstatus is already running".to_string(),
                "Sent stop signal to smstatus (pid 42)".to_string(),
            ],
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for(
                "smstatus daemon: stopped",
                &[
                    "Starting smstatus...",
                    "smstatus is already running",
                    "Sent stop signal to smstatus (pid 42)",
                ]
            )
        );
    }
}
