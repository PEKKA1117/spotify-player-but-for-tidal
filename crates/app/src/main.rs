//! `tidal-player` binary: CLI parsing and terminal wiring. Logic lives in the
//! library (see docs/specs/0001-architecture.md).

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tidal_player::{
    input::key_to_action,
    ipc::{self, ClaimError, lock::PlayerLock, paths::current_uid, server},
    login::{LoginOutcome, run_login},
    panic_hook::install_panic_hook,
    play::{
        ASOUND_DIR_VAR, PlayOptions, configured_device, resolve_play_config, resolve_player_config,
        resolve_settings,
    },
    playback::{
        HAS_ALSA, HttpOpener, NO_ALSA, PlayRequest, PlayerSocket, play_items, spawn_output,
    },
    player_runtime::{
        Expander, PlayerRuntime, RuntimeHandle, TokioJobs, expand_items, parse_items,
        spawn_runtime, startup_commands, time_seed,
    },
    store_setup::StorePlan,
    ui::render,
};
use tidal_player_api::auth::{
    AuthConfig, AuthStatus, Authenticator, SessionStore, StoreError, SystemClock,
};
use tidal_player_api::metadata::MetadataClient;
use tidal_player_audio::devices::{format_devices, parse_devices};
use tidal_player_core::Item;
use tidal_player_core::protocol::{Event as PlayerEvent, InsertAt, RepeatMode};
use tidal_player_core::ui::{self as tui_model, Action, Effect, State, update};
use tokio::sync::watch;

/// Terminal Tidal player.
#[derive(Debug, Parser)]
#[command(
    name = "tidal-player",
    version,
    about,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Tidal track IDs, or track, album or playlist links, to load into the
    /// queue (the first one plays).
    #[arg(value_name = "ITEM")]
    items: Vec<String>,
    /// Add the items at the end of the queue instead of replacing it.
    #[arg(long, conflicts_with = "play_next", requires = "items")]
    add_to_queue: bool,
    /// Add the items right after the current entry instead of replacing
    /// the queue.
    #[arg(long, requires = "items")]
    play_next: bool,
}

/// Where `--add-to-queue` / `--play-next` add the items; `None` replaces
/// the queue (spec 0004 "Commands").
fn queue_mode(cli: &Cli) -> Option<InsertAt> {
    if cli.add_to_queue {
        Some(InsertAt::End)
    } else if cli.play_next {
        Some(InsertAt::Next)
    } else {
        None
    }
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Log in to Tidal with the device flow.
    Login,
    /// Delete the stored session from this machine.
    Logout,
    /// Run the player headless, for clients to attach to.
    Daemon,
    /// Play tracks, albums or playlists in the foreground, headless, as one
    /// queue, and exit when it ends.
    Play(PlayArgs),
    /// List the playback devices; `*` marks the one `play` would use.
    Devices,
}

#[derive(Debug, clap::Args)]
struct PlayArgs {
    /// Tidal track IDs, or track, album or playlist links.
    #[arg(value_name = "ITEM", required = true)]
    items: Vec<String>,
    /// Shuffle the queue (the first item still plays first).
    #[arg(long)]
    shuffle: bool,
    /// Repeat mode.
    #[arg(long, value_enum, default_value_t = RepeatArg::Off)]
    repeat: RepeatArg,
    /// Keep going with Tidal's suggestions when the queue runs out
    /// [env: TIDAL_PLAYER_AUTOPLAY=on].
    #[arg(long)]
    autoplay: bool,
    /// Highest quality to ask for: hi-res, lossless or high
    /// [env: TIDAL_PLAYER_QUALITY] [default: hi-res].
    #[arg(long)]
    quality: Option<String>,
    /// Output device: any ALSA PCM name, see "tidal-player devices"
    /// [env: TIDAL_PLAYER_DEVICE] [default: default].
    #[arg(long)]
    device: Option<String>,
    /// Start this many seconds into the first track.
    #[arg(long, value_name = "SECONDS")]
    start: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum RepeatArg {
    Off,
    Queue,
    Track,
}

impl From<RepeatArg> for RepeatMode {
    fn from(repeat: RepeatArg) -> Self {
        match repeat {
            RepeatArg::Off => Self::Off,
            RepeatArg::Queue => Self::Queue,
            RepeatArg::Track => Self::Track,
        }
    }
}

/// Leaves bracketed paste, the alternate screen and raw mode; errors are
/// ignored because this also runs from the panic hook, where there is
/// nothing better to do.
fn restore_terminal() {
    let _ = execute!(io::stdout(), DisableBracketedPaste);
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

/// What the TUI loop talks to: the player, and the expansions of the open
/// prompt (whose answers arrive on `expanded`).
struct Client<'a> {
    player: &'a RuntimeHandle,
    expander: Expander,
    expanded: std::sync::mpsc::Receiver<Action>,
}

impl Client<'_> {
    /// Runs `effects`; `true` when one of them quits.
    fn execute(&self, effects: Vec<Effect>) -> bool {
        for effect in effects {
            match effect {
                Effect::Quit => return true,
                Effect::Send(command) => self.player.send(command),
                Effect::Expand { item, at } => self.expander.expand(item, at),
            }
        }
        false
    }
}

