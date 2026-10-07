//! `tidal-player` binary: CLI parsing and terminal wiring. Logic lives in the
//! library (see docs/specs/0001-architecture.md).

use std::io::{self, Stdout};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tidal_player::{
    input::key_to_action,
    login::{LoginOutcome, run_login},
    panic_hook::install_panic_hook,
    store_setup::StorePlan,
    ui::render,
};
use tidal_player_api::auth::{
    AuthConfig, AuthStatus, Authenticator, SessionStore, StoreError, SystemClock,
};
use tidal_player_core::protocol::Event as PlayerEvent;
use tidal_player_core::ui::{Action, Effect, State, update};
use tokio::sync::watch;

/// Terminal Tidal player.
#[derive(Debug, Parser)]
#[command(name = "tidal-player", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Log in to Tidal with the device flow.
    Login,
    /// Delete the stored session from this machine.
    Logout,
    /// Run headless (not implemented yet, spec 0005).
    Daemon,
    /// Play one track in the foreground, headless, and exit when it ends.
    Play(PlayArgs),
    /// List the playback devices; `*` marks the one `play` would use.
    Devices,
}

#[derive(Debug, clap::Args)]
struct PlayArgs {
    /// The Tidal track ID.
    track_id: u64,
    /// Highest quality to ask for: hi-res, lossless or high
    /// [env: TIDAL_PLAYER_QUALITY] [default: hi-res].
    #[arg(long)]
    quality: Option<String>,
    /// Output device: any ALSA PCM name, see "tidal-player devices"
    /// [env: TIDAL_PLAYER_DEVICE] [default: default].
    #[arg(long)]
    device: Option<String>,
    /// Start this many seconds into the track.
    #[arg(long, value_name = "SECONDS")]
    start: Option<f64>,
}

/// Leaves the alternate screen and raw mode; errors are ignored because this
/// also runs from the panic hook, where there is nothing better to do.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

/// Maps the status the authenticator reports to the player event the TUI shows.
fn status_action(status: AuthStatus) -> Action {
    Action::Player(match status {
        AuthStatus::Active => PlayerEvent::LoginRestored,
        AuthStatus::LoginRequired => PlayerEvent::LoginRequired,
    })
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut auth_status: Option<watch::Receiver<AuthStatus>>,
) -> Result<()> {
    let mut state = State::default();
    loop {
        terminal.draw(|frame| render(&state, frame))?;
        let mut actions = Vec::new();
        if let Some(rx) = auth_status.as_mut()
            && rx.has_changed().unwrap_or(false)
        {
            actions.push(status_action(*rx.borrow_and_update()));
        }
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                actions.extend(key_to_action(key));
            }
        } else {
            actions.push(Action::Tick);
        }
        for action in actions {
            if update(&mut state, action).contains(&Effect::Quit) {
                return Ok(());
            }
        }
    }
}

/// Reports a store failure the way the spec words it; returns exit code 1.
fn report_store_error(e: &StoreError) -> ExitCode {
    match e {
        StoreError::Unavailable(msg) => eprintln!("{msg}"),
        StoreError::WrongPassphrase => eprintln!("Wrong passphrase for the session file"),
        StoreError::Other(msg) => eprintln!("{msg}"),
    }
    ExitCode::from(1)
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new().context("cannot start the async runtime")
}

fn login(store: &dyn SessionStore) -> Result<LoginOutcome> {
    Ok(runtime()?.block_on(run_login(store)))
}

fn logout(plan: &StorePlan) -> ExitCode {
    // Decided without a passphrase; `delete` never needs one either.
    let had_session = plan.has_stored_session();
    if let Err(e) = plan.build_store().delete() {
        return report_store_error(&e);
    }
    println!(
        "{}",
        if had_session {
            "Logged out"
        } else {
            "Not logged in"
        }
    );
    ExitCode::SUCCESS
}

fn daemon(store: &dyn SessionStore) -> ExitCode {
    match store.load() {
        Ok(Some(_)) => {
            eprintln!("daemon mode is not implemented yet (spec 0005)");
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("Not logged in: run \"tidal-player login\"");
            ExitCode::from(1)
        }
        Err(e) => report_store_error(&e),
    }
}

fn standalone(store: Arc<dyn SessionStore>) -> Result<ExitCode> {
    match store.load() {
        Ok(Some(_)) => {}
        Ok(None) => {
            println!("Not logged in.");
            match login(store.as_ref())? {
                LoginOutcome::LoggedIn => {}
                other => return Ok(ExitCode::from(other.exit_code())),
            }
        }
        Err(e) => return Ok(report_store_error(&e)),
    }
    let auth = match Authenticator::from_store(
        AuthConfig::production(),
        Arc::clone(&store),
        Arc::new(SystemClock),
    ) {
        Ok(Some(auth)) => auth,
        Ok(None) => {
            eprintln!("Not logged in: run \"tidal-player login\"");
            return Ok(ExitCode::from(1));
        }
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(1));
        }
    };

    enable_raw_mode().context("cannot enable raw mode (is stdout a terminal?)")?;
    install_panic_hook(restore_terminal);
    let setup = execute!(io::stdout(), EnterAlternateScreen)
        .context("cannot enter the alternate screen")
        .and_then(|()| Terminal::new(CrosstermBackend::new(io::stdout())).map_err(Into::into));
    let result = setup.and_then(|mut terminal| run(&mut terminal, Some(auth.status())));
    restore_terminal();
    result.map(|()| ExitCode::SUCCESS)
}

fn play(_plan: &StorePlan, _args: &PlayArgs) -> ExitCode {
    ExitCode::SUCCESS
}

fn devices() -> ExitCode {
    ExitCode::SUCCESS
}

fn main() -> Result<ExitCode> {
    let Cli { command } = Cli::parse();
    let plan = StorePlan::from_env();
    match command {
        Some(Command::Logout) => Ok(logout(&plan)),
        Some(Command::Daemon) => Ok(daemon(plan.build_store().as_ref())),
        Some(Command::Login) => {
            let outcome = login(plan.build_store().as_ref())?;
            Ok(ExitCode::from(outcome.exit_code()))
        }
        Some(Command::Play(args)) => Ok(play(&plan, &args)),
        Some(Command::Devices) => Ok(devices()),
        None => standalone(plan.build_store()),
    }
}
