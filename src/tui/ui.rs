use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::{ACTION_LOG_CAPACITY, App, Mode};

const FIXED_INTERIOR_LINES: usize = 5 + ACTION_LOG_CAPACITY;

pub(super) fn draw(frame: &mut Frame, app: &App) {
    let viewport_height = frame
        .area()
        .height
        .saturating_sub(2)
        .saturating_sub(FIXED_INTERIOR_LINES as u16) as usize;

    let block = Block::default().title("smstatus").borders(Borders::ALL);
    let mut lines = vec![
        Line::from(format!("smstatus v{}", env!("CARGO_PKG_VERSION"))),
        Line::from(daemon_status_line(app.daemon_status)),
        Line::from(separator_line(app)),
        Line::from(modules_header_line(app, viewport_height)),
    ];
    for line in visible_module_lines(app, viewport_height) {
        lines.push(Line::from(line));
    }
    lines.push(Line::from(hint_line(&app.mode)));
    for line in action_log_lines(&app.action_log) {
        lines.push(Line::from(line));
    }
    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(block);

    frame.render_widget(paragraph, frame.area());
}

fn separator_line(app: &App) -> String {
    match &app.mode {
        Mode::EditingSeparator(buffer) => format!("New separator: {buffer:?}_"),
        Mode::Normal => match &app.separator {
            Some(sep) => format!("separator: {sep:?}"),
            None => "separator: unknown".to_string(),
        },
    }
}

fn hint_line(mode: &Mode) -> &'static str {
    match mode {
        Mode::Normal => {
            "Press q to quit, s to start, k to stop, e to edit separator, \u{2191}/\u{2193} to scroll modules"
        }
        Mode::EditingSeparator(_) => "Editing separator - Enter to save, Esc to cancel",
    }
}

fn module_window(total: usize, offset: usize, viewport_height: usize) -> (usize, usize, usize) {
    if total == 0 {
        return (0, 0, 0);
    }
    let offset = offset.min(total - 1);
    let end = (offset + viewport_height).min(total);
    (offset, end, total)
}

fn modules_header_line(app: &App, viewport_height: usize) -> String {
    match &app.modules {
        None => "modules: unknown".to_string(),
        Some(modules) if modules.is_empty() => "modules: (none configured)".to_string(),
        Some(modules) => {
            let (start, end, total) =
                module_window(modules.len(), app.module_scroll_offset, viewport_height);
            if viewport_height == 0 {
                format!("modules: {total} configured")
            } else {
                format!("modules: {}-{end} of {total}", start + 1)
            }
        }
    }
}