/// How long the loop waits for a key before it looks at the player's
/// events again (and redraws).
const FRAME: Duration = Duration::from_millis(100);

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut auth_status: Option<watch::Receiver<AuthStatus>>,
    client: &Client<'_>,
    mut state: State,
    startup: Vec<Action>,
) -> Result<()> {
    let mut actions = startup;
    loop {
        for action in actions.drain(..) {
            if client.execute(update(&mut state, action)) {
                return Ok(());
            }
        }
        terminal.draw(|frame| render(&state, frame))?;
        if let Some(rx) = auth_status.as_mut()
            && rx.has_changed().unwrap_or(false)
        {
            actions.push(status_action(*rx.borrow_and_update()));
        }
        actions.extend(player_actions(client));
        if event::poll(FRAME)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    actions.extend(key_to_action(key));
                }
                Event::Paste(text) => actions.push(Action::Paste(text)),
                _ => {}
            }
        } else {
            actions.push(Action::Tick);
        }
    }
}

/// The player's events and the finished expansions, in arrival order per
/// source.
fn player_actions(client: &Client<'_>) -> Vec<Action> {
    let mut actions: Vec<Action> = client
        .player
        .events()
        .try_iter()
        .map(Action::Player)
        .collect();
    actions.extend(client.expanded.try_iter());
    actions
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

/// Becomes this user's player (spec 0005 "Transport"): the lock in the
/// runtime directory. `hint` follows "Another player is running (pid N)".
fn claim_player(hint: &str) -> Result<PlayerLock, ExitCode> {
    let uid = current_uid().map_err(|e| {
        eprintln!("Cannot tell this user's ID: {e}");
        ExitCode::from(1)
    })?;
    ipc::claim(env_var, uid).map_err(|e| {
        match &e {
            ClaimError::Lock(ipc::lock::LockError::Held { .. }) => eprintln!("{e}{hint}"),
            _ => eprintln!("{e}"),
        }
        ExitCode::from(e.exit_code())
    })
}

/// Binds the player's socket (after the lock).
fn bind_player(lock: &PlayerLock) -> Result<(std::os::unix::net::UnixListener, PathBuf), ExitCode> {
    ipc::bind(lock).map_err(|e| {
        eprintln!("{e}");
        ExitCode::from(e.exit_code())
    })
}

/// `tidal-player daemon` (spec 0005 "The daemon"): the lock, the session,
/// the settings, the socket; then the player serves its clients until one
/// asks it to shut down.
fn daemon(plan: &StorePlan) -> Result<ExitCode> {
    let lock = match claim_player("") {
        Ok(lock) => lock,
        Err(code) => return Ok(code),
    };
    let store = plan.build_store();
    let session = match store.load() {
        Ok(Some(session)) => session,
        Ok(None) => {
            eprintln!("Not logged in: run \"tidal-player login\"");
            return Ok(ExitCode::from(1));
        }
        Err(e) => return Ok(report_store_error(&e)),
    };
    let settings = match resolve_settings(None, None, env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };
    let player_settings = match resolve_player_config(env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };
    let auth = match Authenticator::new(api_config(), store, session, Arc::new(SystemClock)) {
        Ok(auth) => Arc::new(auth),
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(1));
        }
    };
    let (listener, socket) = match bind_player(&lock) {
        Ok(bound) => bound,
        Err(code) => return Ok(code),
    };

    let runtime = runtime()?;
    let metadata = Arc::new(MetadataClient::new(Arc::clone(&auth)));
    let (_, country) = runtime.block_on(auth.account());
    let opener = match HttpOpener::new(Arc::clone(&auth), settings.quality, country.clone()) {
        Ok(opener) => Arc::new(opener),
        Err(message) => {
            eprintln!("{message}");
            return Ok(ExitCode::from(1));
        }
    };
    let (results, inputs) = std::sync::mpsc::channel();
    let jobs = TokioJobs::new(runtime.handle().clone(), opener, metadata, results.clone());
    let mut config = player_settings.player;
    config.country = Some(country);
    // The engine opens the device only once something plays (0003).
    let player = spawn_runtime(
        PlayerRuntime::new(
            config,
            time_seed(),
            spawn_output(&settings.device, player_settings.release_paused),
            jobs,
        ),
        inputs,
        results,
    );
    server::forward_login(auth.status(), player.inputs(), runtime.handle());
    let server = server::serve(listener, socket, player.inputs());
    eprintln!("Listening on {}", server.path().display());
    // The player's own events go to its clients; this loop only waits for
    // the player thread to end (a client's `Shutdown`).
    for _ in player.events().iter() {}
    drop(server);
    drop(lock);
    Ok(ExitCode::SUCCESS)
}

