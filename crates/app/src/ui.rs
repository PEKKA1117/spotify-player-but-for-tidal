//! Rendering of the pure UI model.

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use tidal_player_core::ui::State;

/// Draws `state` into `frame`: a bordered block titled with the app name.
pub fn render(state: &State, frame: &mut Frame) {
    frame.render_widget(Block::bordered().title("tidal-player"), frame.area());

    if state.login_required {
        let area = frame.area();
        // The status text goes on the last line inside the border
        // Inner area: starts at (1, 1) and has height area.height - 2
        let status_line_y = area.y + area.height - 2;
        let status_area = Rect {
            x: area.x + 1,
            y: status_line_y,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        let status_text = "Session expired — run \"tidal-player login\" in another terminal";
        frame.render_widget(Paragraph::new(status_text), status_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width))
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ac6_empty_state_80x24() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render(&State::default(), frame))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("tidal-player"), "app name missing:\n{text}");
        insta::assert_snapshot!(text);
    }

    #[test]
    fn ac14_login_required_80x24() {
        let state = State {
            login_required: true,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(&state, frame)).unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("tidal-player login"),
            "status text missing:\n{text}"
        );
        insta::assert_snapshot!(text);
    }

    /// AC17: the status line never panics on a tiny terminal and is only
    /// drawn when there is an inner row for it.
    #[test]
    fn ac17_login_status_tiny_terminals() {
        let state = State {
            login_required: true,
        };
        for (width, height) in [(80, 0), (80, 1), (80, 2), (80, 3), (1, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(&state, frame)).unwrap();
            let text = buffer_text(&terminal);
            let rows: Vec<&str> = text.split('\n').collect();
            if height < 3 {
                assert!(
                    !text.contains("Session expired"),
                    "{width}x{height}: status drawn without an inner row:\n{text}"
                );
            }
            if (width, height) == (80, 3) {
                assert!(
                    rows[1].contains("Session expired"),
                    "{width}x{height}: status not on the inner row:\n{text}"
                );
            }
        }
    }
}
