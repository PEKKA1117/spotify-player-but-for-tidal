//! The I/O half of `tidal-player play` (spec 0003 "Commands"): resolve the
//! track, open its [`HttpSource`], play it on the engine with the ALSA
//! output and the zbus reservation, print what [`Reporter`] says, and stop
//! cleanly on Ctrl-C. Everything it decides is in [`crate::play`] and
//! tested there; this wiring is checked by hand at acceptance (real
//! account, real device).

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tidal_player_api::auth::Authenticator;

use crate::play::Settings;

/// Exit code after Ctrl-C.
pub const EXIT_INTERRUPTED: u8 = 130;

/// What to play.
#[derive(Debug, Clone)]
pub struct PlayRequest {
    pub track_id: u64,
    pub settings: Settings,
    pub start_at: Duration,
}

/// Plays `request` to its end: exit 0, 130 on Ctrl-C (after the device is
/// closed and released), 1 on errors with one line on stderr.
#[cfg(feature = "alsa")]
pub fn play_track(auth: Arc<Authenticator>, request: PlayRequest) -> ExitCode {
    alsa_play::play_track(auth, request)
}

/// Without ALSA there is no output to play to.
#[cfg(not(feature = "alsa"))]
pub fn play_track(auth: Arc<Authenticator>, request: PlayRequest) -> ExitCode {
    let _ = (auth, request);
    eprintln!("{NO_ALSA}");
    ExitCode::from(1)
}

/// The message of a build without the `alsa` feature.
pub const NO_ALSA: &str = "this build has no ALSA output";

/// Whether this build can play.
pub const HAS_ALSA: bool = cfg!(feature = "alsa");

#[cfg(feature = "alsa")]
mod alsa_play {
    use std::io::{self, IsTerminal, Write};
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use tidal_player_api::auth::Authenticator;
    use tidal_player_api::stream::{ResolvedStream, StreamPlan, StreamResolver};
    use tidal_player_audio::{
        Command, Engine, EngineConfig, EngineError, Reserver, alsa_sink_factory,
    };
    use tokio::sync::watch;

    use super::{EXIT_INTERRUPTED, PlayRequest};
    use crate::http_source::{HttpSource, HttpSourceConfig, NETWORK_TIMEOUT, Reresolve};
    use crate::play::{Outcome, Reporter, engine_error_message, stream_error_message};
    use crate::reserve::ZbusReserver;

    /// How often the event loop looks at Ctrl-C while no event arrives.
    const POLL: Duration = Duration::from_millis(100);

    pub(super) fn play_track(auth: Arc<Authenticator>, request: PlayRequest) -> ExitCode {
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

        let prepared = runtime.block_on(async {
            let mut interrupted = interrupt.clone();
            tokio::select! {
                prepared = prepare(auth, &request) => Some(prepared),
                _ = interrupted.wait_for(|i| *i) => None,
            }
        });
        let (resolved, source) = match prepared {
            None => return ExitCode::from(EXIT_INTERRUPTED),
            Some(Err(message)) => {
                eprintln!("{message}");
                return ExitCode::from(1);
            }
            Some(Ok(prepared)) => prepared,
        };

        let factory = alsa_sink_factory(|| Box::new(ZbusReserver::new()) as Box<dyn Reserver>);
        let engine = Engine::spawn(
            Box::new(factory),
            EngineConfig::new(request.settings.device.clone()),
        );
        let code = run_engine(&engine, &request, &resolved, source, &interrupt);
        let _ = engine.send(Command::Shutdown);
        engine.join();
        runtime.shutdown_timeout(Duration::from_millis(100));
        code
    }

    /// Resolves the track and opens its source; `Err` is the stderr line.
    async fn prepare(
        auth: Arc<Authenticator>,
        request: &PlayRequest,
    ) -> Result<(ResolvedStream, HttpSource), String> {
        let track_id = request.track_id;
        let quality = request.settings.quality;
        let (_, country) = auth.account().await;
        let resolver = StreamResolver::new(auth);
        let resolved = resolver
            .resolve_stream(track_id, quality)
            .await
            .map_err(|e| stream_error_message(track_id, &country, &e))?;
        let reresolve: Reresolve = Arc::new(move || {
            let resolver = resolver.clone();
            Box::pin(async move { resolver.resolve_stream(track_id, quality).await })
        });
        let client = reqwest::Client::builder()
            .connect_timeout(NETWORK_TIMEOUT)
            .build()
            .map_err(|e| format!("cannot start the HTTP client: {}", e.without_url()))?;
        let config = HttpSourceConfig::for_stream(&resolved);
        let source = HttpSource::open(client, resolved.clone(), reresolve, config)
            .await
            .map_err(|e| engine_error_message(track_id, &EngineError::Source(e)))?;
        Ok((resolved, source))
    }

    /// The track's length, when the plan says (DASH timelines do).
    fn duration(resolved: &ResolvedStream) -> Option<Duration> {
        match &resolved.plan {
            StreamPlan::Segmented { segments, .. } => segments.last().map(|s| s.end_time()),
            StreamPlan::Single { .. } => None,
        }
    }

    fn run_engine(
        engine: &Engine,
        request: &PlayRequest,
        resolved: &ResolvedStream,
        source: HttpSource,
        interrupt: &watch::Receiver<bool>,
    ) -> ExitCode {
        let play = Command::Play {
            tag: 0,
            source: Box::new(source),
            start_at: request.start_at,
        };
        if engine.send(play).is_err() {
            eprintln!("the playback engine stopped");
            return ExitCode::from(1);
        }
        let stdout = io::stdout();
        let mut out = stdout.lock();
        let mut reporter = Reporter::new(
            request.track_id,
            resolved.quality,
            duration(resolved),
            stdout.is_terminal(),
        );
        let outcome = loop {
            if *interrupt.borrow() {
                let _ = reporter.finish(&mut out);
                return ExitCode::from(EXIT_INTERRUPTED);
            }
            match engine.events().recv_timeout(POLL) {
                Ok(event) => match reporter.on_event(&event, &mut out) {
                    Ok(Some(outcome)) => break outcome,
                    Ok(None) => {}
                    // stdout closed (a pipe): keep playing, quietly.
                    Err(_) => {}
                },
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    break Outcome::Failed("the playback engine stopped".into());
                }
            }
        };
        let _ = reporter.finish(&mut out);
        let _ = out.flush();
        match outcome {
            Outcome::Ended => ExitCode::SUCCESS,
            Outcome::Failed(message) => {
                eprintln!("{message}");
                ExitCode::from(1)
            }
        }
    }
}