fn standalone(
    store: Arc<dyn SessionStore>,
    args: &[String],
    mode: Option<InsertAt>,
) -> Result<ExitCode> {
    // Refused before anything starts (spec 0004 "Filling the queue").
    let items = match parse_items(args) {
        Ok(items) => items,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(e.exit_code()));
        }
    };
    let settings = match resolve_settings(None, None, env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };
    let player_settings = match resolve_player_config(env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };
    // Every player serves the socket (spec 0005 "Roles"). Attaching to a
    // running player instead comes with the TUI client.
    let lock = match claim_player("") {
        Ok(lock) => lock,
        Err(code) => return Ok(code),
    };
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
    let auth =
        match Authenticator::from_store(api_config(), Arc::clone(&store), Arc::new(SystemClock)) {
            Ok(Some(auth)) => Arc::new(auth),
            Ok(None) => {
                eprintln!("Not logged in: run \"tidal-player login\"");
                return Ok(ExitCode::from(1));
            }
            Err(e) => {
                eprintln!("{e}");
                return Ok(ExitCode::from(1));
            }
        };

    let runtime = runtime()?;
    let metadata = Arc::new(MetadataClient::new(Arc::clone(&auth)));
    let (country, expanded) = runtime.block_on(async {
        let (_, country) = auth.account().await;
        (country, expand_items(metadata.as_ref(), &items).await)
    });
    // A failed expansion leaves the queue empty and shows why in the
    // playback window (spec 0004 "Edge cases").
    let (tracks, startup_message) = match expanded {
        Ok(tracks) => (tracks, None),
        Err(e) => (Vec::new(), Some(e.to_string())),
    };
    let opener = match HttpOpener::new(Arc::clone(&auth), settings.quality, country.clone()) {
        Ok(opener) => Arc::new(opener),
        Err(message) => {
            eprintln!("{message}");
            return Ok(ExitCode::from(1));
        }
    };
    let (results, inputs) = std::sync::mpsc::channel();
    let (expanded_tx, expanded) = std::sync::mpsc::channel();
    let expander = Expander::new(
        runtime.handle().clone(),
        Arc::clone(&metadata) as _,
        expanded_tx,
    );
    let jobs = TokioJobs::new(runtime.handle().clone(), opener, metadata, results.clone());
    let mut config = player_settings.player;
    config.country = Some(country);
    let (listener, socket) = match bind_player(&lock) {
        Ok(bound) => bound,
        Err(code) => return Ok(code),
    };
    let player = spawn_runtime(
        PlayerRuntime::new(
            config,
            time_seed(),
            spawn_output(&settings.device, player_settings.release_paused),
            jobs,
        ),
        inputs,
        results,
    );
    server::forward_login(auth.status(), player.inputs(), runtime.handle());
    let server = server::serve(listener, socket, player.inputs());
    // The plain form replaces the queue; `--add-to-queue`/`--play-next`
    // add to it the way the open prompt does, which starts the first added
    // track on the (until 0005, always empty) startup queue.
    let startup = match mode {
        None => {
            for command in startup_commands(tracks) {
                player.send(command);
            }
            Vec::new()
        }
        Some(_) if tracks.is_empty() => Vec::new(),
        Some(at) => vec![Action::Expanded {
            at,
            result: Ok(tracks),
        }],
    };
    let mut state = State::new(tui_model::Steps {
        volume: player_settings.steps.volume,
        seek: player_settings.steps.seek,
    });
    state.message = startup_message;
    let client = Client {
        player: &player,
        expander,
        expanded,
    };

    let result = tui(&client, auth.status(), state, startup);
    // Quit: the player stops and releases the device first.
    player.shutdown();
    drop(server);
    drop(lock);
    restore_terminal();
    result.map(|()| ExitCode::SUCCESS)
}

/// Runs the TUI until the user quits; the terminal is left for the caller
/// to restore.
fn tui(
    client: &Client<'_>,
    auth_status: watch::Receiver<AuthStatus>,
    state: State,
    startup: Vec<Action>,
) -> Result<()> {
    enable_raw_mode().context("cannot enable raw mode (is stdout a terminal?)")?;
    install_panic_hook(restore_terminal);
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)
        .context("cannot enter the alternate screen")?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    run(&mut terminal, Some(auth_status), client, state, startup)
}

