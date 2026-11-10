//! The zbus layer (spec 0010 "Where it runs"): the bus name, the object
//! `/org/mpris/MediaPlayer2` with `org.mpris.MediaPlayer2` and
//! `org.mpris.MediaPlayer2.Player` (zbus adds `Properties`, `Introspectable`
//! and `Peer`), and the signals. Every value comes from the [`Adapter`];
//! every call goes to it. Checked end to end by `tests/mpris.rs`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use zbus::fdo::RequestNameFlags;
use zbus::names::ErrorName;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{self, ObjectPath, OwnedValue};

use super::hub::{Adapter, Bus};
use super::model::{Call, CallError, Properties, Value};
use super::{Level, Log};

/// The bus name (`playerctl -p tidal_player`).
pub const NAME: &str = "org.mpris.MediaPlayer2.tidal_player";
/// The object every MPRIS player serves.
pub const PATH: &str = "/org/mpris/MediaPlayer2";
pub const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
pub const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
/// The prefix of our own error names.
pub const ERROR_PREFIX: &str = "org.mpris.MediaPlayer2.tidal_player.Error";

/// Connects to the session bus, serves the object and takes the name (or
/// `.instance<pid>` when it is taken): the signal emitter for the adapter
/// and the name, or why not.
pub async fn connect(
    adapter: Adapter,
    runtime: tokio::runtime::Handle,
    log: Log,
) -> Result<(Arc<dyn Bus>, String), String> {
    let player = Player { adapter, runtime };
    let connection = zbus::connection::Builder::session()
        .and_then(|b| b.serve_at(PATH, Root))
        .and_then(|b| b.serve_at(PATH, player))
        .map_err(|e| e.to_string())?
        .build()
        .await
        .map_err(|e| e.to_string())?;
    let flags = RequestNameFlags::DoNotQueue.into();
    let name = match connection.request_name_with_flags(NAME, flags).await {
        Ok(_) => NAME.to_owned(),
        Err(zbus::Error::NameTaken) => {
            // A second player (another runtime directory): the MPRIS spec's
            // name for further instances.
            let name = format!("{NAME}.instance{}", std::process::id());
            connection
                .request_name_with_flags(name.as_str(), flags)
                .await
                .map_err(|e| format!("cannot own {name}: {e}"))?;
            name
        }
        Err(e) => return Err(format!("cannot own {NAME}: {e}")),
    };
    let emitter = Emitter {
        connection: zbus::blocking::Connection::from(connection),
        broken: AtomicBool::new(false),
        log,
    };
    Ok((Arc::new(emitter), name))
}

/// Emits the adapter's signals; the first failure (the bus went away) is
/// logged once, and nothing is retried.
struct Emitter {
    connection: zbus::blocking::Connection,
    broken: AtomicBool,
    log: Log,
}

impl Emitter {
    fn emit<B>(&self, interface: &str, member: &str, body: &B)
    where
        B: serde::Serialize + zvariant::DynamicType,
    {
        if self.broken.load(Ordering::Relaxed) {
            return;
        }
        if let Err(e) = self
            .connection
            .emit_signal(None::<&str>, PATH, interface, member, body)
            && !self.broken.swap(true, Ordering::Relaxed)
        {
            (self.log)(Level::Warn, &format!("MPRIS stopped working: {e}"));
        }
    }
}

impl Bus for Emitter {
    fn properties_changed(&self, changed: &Properties) {
        let changed: HashMap<&str, zvariant::Value<'_>> =
            changed.iter().map(|(k, v)| (*k, to_value(v))).collect();
        let invalidated: Vec<&str> = Vec::new();
        self.emit(
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(PLAYER_INTERFACE, changed, invalidated),
        );
    }

    fn seeked(&self, position: Duration) {
        self.emit(PLAYER_INTERFACE, "Seeked", &(micros(position),));
    }

    fn close(&self) {
        let connection = self.connection.inner().clone();
        if let Err(e) = zbus::block_on(connection.close()) {
            tracing::debug!("closing the session bus connection: {e}");
        }
    }
}

fn micros(d: Duration) -> i64 {
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
}

/// A model value as a D-Bus value.
fn to_value(value: &Value) -> zvariant::Value<'static> {
    match value {
        Value::Bool(b) => zvariant::Value::from(*b),
        Value::Double(d) => zvariant::Value::from(*d),
        Value::Int64(i) => zvariant::Value::from(*i),
        Value::Str(s) => zvariant::Value::from(s.clone()),
        Value::ObjectPath(p) => match ObjectPath::try_from(p.clone()) {
            Ok(path) => zvariant::Value::from(path),
            Err(_) => zvariant::Value::from(p.clone()),
        },
        Value::StrList(list) => zvariant::Value::from(list.clone()),
        Value::Metadata(map) => zvariant::Value::from(metadata(map)),
    }
}

fn metadata(map: &super::model::Metadata) -> HashMap<String, OwnedValue> {
    map.iter()
        .filter_map(|(k, v)| Some((k.clone(), OwnedValue::try_from(to_value(v)).ok()?)))
        .collect()
}

/// A failed call as the bus answers it (spec 0010 "Errors").
#[derive(Debug)]
struct MprisError(CallError);

impl MprisError {
    fn name_str(&self) -> &'static str {
        match self.0 {
            CallError::InvalidArgs(_) => "org.freedesktop.DBus.Error.InvalidArgs",
            CallError::Failed(_) => "org.mpris.MediaPlayer2.tidal_player.Error.Failed",
            CallError::Timeout(_) => "org.mpris.MediaPlayer2.tidal_player.Error.Timeout",
        }
    }

    fn message(&self) -> &str {
        match &self.0 {
            CallError::InvalidArgs(m) | CallError::Failed(m) | CallError::Timeout(m) => m,
        }
    }
}

