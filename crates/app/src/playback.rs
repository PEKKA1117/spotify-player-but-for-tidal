//! The I/O half of playback (spec 0003 "Commands", spec 0004 "Commands"):
//! the stream opener (resolve, then an [`HttpSource`]), the ALSA engine with
//! the zbus reservation, and `tidal-player play` around
//! [`crate::play::play_queue`], stopping cleanly on Ctrl-C. Everything it
//! decides is in [`crate::play`] and [`crate::player_runtime`] and tested
//! there; this wiring is checked by hand at acceptance (real account, real
//! device).

use std::collections::VecDeque;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tidal_player_api::auth::{Authenticator, BoxFuture};
use tidal_player_api::stream::{ResolvedStream, StreamPlan, StreamResolver};
use tidal_player_audio::{self as audio, EngineError, SinkError};
use tidal_player_core::player::{Failure, PlayerConfig};
use tidal_player_core::{AudioQuality, Item, TrackId};

use crate::http_source::{HttpSource, HttpSourceConfig, NETWORK_TIMEOUT, Reresolve};
use crate::play::{PlayOptions, Settings, engine_failure, stream_failure};
use crate::player_runtime::{EngineControl, Prepared, StreamOpener};

/// Exit code after Ctrl-C.
pub const EXIT_INTERRUPTED: u8 = 130;

/// What `play` plays.
#[derive(Debug, Clone)]
pub struct PlayRequest {
    pub items: Vec<Item>,
    pub settings: Settings,
    /// From the environment and `--autoplay`; the country is filled in from
    /// the session.
    pub player: PlayerConfig,
    /// The engine's release delay (spec 0005); `None`: never.
    pub release_paused: Option<Duration>,
    pub options: PlayOptions,
}

/// The player's claim and socket (spec 0005 "Roles": `play` serves the
/// socket too).
#[derive(Debug)]
pub struct PlayerSocket {
    pub lock: crate::ipc::lock::PlayerLock,
    pub listener: std::os::unix::net::UnixListener,
    pub path: std::path::PathBuf,
}

/// The message of a build without the `alsa` feature.
pub const NO_ALSA: &str = "this build has no ALSA output";

/// Whether this build can play.
pub const HAS_ALSA: bool = cfg!(feature = "alsa");

/// Resolves a track and opens it over HTTP, re-resolving an expired URL
/// (spec 0003 "Fetching").
#[derive(Debug, Clone)]
pub struct HttpOpener {
    resolver: StreamResolver,
    client: reqwest::Client,
    quality: AudioQuality,
    country: String,
}

impl HttpOpener {
    /// `Err` is the stderr line.
    pub fn new(
        auth: Arc<Authenticator>,
        quality: AudioQuality,
        country: String,
    ) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .connect_timeout(NETWORK_TIMEOUT)
            .build()
            .map_err(|e| format!("cannot start the HTTP client: {}", e.without_url()))?;
        Ok(Self {
            resolver: StreamResolver::new(auth),
            client,
            quality,
            country,
        })
    }
}

impl StreamOpener for HttpOpener {
    fn open(&self, track: TrackId) -> BoxFuture<'static, Result<Prepared, Failure>> {
        let Self {
            resolver,
            client,
            quality,
            country,
        } = self.clone();
        Box::pin(async move {
            let id = track.0;
            let resolved = resolver
                .resolve_stream(id, quality)
                .await
                .map_err(|e| stream_failure(id, &country, &e))?;
            let reresolve: Reresolve = Arc::new(move || {
                let resolver = resolver.clone();
                Box::pin(async move { resolver.resolve_stream(id, quality).await })
            });
            let config = HttpSourceConfig::for_stream(&resolved);
            let granted = resolved.quality;
            let duration = duration(&resolved);
            let source = HttpSource::open(client, resolved, reresolve, config)
                .await
                .map_err(|e| engine_failure(id, &EngineError::Source(e)))?;
            Ok(Prepared {
                track,
                quality: granted,
                duration,
                source: Box::new(source),
            })
        })
    }
}

/// The track's length, when the plan says (DASH timelines do).
fn duration(resolved: &ResolvedStream) -> Option<Duration> {
    match &resolved.plan {
        StreamPlan::Segmented { segments, .. } => segments.last().map(|s| s.end_time()),
        StreamPlan::Single { .. } => None,
    }
}

/// The output engine on `device`, releasing it after `release_paused`
/// paused (spec 0005): ALSA, or, in a build without it, an engine whose
/// every track fails with [`NO_ALSA`].
#[cfg(feature = "alsa")]
pub fn spawn_output(
    device: &str,
    release_paused: Option<Duration>,
) -> Box<dyn EngineControl + Send> {
    use crate::reserve::ZbusReserver;
    use tidal_player_audio::{Engine, EngineConfig, ReleaseRequests, Reserver, alsa_sink_factory};
    // The reservation object each sink exports asks this engine.
    let requests = ReleaseRequests::new();
    let for_sinks = requests.clone();
    let factory = alsa_sink_factory(move || {
        Box::new(ZbusReserver::new(for_sinks.clone())) as Box<dyn Reserver>
    });
    Box::new(Engine::spawn(
        Box::new(factory),
        EngineConfig::new(device.to_owned())
            .with_release_paused(release_paused)
            .with_release_requests(requests),
    ))
}

