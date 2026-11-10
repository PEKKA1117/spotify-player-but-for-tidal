//! The MPRIS view of the player (spec 0010 "`org.mpris.MediaPlayer2.Player`"):
//! pure functions from the player's snapshot to the properties, the
//! metadata, what changed, when `Seeked` fires, the extrapolated position,
//! and from an MPRIS call to the player's `Command`. No D-Bus types: the
//! values are [`Value`]s, which the bus layer converts.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use tidal_player_core::EntryId;
use tidal_player_core::item::parse_item;
use tidal_player_core::protocol::{Command, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode};

/// `mpris:trackid` with no current entry (the one `/org/mpris` path the
/// MPRIS spec lets players use).
pub const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
/// Our own track IDs: this, then the queue entry ID.
pub const TRACK_PREFIX: &str = "/tidal_player/entry/";

/// A property or metadata value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Double(f64),
    Int64(i64),
    Str(String),
    /// A D-Bus object path.
    ObjectPath(String),
    StrList(Vec<String>),
    Metadata(Metadata),
}

/// `Metadata`: key → value.
pub type Metadata = BTreeMap<String, Value>;
/// `org.mpris.MediaPlayer2.Player`'s properties: name → value.
pub type Properties = BTreeMap<&'static str, Value>;

/// The `mpris:trackid` of a queue entry.
pub fn trackid(entry: EntryId) -> String {
    format!("{TRACK_PREFIX}{}", entry.0)
}

/// The entry a `mpris:trackid` names, when it is one of ours.
pub fn entry_of(trackid: &str) -> Option<EntryId> {
    let id = trackid.strip_prefix(TRACK_PREFIX)?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    id.parse().ok().map(EntryId)
}

/// Whether `path` is a D-Bus object path: `/`, or `/`-separated non-empty
/// elements of `[A-Za-z0-9_]`.
fn is_object_path(path: &str) -> bool {
    path == "/"
        || path.strip_prefix('/').is_some_and(|rest| {
            rest.split('/').all(|element| {
                !element.is_empty()
                    && element
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        })
}

fn current(snapshot: &PlayerSnapshot) -> Option<(usize, &QueueEntry)> {
    let id = snapshot.current?;
    snapshot.queue.iter().enumerate().find(|(_, e)| e.id == id)
}

fn micros(d: Duration) -> i64 {
    i64::try_from(d.as_micros()).unwrap_or(i64::MAX)
}

/// `Metadata` of the current entry (`NoTrack` alone with none); `art`
/// gives the `mpris:artUrl` for a cover ID (`None`: omitted).
pub fn metadata(snapshot: &PlayerSnapshot, art: &dyn Fn(&str) -> Option<String>) -> Metadata {
    let mut metadata = Metadata::new();
    let Some((_, entry)) = current(snapshot) else {
        metadata.insert(
            "mpris:trackid".into(),
            Value::ObjectPath(NO_TRACK.to_owned()),
        );
        return metadata;
    };
    let track = &entry.track;
    let mut put = |key: &str, value: Value| {
        metadata.insert(key.to_owned(), value);
    };
    put("mpris:trackid", Value::ObjectPath(trackid(entry.id)));
    if let Some(duration) = track.duration {
        put("mpris:length", Value::Int64(micros(duration)));
    }
    let title = match &track.version {
        Some(version) => format!("{} ({version})", track.title),
        None => track.title.clone(),
    };
    put("xesam:title", Value::Str(title));
    put(
        "xesam:artist",
        Value::StrList(track.artists.iter().map(|a| a.name.clone()).collect()),
    );
    if let Some(album) = &track.album {
        put("xesam:album", Value::Str(album.title.clone()));
        if let Some(url) = album.cover.as_deref().and_then(art) {
            put("mpris:artUrl", Value::Str(url));
        }
    }
    put(
        "xesam:url",
        Value::Str(format!("https://tidal.com/browse/track/{}", track.id)),
    );
    metadata
}

/// `PlaybackStatus`: a load reads as playing (the user asked to play); the
/// player already shows a load held by a pause as `Paused`.
fn playback_status(state: PlaybackState) -> &'static str {
    match state {
        PlaybackState::Playing | PlaybackState::Buffering | PlaybackState::Loading => "Playing",
        PlaybackState::Paused => "Paused",
        PlaybackState::Stopped => "Stopped",
    }
}

fn loop_status(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "None",
        RepeatMode::Queue => "Playlist",
        RepeatMode::Track => "Track",
    }
}