impl std::fmt::Display for MprisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name_str(), self.message())
    }
}

impl std::error::Error for MprisError {}

impl zbus::DBusError for MprisError {
    fn create_reply(&self, call: &zbus::message::Header<'_>) -> zbus::Result<zbus::Message> {
        zbus::Message::error(call, self.name())?.build(&(self.message(),))
    }

    fn name(&self) -> ErrorName<'_> {
        ErrorName::from_static_str_unchecked(self.name_str())
    }

    fn description(&self) -> Option<&str> {
        Some(self.message())
    }
}

/// A property write's error: property writes can only answer the
/// standard errors (zbus), so a timeout is `NoReply`.
fn write_error(e: CallError) -> zbus::fdo::Error {
    match e {
        CallError::InvalidArgs(m) => zbus::fdo::Error::InvalidArgs(m),
        CallError::Failed(m) => zbus::fdo::Error::Failed(m),
        CallError::Timeout(m) => zbus::fdo::Error::NoReply(m),
    }
}

/// `org.mpris.MediaPlayer2`.
struct Root;

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    /// Does nothing (`CanRaise` is false).
    fn raise(&self) {}

    /// Does nothing: a desktop widget must not stop a daemon.
    fn quit(&self) {}

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_quit(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn identity(&self) -> String {
        "tidal-player".into()
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["tidal".into(), "https".into()]
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

/// `org.mpris.MediaPlayer2.Player`, answered from the adapter's copy.
struct Player {
    adapter: Adapter,
    runtime: tokio::runtime::Handle,
}

impl Player {
    /// Runs `call` on the player's tokio runtime (zbus serves on its own
    /// executor).
    async fn run(&self, call: Call) -> Result<(), CallError> {
        let adapter = self.adapter.clone();
        match self
            .runtime
            .spawn(async move { adapter.call(call).await })
            .await
        {
            Ok(result) => result,
            Err(e) => Err(CallError::Failed(e.to_string())),
        }
    }

    async fn method(&self, call: Call) -> Result<(), MprisError> {
        self.run(call).await.map_err(MprisError)
    }

    async fn write(&self, call: Call) -> zbus::fdo::Result<()> {
        self.run(call).await.map_err(write_error)
    }

    fn get(&self, name: &str) -> Value {
        self.adapter.property(name).unwrap_or(Value::Bool(false))
    }

    fn string(&self, name: &str) -> String {
        match self.get(name) {
            Value::Str(s) => s,
            _ => String::new(),
        }
    }

    fn bool(&self, name: &str) -> bool {
        matches!(self.get(name), Value::Bool(true))
    }

    fn double(&self, name: &str) -> f64 {
        match self.get(name) {
            Value::Double(d) => d,
            _ => 0.0,
        }
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    async fn next(&self) -> Result<(), MprisError> {
        self.method(Call::Next).await
    }

    async fn previous(&self) -> Result<(), MprisError> {
        self.method(Call::Previous).await
    }

    async fn pause(&self) -> Result<(), MprisError> {
        self.method(Call::Pause).await
    }

    async fn play_pause(&self) -> Result<(), MprisError> {
        self.method(Call::PlayPause).await
    }

    async fn stop(&self) -> Result<(), MprisError> {
        self.method(Call::Stop).await
    }

    async fn play(&self) -> Result<(), MprisError> {
        self.method(Call::Play).await
    }

    async fn seek(&self, offset: i64) -> Result<(), MprisError> {
        self.method(Call::Seek(offset)).await
    }

    async fn set_position(
        &self,
        track_id: ObjectPath<'_>,
        position: i64,
    ) -> Result<(), MprisError> {
        self.method(Call::SetPosition {
            trackid: track_id.to_string(),
            position,
        })
        .await
    }

    async fn open_uri(&self, uri: String) -> Result<(), MprisError> {
        self.method(Call::OpenUri(uri)).await
    }

    /// Emitted by the adapter (declared here for introspection).
    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.string("PlaybackStatus")
    }

    // The writable properties are signalled by the adapter once the player
    // applied the write, not by zbus after the setter (which would signal
    // them twice): hence `emits_changed_signal = "false"`.
    #[zbus(property(emits_changed_signal = "false"))]
    fn loop_status(&self) -> String {
        self.string("LoopStatus")
    }

    #[zbus(property)]
    async fn set_loop_status(&self, value: String) -> zbus::fdo::Result<()> {
        self.write(Call::SetLoopStatus(value)).await
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn rate(&self) -> f64 {
        1.0
    }

    /// Does nothing: the rate is always 1.0.
    #[zbus(property)]
    fn set_rate(&self, value: f64) {
        let _ = value;
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn shuffle(&self) -> bool {
        self.bool("Shuffle")
    }

    #[zbus(property)]
    async fn set_shuffle(&self, value: bool) -> zbus::fdo::Result<()> {
        self.write(Call::SetShuffle(value)).await
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        match self.get("Metadata") {
            Value::Metadata(map) => metadata(&map),
            _ => HashMap::new(),
        }
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn volume(&self) -> f64 {
        self.double("Volume")
    }

    #[zbus(property)]
    async fn set_volume(&self, value: f64) -> zbus::fdo::Result<()> {
        self.write(Call::SetVolume(value)).await
    }

    /// Never signalled (the MPRIS spec): `Seeked` covers jumps.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        match self.get("Position") {
            Value::Int64(p) => p,
            _ => 0,
        }
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.bool("CanGoNext")
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.bool("CanGoPrevious")
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.bool("CanPlay")
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.bool("CanPause")
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.bool("CanSeek")
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn can_control(&self) -> bool {
        true
    }
}
