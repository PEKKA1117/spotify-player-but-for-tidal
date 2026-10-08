//! `tidal-player` binary: CLI parsing and terminal wiring. Logic lives in the
//! library (see docs/specs/0001-architecture.md).

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    event::{self, DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tidal_player::{
    client::{
        self, Connector, FindError, InProcess, Session, SocketConnector, SystemClock, startup_open,
    },
    config::{self, AppConfig},
    daemon::{forward_signals, notify_ready},
    input::event_to_action,
    ipc::{
        self, ClaimError,
        client::{ConnectError, Connection},
        lock::{LockError, PlayerLock},
        paths::current_uid,
        server,
    },
    login::{LoginOutcome, run_login},
    oneshot::PlaybackCommand,
    panic_hook::install_panic_hook,
    play::{
        ASOUND_DIR_VAR, PlayOptions, configured_device_with, resolve_play_config_with,
        resolve_player_config_with, resolve_settings_with,
    },
    playback::{
        HAS_ALSA, HttpOpener, NO_ALSA, PlayRequest, PlayerSocket, play_items, spawn_output,
    },
    player_runtime::{PlayerRuntime, TokioJobs, parse_items, spawn_runtime, time_seed},
    store_setup::StorePlan,
    ui::render,
};
use tidal_player_api::auth::{
    AuthConfig, Authenticator, SessionStore, StoreError, SystemClock as AuthClock,
};
use tidal_player_api::library::LibraryClient;
use tidal_player_api::metadata::MetadataClient;
use tidal_player_audio::devices::{format_devices, parse_devices};
use tidal_player_core::Item;
use tidal_player_core::protocol::{InsertAt, RepeatMode};
use tidal_player_core::ui::{self as tui_model, Action, Effect, Keymap, State, update};

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
    /// Read app.toml and keymap.toml from this directory instead of
    /// $TIDAL_PLAYER_CONFIG_DIR or ~/.config/tidal-player.
    #[arg(short = 'c', long, global = true, value_name = "DIR")]
    config_folder: Option<PathBuf>,
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
    Daemon(DaemonArgs),
    /// Control the running player: one command, then exit.
    Playback {
        #[command(subcommand)]
        command: PlaybackCommand,
    },
    /// Play tracks, albums or playlists in the foreground, headless, as one
    /// queue, and exit when it ends.
    Play(PlayArgs),
    /// List the playback devices; `*` marks the one `play` would use.
    Devices,
}

#[derive(Debug, clap::Args)]
struct DaemonArgs {
    #[command(subcommand)]
    action: Option<DaemonAction>,
}

