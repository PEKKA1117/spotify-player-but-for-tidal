//! `tidal-player` binary: CLI parsing and terminal wiring. Logic lives in the
//! library (see docs/specs/0001-architecture.md).

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tidal_player::{input::key_to_action, panic_hook::install_panic_hook, ui::render};
use tidal_player_core::ui::{Action, Effect, State, update};

/// Terminal Tidal player.
#[derive(Debug, Parser)]
#[command(name = "tidal-player", version, about)]
struct Cli {}

/// Leaves the alternate screen and raw mode; errors are ignored because this
/// also runs from the panic hook, where there is nothing better to do.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut state = State::default();
    loop {
        terminal.draw(|frame| render(&state, frame))?;
        let action = if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => key_to_action(key),
                _ => None,
            }
        } else {
            Some(Action::Tick)
        };
        if let Some(action) = action {
            if update(&mut state, action).contains(&Effect::Quit) {
                return Ok(());
            }
        }
    }
}

fn main() -> Result<()> {
    let Cli {} = Cli::parse();

    enable_raw_mode().context("cannot enable raw mode (is stdout a terminal?)")?;
    install_panic_hook(restore_terminal);
    let setup = execute!(io::stdout(), EnterAlternateScreen)
        .context("cannot enter the alternate screen")
        .and_then(|()| Terminal::new(CrosstermBackend::new(io::stdout())).map_err(Into::into));
    let result = setup.and_then(|mut terminal| run(&mut terminal));
    restore_terminal();
    result
}