fn env_var(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Test-only: in debug builds, the API base URL comes from this variable
/// when set, so CLI tests reach a mock server. Release builds ignore it
/// (it would send the bearer token elsewhere).
const API_BASE_VAR: &str = "TIDAL_PLAYER_API_BASE";

fn api_config() -> AuthConfig {
    let mut config = AuthConfig::production();
    if cfg!(debug_assertions)
        && let Some(base) = env_var(API_BASE_VAR).filter(|b| !b.is_empty())
    {
        config.api_base = base;
    }
    config
}

fn play(plan: &StorePlan, args: &PlayArgs) -> ExitCode {
    let settings = match resolve_settings(args.quality.as_deref(), args.device.as_deref(), env_var)
    {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let start_at = match args.start.map(Duration::try_from_secs_f64) {
        None => Duration::ZERO,
        Some(Ok(start)) => start,
        Some(Err(_)) => {
            eprintln!("invalid --start: expected a number of seconds, 0 or more");
            return ExitCode::from(2);
        }
    };
    let items: Vec<Item> = match parse_items(&args.items) {
        Ok(items) => items,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(e.exit_code());
        }
    };
    let player = match resolve_play_config(args.autoplay, env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if !HAS_ALSA {
        eprintln!("{NO_ALSA}");
        return ExitCode::from(1);
    }
    let lock = match claim_player(": use \"tidal-player playback load\"") {
        Ok(lock) => lock,
        Err(code) => return code,
    };
    let store = plan.build_store();
    let session = match store.load() {
        Ok(Some(session)) => session,
        Ok(None) => {
            eprintln!("Not logged in: run \"tidal-player login\"");
            return ExitCode::from(1);
        }
        Err(e) => return report_store_error(&e),
    };
    let auth = match Authenticator::new(api_config(), store, session, Arc::new(SystemClock)) {
        Ok(auth) => Arc::new(auth),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let (listener, socket) = match bind_player(&lock) {
        Ok(bound) => bound,
        Err(code) => return code,
    };
    play_items(
        auth,
        PlayerSocket {
            lock,
            listener,
            path: socket,
        },
        PlayRequest {
            items,
            settings,
            player: player.player,
            release_paused: player.release_paused,
            options: PlayOptions {
                shuffle: args.shuffle,
                repeat: args.repeat.into(),
                autoplay: args.autoplay,
                start_at,
            },
        },
    )
}

/// `tidal-player devices`: `/proc/asound` (or `TIDAL_PLAYER_ASOUND_DIR`).
fn devices() -> ExitCode {
    let dir = env_var(ASOUND_DIR_VAR)
        .filter(|d| !d.is_empty())
        .map_or_else(|| PathBuf::from("/proc/asound"), PathBuf::from);
    // A missing file means no card (no ALSA, or a container): `default` only.
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    let listing = parse_devices(&read("cards"), &read("pcm"));
    print!("{}", format_devices(&listing, &configured_device(env_var)));
    ExitCode::SUCCESS
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let mode = queue_mode(&cli);
    let Cli { command, items, .. } = cli;
    let plan = StorePlan::from_env();
    match command {
        Some(Command::Logout) => Ok(logout(&plan)),
        Some(Command::Daemon) => daemon(&plan),
        Some(Command::Login) => {
            let outcome = login(plan.build_store().as_ref())?;
            Ok(ExitCode::from(outcome.exit_code()))
        }
        Some(Command::Play(args)) => Ok(play(&plan, &args)),
        Some(Command::Devices) => Ok(devices()),
        None => standalone(plan.build_store(), &items, mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC28: `--add-to-queue` and `--play-next` parse into the matching
    /// `at`, are mutually exclusive and need an item.
    #[test]
    fn ac28_queue_flags_parse() {
        let parse = |args: &[&str]| {
            Cli::try_parse_from(std::iter::once("tidal-player").chain(args.iter().copied()))
        };
        let rows: [(&[&str], Option<InsertAt>); 5] = [
            (&["123"], None),
            (&[], None),
            (&["--add-to-queue", "123"], Some(InsertAt::End)),
            (&["--play-next", "1", "2"], Some(InsertAt::Next)),
            (&["123", "--play-next"], Some(InsertAt::Next)),
        ];
        for (args, at) in rows {
            let cli = parse(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            assert_eq!(queue_mode(&cli), at, "{args:?}");
        }
        let refused: [&[&str]; 4] = [
            &["--add-to-queue", "--play-next", "1"],
            &["--play-next", "--add-to-queue", "1"],
            &["--add-to-queue"],
            &["--play-next"],
        ];
        for args in refused {
            assert!(parse(args).is_err(), "{args:?} accepted");
        }
    }
}