#[derive(Debug, clap::Subcommand)]
enum DaemonAction {
    /// Ask the running player to shut down, and wait until it is gone.
    Stop,
    /// Print a systemd user unit for the daemon (see docs/daemon.md).
    Unit,
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

/// How long the loop waits for a key before it looks at the player's
/// messages again (and redraws).
const FRAME: Duration = Duration::from_millis(100);

/// The TUI, as a client of a player (spec 0005 "The TUI as a client"):
/// the standalone TUI's own player in-process, or another process's over
/// the socket. Keys become commands; the screen changes with the player's
/// messages only.
fn run<C: Connector>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    session: &mut Session<C>,
    mut state: State,
) -> Result<()> {
    let mut actions = Vec::new();
    // A list window's height follows the terminal (spec 0006 "Lists load
    // as you scroll"): sent before the first frame and whenever the size
    // (or the session-expired line, which takes a row) changes, from the
    // same layout `render` draws.
    let mut list_height = None;
    loop {
        let size = terminal.size()?;
        let height = tidal_player::ui::list_height(
            ratatui::layout::Rect::new(0, 0, size.width, size.height),
            state.login_required,
        );
        if list_height != Some(height) {
            list_height = Some(height);
            actions.push(Action::Resize {
                list_height: height,
            });
        }
        actions.extend(session.poll(Instant::now()));
        for action in actions.drain(..) {
            for effect in update(&mut state, action) {
                match effect {
                    Effect::Quit => return Ok(()),
                    Effect::Send(command) => session.send(command),
                    Effect::Library { id, request } => session.send_library(id, request),
                }
            }
        }
        terminal.draw(|frame| render(&state, frame))?;
        if event::poll(FRAME)? {
            actions.extend(event_to_action(event::read()?));
        } else {
            actions.push(Action::Tick);
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
fn daemon(plan: &StorePlan, app: &AppConfig) -> Result<ExitCode> {
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
    let settings = match resolve_settings_with(app, None, None, env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };
    let player_settings = match resolve_player_config_with(app, env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };
    let auth = match Authenticator::new(api_config(), store, session, Arc::new(AuthClock)) {
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
    let jobs = TokioJobs::new(runtime.handle().clone(), opener, metadata, results.clone())
        .with_library(player_library(&auth), player_settings.library.clone());
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
    // Before READY=1: a SIGTERM from then on is a clean shutdown.
    forward_signals(runtime.handle(), player.inputs()).context("cannot handle signals")?;
    let server = server::serve(listener, socket, player.inputs());
    eprintln!("Listening on {}", server.path().display());
    // The socket was bound before: it accepts connections now.
    if let Err(e) = notify_ready(env_var) {
        eprintln!("Cannot tell systemd the daemon is ready: {e}");
    }
    // Until a `Shutdown` (a client's, or a signal's): every subscriber was
    // sent `ShuttingDown` and the engine is stopped and gone.
    player.wait();
    // The clients get their last messages, then the socket goes.
    drop(server);
    drop(lock);
    runtime.shutdown_timeout(Duration::from_millis(100));
    Ok(ExitCode::SUCCESS)
}

/// Who this TUI is (spec 0005 "Roles"): a client of the running player, or
/// the player itself (holding the lock).
enum Role {
    Client {
        connection: Connection,
        socket: PathBuf,
    },
    Player(PlayerLock),
}

/// Finds the running player, or becomes it. The socket is tried first, so
/// a client never takes (or even probes) the lock; when nothing answers,
/// this process tries to take the lock; a player that holds it but is
/// still starting is tried for 2 s.
fn choose_role() -> Result<Role, ExitCode> {
    let fail = |e: &dyn std::fmt::Display| {
        eprintln!("{e}");
        ExitCode::from(1)
    };
    let (_, socket) = client::locate(env_var).map_err(|e| fail(&e))?;
    match Connection::connect(&socket) {
        Ok(connection) => return Ok(Role::Client { connection, socket }),
        Err(ConnectError::Greeting(e)) => return Err(fail(&e)),
        Err(_) => {}
    }
    let uid = current_uid().map_err(|e| fail(&format!("Cannot tell this user's ID: {e}")))?;
    match ipc::claim(env_var, uid) {
        Ok(lock) => Ok(Role::Player(lock)),
        Err(ClaimError::Lock(LockError::Held { pid })) => {
            let connection = client::retry_connect(
                &mut || SocketConnector::new(socket.clone()).connect(),
                &mut SystemClock,
                pid,
                &socket,
            )
            .map_err(|e: FindError| fail(&e))?;
            Ok(Role::Client { connection, socket })
        }
        Err(e) => {
            eprintln!("{e}");
            Err(ExitCode::from(e.exit_code()))
        }
    }
}

/// The library the player answers clients' requests with (spec 0006),
/// sharing the player's `Authenticator`.
fn player_library(auth: &Arc<Authenticator>) -> Arc<dyn tidal_player::player_runtime::Library> {
    Arc::new(LibraryClient::new(Arc::clone(auth)))
}

/// `tidal-player [ITEM]...`: the TUI, as the player (standalone) or as a
/// client of the running one.
fn tui_main(
    plan: &StorePlan,
    app: &AppConfig,
    keymap: Keymap,
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
    let player_settings = match resolve_player_config_with(app, env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
    };
    let state = configured_tui_state(&player_settings, keymap);
    let open = startup_open(items, mode);
    match choose_role() {
        Ok(Role::Client { connection, socket }) => attached(connection, socket, open, state),
        Ok(Role::Player(lock)) => {
            standalone(lock, plan.build_store(), app, player_settings, open, state)
        }
        Err(code) => Ok(code),
    }
}

/// The TUI's model before it starts, for both roles: the page sizes are
/// resolved here, as an attached client asks the player's API through
/// them with the same environment (spec 0006 "Page size", 0007 "Search
/// page size"), and a standalone one is the player. It starts on the
/// library (spec 0006 "Pages").
fn tui_state(player_settings: &tidal_player::play::PlayerSettings) -> State {
    let mut state = State::new(tui_model::Steps {
        volume: player_settings.steps.volume,
        seek: player_settings.steps.seek,
    });
    state.page_size = player_settings.library.page_size;
    state.search_page_size = player_settings.library.search_page_size;
    tui_model::start_on_library(&mut state);
    state
}

/// The TUI's model from the resolved settings (`app.toml` under the
/// environment) and the client's own `keymap.toml` (spec 0008 AC15): the
/// steps, the page sizes, the library layout and the keymap, whose notice
/// shows once the player has answered.
fn configured_tui_state(
    player_settings: &tidal_player::play::PlayerSettings,
    keymap: Keymap,
) -> State {
    let _ = keymap;
    tui_state(player_settings)
}

/// A TUI client (spec 0005 "The TUI as a client"): no session, no store,
/// no engine; only the connection. `open` goes first; quitting detaches.
fn attached(
    connection: Connection,
    socket: PathBuf,
    open: Option<tidal_player_core::protocol::Command>,
    state: State,
) -> Result<ExitCode> {
    let mut session = Session::new(SocketConnector::new(socket), connection, open);
    let result = tui(&mut session, state);
    restore_terminal();
    result.map(|()| ExitCode::SUCCESS)
}

/// The standalone player with its TUI (0004), holding `lock`: the TUI is
/// a client of it in-process, with the same messages as over the socket.
fn standalone(
    lock: PlayerLock,
    store: Arc<dyn SessionStore>,
    app: &AppConfig,
    player_settings: tidal_player::play::PlayerSettings,
    open: Option<tidal_player_core::protocol::Command>,
    state: State,
) -> Result<ExitCode> {
    let settings = match resolve_settings_with(app, None, None, env_var) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("{e}");
            return Ok(ExitCode::from(2));
        }
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
        match Authenticator::from_store(api_config(), Arc::clone(&store), Arc::new(AuthClock)) {
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
    let (_, country) = runtime.block_on(auth.account());
    let opener = match HttpOpener::new(Arc::clone(&auth), settings.quality, country.clone()) {
        Ok(opener) => Arc::new(opener),
        Err(message) => {
            eprintln!("{message}");
            return Ok(ExitCode::from(1));
        }
    };
    let (results, inputs) = std::sync::mpsc::channel();
    let jobs = TokioJobs::new(runtime.handle().clone(), opener, metadata, results.clone())
        .with_library(player_library(&auth), player_settings.library.clone());
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
    // The login status reaches the TUI as it reaches any client (0002
    // AC14): in the `Welcome`, then as events.
    server::forward_login(auth.status(), player.inputs(), runtime.handle());
    let server = server::serve(listener, socket, player.inputs());
    // The startup items go to the player as an `Open`, like the prompt's;
    // a failed expansion is the reply's message in the playback window.
    let mut connector = InProcess::new(player.inputs());
    let link = connector
        .connect()
        .map_err(|e| anyhow::anyhow!("cannot join the player: {e}"))?;
    let mut session = Session::new(connector, link, open);

    let result = tui(&mut session, state);
    // Quit: the player stops and releases the device first; its other
    // clients are told it shut down.
    player.shutdown();
    drop(server);
    drop(lock);
    restore_terminal();
    result.map(|()| ExitCode::SUCCESS)
}

/// Runs the TUI until the user quits; the terminal is left for the caller
/// to restore.
fn tui<C: Connector>(session: &mut Session<C>, state: State) -> Result<()> {
    enable_raw_mode().context("cannot enable raw mode (is stdout a terminal?)")?;
    install_panic_hook(restore_terminal);
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)
        .context("cannot enter the alternate screen")?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    run(&mut terminal, session, state)
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

fn play(plan: &StorePlan, app: &AppConfig, args: &PlayArgs) -> ExitCode {
    let settings = match resolve_settings_with(
        app,
        args.quality.as_deref(),
        args.device.as_deref(),
        env_var,
    ) {
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
    let player = match resolve_play_config_with(app, args.autoplay, env_var) {
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
    let auth = match Authenticator::new(api_config(), store, session, Arc::new(AuthClock)) {
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
fn devices(app: &AppConfig) -> ExitCode {
    let dir = env_var(ASOUND_DIR_VAR)
        .filter(|d| !d.is_empty())
        .map_or_else(|| PathBuf::from("/proc/asound"), PathBuf::from);
    // A missing file means no card (no ALSA, or a container): `default` only.
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
    let listing = parse_devices(&read("cards"), &read("pcm"));
    print!(
        "{}",
        format_devices(&listing, &configured_device_with(app, env_var))
    );
    ExitCode::SUCCESS
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let mode = queue_mode(&cli);
    let Cli {
        command,
        items,
        config_folder,
        ..
    } = cli;
    let plan = StorePlan::from_env();
    // Every command that starts a player or a TUI validates the whole of
    // app.toml first (spec 0008 AC12): before the lock, the socket and raw
    // mode.
    let reads_config = matches!(
        command,
        None | Some(Command::Daemon(DaemonArgs { action: None }))
            | Some(Command::Play(_))
            | Some(Command::Devices)
    );
    let config_dir = config::process_config_dir(config_folder.as_deref());
    let app = if reads_config {
        match config::load_app_toml(&config_dir) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("{e}");
                return Ok(ExitCode::from(2));
            }
        }
    } else {
        AppConfig::default()
    };
    // keymap.toml too, in every mode that starts a player or a TUI (spec
    // 0008 AC12): a broken file stops the daemon as well, though only a
    // TUI uses the keys. `devices` starts neither and does not read it.
    let keymap = if reads_config && !matches!(command, Some(Command::Devices)) {
        match config::load_keymap_toml(&config_dir) {
            Ok(keymap) => keymap,
            Err(e) => {
                eprintln!("{e}");
                return Ok(ExitCode::from(2));
            }
        }
    } else {
        Keymap::default()
    };
    match command {
        Some(Command::Logout) => Ok(logout(&plan)),
        Some(Command::Daemon(DaemonArgs { action: None })) => daemon(&plan, &app),
        Some(Command::Daemon(DaemonArgs {
            action: Some(DaemonAction::Stop),
        })) => Ok(tidal_player::daemon::stop()),
        Some(Command::Daemon(DaemonArgs {
            action: Some(DaemonAction::Unit),
        })) => {
            let exe = std::env::current_exe().context("cannot tell where this program is")?;
            print!("{}", tidal_player::daemon::unit(&exe));
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Playback { command }) => Ok(tidal_player::oneshot::run(&command)),
        Some(Command::Login) => {
            let outcome = login(plan.build_store().as_ref())?;
            Ok(ExitCode::from(outcome.exit_code()))
        }
        Some(Command::Play(args)) => Ok(play(&plan, &app, &args)),
        Some(Command::Devices) => Ok(devices(&app)),
        None => tui_main(&plan, &app, keymap, &items, mode),
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

    /// 0007 AC13 (wiring): every TUI, attached or standalone, sizes its
    /// requests with both resolved page sizes, and starts on the library.
    #[test]
    fn ac13_tui_state_page_sizes() {
        let mut settings = tidal_player::play::PlayerSettings::default();
        settings.library.page_size = 33;
        settings.library.search_page_size = 7;
        let state = tui_state(&settings);
        assert_eq!(state.page_size, 33);
        assert_eq!(state.search_page_size, 7);
        assert_eq!(state.page().kind, tidal_player_core::ui::PageKind::Library);
    }
    /// 0008 AC15: the TUI's state takes its steps, page sizes, library
    /// layout and keymap from the config directory, the environment over
    /// `app.toml` (table: defaults, a file, a variable over a file).
    #[test]
    fn ac15_tui_state_from_config() {
        use tidal_player_core::ui::keymap::{Binding, UiCommand};
        use tidal_player_core::ui::{Key, LibraryLayout};

        const APP: &str = "volume_step = 10\nseek_duration_secs = 30\npage_size = 50\n\
            search_page_size = 7\n[layout]\nlibrary = { playlist_percent = 30, album_percent = 50 }\n";
        const KEYMAP: &str = "[[keymaps]]\ncommand = \"Mute\"\nkey_sequence = \"n\"\n\n\
            [[keymaps]]\ncommand = \"PlayRandom\"\nkey_sequence = \"x\"\n";
        struct Row {
            name: &'static str,
            files: bool,
            env: &'static [(&'static str, &'static str)],
            volume: u8,
            seek: u64,
            page_size: u32,
            search_page_size: u32,
            layout: (u16, u16),
            n: UiCommand,
            message: Option<&'static str>,
        }
        let rows = [
            Row {
                name: "defaults",
                files: false,
                env: &[],
                volume: 5,
                seek: 5,
                page_size: 100,
                search_page_size: 20,
                layout: (40, 40),
                n: UiCommand::NextTrack,
                message: None,
            },
            Row {
                name: "a file",
                files: true,
                env: &[],
                volume: 10,
                seek: 30,
                page_size: 50,
                search_page_size: 7,
                layout: (30, 50),
                n: UiCommand::Mute,
                message: Some(
                    "keymap.toml: 1 spotify-player command not supported here: PlayRandom",
                ),
            },
            Row {
                name: "a variable over a file",
                files: true,
                env: &[
                    ("TIDAL_PLAYER_VOLUME_STEP", "3"),
                    ("TIDAL_PLAYER_PAGE_SIZE", "9"),
                ],
                volume: 3,
                seek: 30,
                page_size: 9,
                search_page_size: 7,
                layout: (30, 50),
                n: UiCommand::Mute,
                message: Some(
                    "keymap.toml: 1 spotify-player command not supported here: PlayRandom",
                ),
            },
        ];
        for row in rows {
            let dir = tempfile::tempdir().unwrap();
            if row.files {
                std::fs::write(config::app_toml_path(dir.path()), APP).unwrap();
                std::fs::write(config::keymap_toml_path(dir.path()), KEYMAP).unwrap();
            }
            let app = config::load_app_toml(dir.path()).unwrap();
            let keymap = config::load_keymap_toml(dir.path()).unwrap();
            let env = |key: &str| {
                row.env
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_owned())
            };
            let settings = resolve_player_config_with(&app, env).unwrap();
            let state = configured_tui_state(&settings, keymap);
            let name = row.name;
            assert_eq!(state.steps.volume, row.volume, "{name}");
            assert_eq!(state.steps.seek, Duration::from_secs(row.seek), "{name}");
            assert_eq!(state.page_size, row.page_size, "{name}");
            assert_eq!(state.search_page_size, row.search_page_size, "{name}");
            assert_eq!(
                state.library_layout,
                LibraryLayout {
                    playlist_percent: row.layout.0,
                    album_percent: row.layout.1,
                },
                "{name}"
            );
            assert_eq!(
                state.keymap.get(&[Key::Char('n')]),
                Some(Binding::Command(row.n)),
                "{name}"
            );
            assert_eq!(state.message(), row.message, "{name}");
            assert_eq!(state.page().kind, tidal_player_core::ui::PageKind::Library);
        }
    }
}