fn visible_module_lines(app: &App, viewport_height: usize) -> Vec<String> {
    let Some(modules) = &app.modules else {
        return Vec::new();
    };
    let (start, end, _total) =
        module_window(modules.len(), app.module_scroll_offset, viewport_height);
    modules[start..end].to_vec()
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
        let chars: Vec<char> = content.chars().collect();
        let content_width = chars.len();
        if content_width <= width {
            let left = width / 2 - content_width / 2;
            let right = width - content_width - left;
            format!("{}{content}{}", " ".repeat(left), " ".repeat(right))
        } else {
            chars[..width].iter().collect()
        }
    }

    fn render(app: &App) -> Buffer {
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn expected_for(status_text: &str, separator_text: &str, action_log: &[&str]) -> Buffer {
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered(status_text, 38));
        let separator_line = format!("│{}│", centered(separator_text, 38));
        let modules_line = format!("│{}│", centered("modules: unknown", 38));
        let hint_line = format!(
            "│{}│",
            centered(
                "Press q to quit, s to start, k to stop, e to edit separator, \u{2191}/\u{2193} to scroll modules",
                38
            )
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
            separator_line,
            modules_line,
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
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: stopped", "separator: unknown", &[])
        );
    }

    #[test]
    fn draw_renders_running_status() {
        let app = App {
            daemon_status: Some(DaemonStatus::Running { pid: 12345 }),
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for(
                "smstatus daemon: running (pid 12345)",
                "separator: unknown",
                &[]
            )
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
            expected_for(
                "smstatus daemon: running (pid unknown)",
                "separator: unknown",
                &[]
            )
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
            expected_for("smstatus daemon: status unknown", "separator: unknown", &[])
        );
    }

    #[test]
    fn draw_renders_empty_action_log_as_blank_rows() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            action_log: vec![],
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: stopped", "separator: unknown", &[])
        );
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
                "separator: unknown",
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
                "separator: unknown",
                &[
                    "Starting smstatus...",
                    "smstatus is already running",
                    "Sent stop signal to smstatus (pid 42)",
                ]
            )
        );
    }

    #[test]
    fn draw_renders_separator_value() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            separator: Some(" | ".to_string()),
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: stopped", "separator: \" | \"", &[])
        );
    }

    #[test]
    fn draw_renders_empty_separator_as_quoted_empty_string() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            separator: Some(String::new()),
            ..App::default()
        };
        assert_eq!(
            render(&app),
            expected_for("smstatus daemon: stopped", "separator: \"\"", &[])
        );
    }

    #[test]
    fn draw_renders_editing_prompt_with_buffer_contents() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator("::".to_string()),
            ..App::default()
        };
        let expected_separator = "New separator: \"::\"_";
        let expected_hint = "Editing separator - Enter to save, Esc to cancel";
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered("smstatus daemon: stopped", 38));
        let separator_line = format!("│{}│", centered(expected_separator, 38));
        let modules_line = format!("│{}│", centered("modules: unknown", 38));
        let hint_line = format!("│{}│", centered(expected_hint, 38));
        let blank = format!("│{}│", centered("", 38));
        let expected = Buffer::with_lines([
            "┌smstatus──────────────────────────────┐".to_string(),
            version_line,
            status_line,
            separator_line,
            modules_line,
            hint_line,
            blank.clone(),
            blank.clone(),
            blank,
            "└──────────────────────────────────────┘".to_string(),
        ]);
        assert_eq!(render(&app), expected);
    }

    #[test]
    fn draw_renders_editing_prompt_with_empty_buffer() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            mode: Mode::EditingSeparator(String::new()),
            ..App::default()
        };
        let expected_separator = "New separator: \"\"_";
        let expected_hint = "Editing separator - Enter to save, Esc to cancel";
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered("smstatus daemon: stopped", 38));
        let separator_line = format!("│{}│", centered(expected_separator, 38));
        let modules_line = format!("│{}│", centered("modules: unknown", 38));
        let hint_line = format!("│{}│", centered(expected_hint, 38));
        let blank = format!("│{}│", centered("", 38));
        let expected = Buffer::with_lines([
            "┌smstatus──────────────────────────────┐".to_string(),
            version_line,
            status_line,
            separator_line,
            modules_line,
            hint_line,
            blank.clone(),
            blank.clone(),
            blank,
            "└──────────────────────────────────────┘".to_string(),
        ]);
        assert_eq!(render(&app), expected);
    }

    fn render_with_height(app: &App, height: u16) -> Buffer {
        let backend = TestBackend::new(40, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn hint_line_normal_mode_includes_scroll_hotkey() {
        assert_eq!(
            hint_line(&Mode::Normal),
            "Press q to quit, s to start, k to stop, e to edit separator, \u{2191}/\u{2193} to scroll modules"
        );
    }

    #[test]
    fn draw_renders_modules_that_fully_fit_in_the_viewport() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "cpu".to_string(),
                "disk#root".to_string(),
                "battery".to_string(),
            ]),
            ..App::default()
        };
        let height = 13;
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered("smstatus daemon: stopped", 38));
        let separator_line = format!("│{}│", centered("separator: unknown", 38));
        let modules_header = format!("│{}│", centered("modules: 1-3 of 3", 38));
        let module_cpu = format!("│{}│", centered("cpu", 38));
        let module_disk = format!("│{}│", centered("disk#root", 38));
        let module_battery = format!("│{}│", centered("battery", 38));
        let hint_line_row = format!(
            "│{}│",
            centered(
                "Press q to quit, s to start, k to stop, e to edit separator, \u{2191}/\u{2193} to scroll modules",
                38
            )
        );
        let blank = format!("│{}│", centered("", 38));
        let expected = Buffer::with_lines([
            "┌smstatus──────────────────────────────┐".to_string(),
            version_line,
            status_line,
            separator_line,
            modules_header,
            module_cpu,
            module_disk,
            module_battery,
            hint_line_row,
            blank.clone(),
            blank.clone(),
            blank,
            "└──────────────────────────────────────┘".to_string(),
        ]);
        assert_eq!(render_with_height(&app, height), expected);
    }

    #[test]
    fn draw_renders_empty_module_list_header_with_zero_rows() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![]),
            ..App::default()
        };
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered("smstatus daemon: stopped", 38));
        let separator_line = format!("│{}│", centered("separator: unknown", 38));
        let modules_header = format!("│{}│", centered("modules: (none configured)", 38));
        let hint_line_row = format!(
            "│{}│",
            centered(
                "Press q to quit, s to start, k to stop, e to edit separator, \u{2191}/\u{2193} to scroll modules",
                38
            )
        );
        let blank = format!("│{}│", centered("", 38));
        let expected = Buffer::with_lines([
            "┌smstatus──────────────────────────────┐".to_string(),
            version_line,
            status_line,
            separator_line,
            modules_header,
            hint_line_row,
            blank.clone(),
            blank.clone(),
            blank,
            "└──────────────────────────────────────┘".to_string(),
        ]);
        assert_eq!(render(&app), expected);
    }

    #[test]
    fn draw_renders_degraded_header_when_viewport_height_is_zero_with_modules_present() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "cpu".to_string(),
                "disk#root".to_string(),
                "battery".to_string(),
            ]),
            ..App::default()
        };
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered("smstatus daemon: stopped", 38));
        let separator_line = format!("│{}│", centered("separator: unknown", 38));
        let modules_header = format!("│{}│", centered("modules: 3 configured", 38));
        let hint_line_row = format!(
            "│{}│",
            centered(
                "Press q to quit, s to start, k to stop, e to edit separator, \u{2191}/\u{2193} to scroll modules",
                38
            )
        );
        let blank = format!("│{}│", centered("", 38));
        let expected = Buffer::with_lines([
            "┌smstatus──────────────────────────────┐".to_string(),
            version_line,
            status_line,
            separator_line,
            modules_header,
            hint_line_row,
            blank.clone(),
            blank.clone(),
            blank,
            "└──────────────────────────────────────┘".to_string(),
        ]);
        assert_eq!(render(&app), expected);
    }

    #[test]
    fn draw_renders_scrolled_slice_of_modules_when_more_than_fit() {
        let app = App {
            daemon_status: Some(DaemonStatus::Stopped),
            modules: Some(vec![
                "m0".to_string(),
                "m1".to_string(),
                "m2".to_string(),
                "m3".to_string(),
                "m4".to_string(),
                "m5".to_string(),
            ]),
            module_scroll_offset: 2,
            ..App::default()
        };
        let height = 12;
        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let status_line = format!("│{}│", centered("smstatus daemon: stopped", 38));
        let separator_line = format!("│{}│", centered("separator: unknown", 38));
        let modules_header = format!("│{}│", centered("modules: 3-4 of 6", 38));
        let module_m2 = format!("│{}│", centered("m2", 38));
        let module_m3 = format!("│{}│", centered("m3", 38));
        let hint_line_row = format!(
            "│{}│",
            centered(
                "Press q to quit, s to start, k to stop, e to edit separator, \u{2191}/\u{2193} to scroll modules",
                38
            )
        );
        let blank = format!("│{}│", centered("", 38));
        let expected = Buffer::with_lines([
            "┌smstatus──────────────────────────────┐".to_string(),
            version_line,
            status_line,
            separator_line,
            modules_header,
            module_m2,
            module_m3,
            hint_line_row,
            blank.clone(),
            blank.clone(),
            blank,
            "└──────────────────────────────────────┘".to_string(),
        ]);
        assert_eq!(render_with_height(&app, height), expected);
    }
}