/// Every property of the `Player` table: `position` is the current
/// entry's position (from [`PositionClock`]), `art` as for [`metadata`].
pub fn player_properties(
    snapshot: &PlayerSnapshot,
    position: Duration,
    art: &dyn Fn(&str) -> Option<String>,
) -> Properties {
    let current = current(snapshot);
    let has_current = current.is_some();
    let can_go_next = current.is_some_and(|(index, _)| {
        index + 1 < snapshot.queue.len() || snapshot.repeat != RepeatMode::Off
    });
    let can_seek = current.is_some_and(|(_, e)| e.track.duration.is_some());
    let volume = if snapshot.muted {
        0.0
    } else {
        f64::from(snapshot.volume) / 100.0
    };
    let position = if has_current { micros(position) } else { 0 };
    let mut p = Properties::new();
    p.insert(
        "PlaybackStatus",
        Value::Str(playback_status(snapshot.state).into()),
    );
    p.insert(
        "LoopStatus",
        Value::Str(loop_status(snapshot.repeat).into()),
    );
    p.insert("Shuffle", Value::Bool(snapshot.shuffle));
    p.insert("Volume", Value::Double(volume));
    p.insert("Position", Value::Int64(position));
    p.insert("Rate", Value::Double(1.0));
    p.insert("MinimumRate", Value::Double(1.0));
    p.insert("MaximumRate", Value::Double(1.0));
    p.insert("Metadata", Value::Metadata(metadata(snapshot, art)));
    p.insert("CanGoNext", Value::Bool(can_go_next));
    p.insert("CanGoPrevious", Value::Bool(has_current));
    p.insert("CanPlay", Value::Bool(has_current));
    p.insert("CanPause", Value::Bool(has_current));
    p.insert("CanSeek", Value::Bool(can_seek));
    p.insert("CanControl", Value::Bool(true));
    p
}

/// The properties `new` changed from `old`, with their new values;
/// `Position` never counts (it is not signalled).
pub fn changes(old: &Properties, new: &Properties) -> Properties {
    new.iter()
        .filter(|(key, value)| **key != "Position" && old.get(*key) != Some(*value))
        .map(|(key, value)| (*key, value.clone()))
        .collect()
}

/// Where the current entry was or is: its ID and position.
pub type At = Option<(EntryId, Duration)>;

/// How far a position may run ahead of the clock before it is a seek.
const SLACK: Duration = Duration::from_secs(1);

/// The position to signal with `Seeked`, when the current entry's position
/// jumped from `old` to `new` (`elapsed`: the playing time between the two
/// reports). Another entry counts from `0:00`: it is a seek only when it
/// starts elsewhere (a restored position).
pub fn seeked(old: At, new: At, elapsed: Duration) -> Option<Duration> {
    let (entry, position) = new?;
    let from = match old {
        Some((before, at)) if before == entry => at,
        _ => Duration::ZERO,
    };
    (position < from || position > from + elapsed + SLACK).then_some(position)
}

/// The current entry's position as the player last reported it, moving on
/// with the clock while it plays (spec 0010 `Position`).
#[derive(Debug, Clone)]
pub struct PositionClock {
    entry: Option<EntryId>,
    position: Duration,
    at: Instant,
    playing: bool,
    duration: Option<Duration>,
}

impl PositionClock {
    pub fn new(now: Instant) -> Self {
        Self {
            entry: None,
            position: Duration::ZERO,
            at: now,
            playing: false,
            duration: None,
        }
    }