/// The output engine of a build without ALSA: every track fails.
#[cfg(not(feature = "alsa"))]
pub fn spawn_output(
    device: &str,
    release_paused: Option<Duration>,
) -> Box<dyn EngineControl + Send> {
    let _ = (device, release_paused);
    Box::new(NoOutput::default())
}

/// The engine of a build without ALSA: every `Play` fails with [`NO_ALSA`].
#[derive(Debug, Default)]
pub struct NoOutput {
    events: VecDeque<audio::Event>,
}

impl EngineControl for NoOutput {
    fn send(&mut self, command: audio::Command) {
        if let audio::Command::Play { tag, .. } = command {
            self.events.push_back(audio::Event::Error {
                tag,
                error: EngineError::Output(SinkError::Backend(NO_ALSA.into())),
            });
        }
    }

    fn poll_event(&mut self, wait: Duration) -> Option<audio::Event> {
        let event = self.events.pop_front();
        if event.is_none() {
            std::thread::sleep(wait);
        }
        event
    }
}

/// Plays `request` as one queue: exit 0 when it ran out after a track
/// played to its end, 130 on Ctrl-C (after the device is closed and
/// released), 1 on errors with the message on stderr.
#[cfg(feature = "alsa")]
pub fn play_items(
    auth: Arc<Authenticator>,
    socket: PlayerSocket,
    request: PlayRequest,
) -> ExitCode {
    alsa_play::play_items(auth, socket, request)
}

/// Without ALSA there is no output to play to.
#[cfg(not(feature = "alsa"))]
pub fn play_items(
    auth: Arc<Authenticator>,
    socket: PlayerSocket,
    request: PlayRequest,
) -> ExitCode {
    let _ = (auth, socket, request);
    eprintln!("{NO_ALSA}");
    ExitCode::from(1)
}

#[cfg(feature = "alsa")]
mod alsa_play {
    use std::io::{self, IsTerminal};
    use std::process::ExitCode;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use tidal_player_api::auth::Authenticator;
    use tidal_player_api::metadata::MetadataClient;
    use tokio::sync::watch;

    use super::{EXIT_INTERRUPTED, HttpOpener, PlayRequest, PlayerSocket, spawn_output};
    use crate::ipc::server;
    use crate::play::{PlayOutput, play_queue, play_tracks};
    use crate::player_runtime::{POLL, PlayerRuntime, TokioJobs, time_seed};

    pub(super) fn play_items(
        auth: Arc<Authenticator>,
        socket: PlayerSocket,
        request: PlayRequest,
    ) -> ExitCode {
        let PlayerSocket {
            lock,
            listener,
            path,
        } = socket;
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(e) => {
                eprintln!("cannot start the async runtime: {e}");
                return ExitCode::from(1);
            }
        };
        let (interrupt_tx, interrupt) = watch::channel(false);
        runtime.spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                let _ = interrupt_tx.send(true);
            }
        });

        let metadata = Arc::new(MetadataClient::new(Arc::clone(&auth)));
        let prepared = runtime.block_on(async {
            let mut interrupted = interrupt.clone();
            let prepare = async {
                let (_, country) = auth.account().await;
                let tracks = play_tracks(metadata.as_ref(), &request.items, &request.options).await;
                (country, tracks)
            };
            tokio::select! {
                prepared = prepare => Some(prepared),
                _ = interrupted.wait_for(|i| *i) => None,
            }
        });
        let (country, tracks) = match prepared {
            None => return ExitCode::from(EXIT_INTERRUPTED),
            Some((_, Err(e))) => {
                eprintln!("{e}");
                return ExitCode::from(e.exit_code());
            }
            Some((country, Ok(tracks))) => (country, tracks),
        };
        let status = auth.status();
        let opener = match HttpOpener::new(auth, request.settings.quality, country.clone()) {
            Ok(opener) => Arc::new(opener),
            Err(message) => {
                eprintln!("{message}");
                return ExitCode::from(1);
            }
        };

        let (results, inputs) = mpsc::channel();
        // Clients reach this player through its input channel, handled by
        // `play_queue` with everything else.
        server::forward_login(status, results.clone(), runtime.handle());
        let server = server::serve(listener, path, results.clone());
        let jobs = TokioJobs::new(runtime.handle().clone(), opener, metadata, results);
        let mut config = request.player.clone();
        config.country = Some(country);
        let engine = spawn_output(&request.settings.device, request.release_paused);
        let mut player = PlayerRuntime::new(config, time_seed(), engine, jobs);
        let stdout = io::stdout();
        let tty = stdout.is_terminal();
        let (mut out, mut err) = (stdout.lock(), io::stderr());
        let mut output = PlayOutput {
            out: &mut out,
            err: &mut err,
            tty,
        };
        let code = play_queue(
            &mut player,
            tracks,
            &request.options,
            &inputs,
            POLL,
            &mut || *interrupt.borrow(),
            &mut output,
        );
        // Dropping the player drops the engine, which closes and releases
        // the device before the process exits.
        drop(player);
        drop(server);
        drop(lock);
        runtime.shutdown_timeout(Duration::from_millis(100));
        ExitCode::from(code)
    }
}
