use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::App;

pub(super) fn draw(frame: &mut Frame, _app: &App) {
    let block = Block::default().title("smstatus").borders(Borders::ALL);
    let paragraph = Paragraph::new(vec![
        Line::from(format!("smstatus v{}", env!("CARGO_PKG_VERSION"))),
        Line::from("Press q to quit"),
    ])
    .alignment(Alignment::Center)
    .block(block);

    frame.render_widget(paragraph, frame.area());
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::tui::app::App;

    fn centered(content: &str, width: usize) -> String {
        let content_width = content.chars().count();
        let left = width / 2 - content_width / 2;
        let right = width - content_width - left;
        format!("{}{content}{}", " ".repeat(left), " ".repeat(right))
    }

    #[test]
    fn draw_renders_bordered_centered_version_and_quit_hint() {
        let backend = TestBackend::new(40, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &App::default())).unwrap();

        let version_line = format!(
            "│{}│",
            centered(&format!("smstatus v{}", env!("CARGO_PKG_VERSION")), 38)
        );
        let expected = Buffer::with_lines([
            "┌smstatus──────────────────────────────┐",
            &version_line,
            "│            Press q to quit           │",
            "│                                      │",
            "│                                      │",
            "└──────────────────────────────────────┘",
        ]);

        assert_eq!(terminal.backend().buffer(), &expected);
    }
}