    /// A report from the player: a snapshot, or a position event.
    pub fn report(
        &mut self,
        entry: Option<EntryId>,
        position: Duration,
        state: PlaybackState,
        duration: Option<Duration>,
        now: Instant,
    ) {
        self.entry = entry;
        self.position = position;
        self.playing = state == PlaybackState::Playing;
        self.duration = duration;
        self.at = now;
    }

    /// Where the last report was.
    pub fn at(&self) -> At {
        self.entry.map(|e| (e, self.position))
    }

    /// The playing time since the last report.
    pub fn elapsed(&self, now: Instant) -> Duration {
        if self.playing {
            now.saturating_duration_since(self.at)
        } else {
            Duration::ZERO
        }
    }

    /// The position now: `0` with no current entry.
    pub fn position(&self, now: Instant) -> Duration {
        if self.entry.is_none() {
            return Duration::ZERO;
        }
        let position = self.position + self.elapsed(now);
        match self.duration {
            Some(duration) => position.min(duration.max(self.position)),
            None => position,
        }
    }
}

/// An MPRIS method call or property write (spec 0010 "methods and writes").
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    /// Offset in µs.
    Seek(i64),
    /// A `trackid` (an object path, as given) and a position in µs.
    SetPosition {
        trackid: String,
        position: i64,
    },
    OpenUri(String),
    SetShuffle(bool),
    SetLoopStatus(String),
    SetVolume(f64),
    SetRate(f64),
}

/// Why a call failed, as the bus answers it (spec 0010 "Errors").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// `org.freedesktop.DBus.Error.InvalidArgs`.
    InvalidArgs(String),
    /// `org.mpris.MediaPlayer2.tidal_player.Error.Failed`.
    Failed(String),
    /// `org.mpris.MediaPlayer2.tidal_player.Error.Timeout`.
    Timeout(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArgs(m) | Self::Failed(m) | Self::Timeout(m) => f.write_str(m),
        }
    }
}

/// The player command for `call`; `None`: the call does nothing.
pub fn command_for(call: &Call) -> Result<Option<Command>, CallError> {
    Ok(Some(match call {
        Call::PlayPause => Command::TogglePause,
        Call::Play => Command::Play,
        Call::Pause => Command::Pause,
        Call::Stop => Command::Stop,
        Call::Next => Command::Next,
        Call::Previous => Command::Previous,
        // Integer division rounds toward zero.
        Call::Seek(offset) => Command::SeekBy(offset / 1000),
        Call::SetPosition { trackid, position } => {
            if !is_object_path(trackid) {
                return Err(CallError::InvalidArgs(format!(
                    "Not an object path: {trackid}"
                )));
            }
            let (Some(entry), Ok(position)) = (entry_of(trackid), u64::try_from(*position)) else {
                return Ok(None);
            };
            Command::SetPosition {
                entry,
                position: Duration::from_micros(position),
            }
        }
        Call::OpenUri(uri) => {
            let item = parse_item(uri).map_err(|e| CallError::InvalidArgs(e.to_string()))?;
            Command::Open {
                items: vec![item],
                at: None,
            }
        }
        Call::SetShuffle(on) => Command::SetShuffle(*on),
        Call::SetLoopStatus(status) => Command::SetRepeat(match status.as_str() {
            "None" => RepeatMode::Off,
            "Playlist" => RepeatMode::Queue,
            "Track" => RepeatMode::Track,
            other => {
                return Err(CallError::InvalidArgs(format!(
                    "LoopStatus must be None, Track or Playlist, got \"{other}\""
                )));
            }
        }),
        // NaN becomes 0 (a saturating cast).
        Call::SetVolume(volume) => {
            Command::SetVolume((volume * 100.0).round().clamp(0.0, 100.0) as u8)
        }
        Call::SetRate(_) => return Ok(None),
    }))
}

#[cfg(test)]
mod tests;
