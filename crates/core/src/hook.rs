//! The player event hook (spec 0025), its pure part: which events an input
//! caused, what the command gets for each, and the line of waiting events.
//! The process side is the binary's `hook` module.

use std::collections::VecDeque;
use std::time::Duration;

use crate::Track;
use crate::protocol::{PlaybackState, PlayerSnapshot};

#[cfg(test)]
mod tests;

/// The most events waiting for the command.
pub const MAX_WAITING: usize = 64;

/// One event, with what the command is told about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEvent {
    Changed(Track),
    Playing(Track, Duration),
    Paused(Track, Duration),
    Stopped(Track, Duration),
    EndOfTrack(Track),
    Seeked(Track, Duration),
    VolumeChanged(u8),
    DeviceChanged(String),
}

/// What one input did.
#[derive(Debug, Clone, Copy)]
pub struct Transition<'a> {
    pub before: &'a PlayerSnapshot,
    pub after: &'a PlayerSnapshot,
    /// A track started (its first frame, or gaplessly).
    pub started: bool,
    /// A track played to its end.
    pub ended: bool,
    /// The input can move the position by hand.
    pub seek: bool,
}

/// Turns inputs into events, remembering the track that last started: a
/// status bar has to hear `Stopped` for it however the player got there,
/// even through a skip that never played anything.
#[derive(Debug, Clone, Default)]
pub struct HookTracker {
    started: Option<Track>,
}

impl HookTracker {
    /// The events of one input, in the order they run.
    pub fn update(&mut self, input: Transition<'_>) -> Vec<HookEvent> {
        let Transition {
            before,
            after,
            started,
            ended,
            seek,
        } = input;
        let mut events = Vec::new();
        if ended && let Some(track) = &self.started {
            events.push(HookEvent::EndOfTrack(track.clone()));
        }
        if started {
            self.started = current_track(after).cloned();
            if let Some(track) = &self.started {
                events.push(HookEvent::Changed(track.clone()));
                if sounding(after.state) {
                    events.push(HookEvent::Playing(track.clone(), after.position));
                }
            }
        } else if let Some(track) = &self.started {
            if before.state == PlaybackState::Paused && sounding(after.state) {
                events.push(HookEvent::Playing(track.clone(), after.position));
            } else if sounding(before.state) && after.state == PlaybackState::Paused {
                events.push(HookEvent::Paused(track.clone(), after.position));
            }
        }
        if after.state == PlaybackState::Stopped
            && let Some(track) = self.started.take()
        {
            events.push(HookEvent::Stopped(track, after.position));
        }
        if seek
            && !started
            && before.current == after.current
            && before.position != after.position
            && (sounding(after.state) || after.state == PlaybackState::Paused)
            && let Some(track) = &self.started
            && current_track(after).is_some_and(|current| current.id == track.id)
        {
            events.push(HookEvent::Seeked(track.clone(), after.position));
        }
        if heard_volume(before) != heard_volume(after) {
            events.push(HookEvent::VolumeChanged(heard_volume(after)));
        }
        if before.device != after.device {
            events.push(HookEvent::DeviceChanged(after.device.clone()));
        }
        events
    }

    /// The event of shutting down in `snapshot`'s state: the runtime does
    /// not go through `Stopped` then, so the snapshot does not show it.
    pub fn shutdown(&mut self, snapshot: &PlayerSnapshot) -> Option<HookEvent> {
        let track = self.started.take()?;
        Some(HookEvent::Stopped(track, snapshot.position))
    }
}

/// Buffering is a stall of a track that plays, not a change of state.
fn sounding(state: PlaybackState) -> bool {
    matches!(state, PlaybackState::Playing | PlaybackState::Buffering)
}

fn current_track(snapshot: &PlayerSnapshot) -> Option<&Track> {
    let current = snapshot.current?;
    let entry = snapshot.queue.iter().find(|entry| entry.id == current)?;
    Some(&entry.track)
}

fn heard_volume(snapshot: &PlayerSnapshot) -> u8 {
    if snapshot.muted { 0 } else { snapshot.volume }
}

impl HookEvent {
    /// What follows the configured `args`: the event's name and arguments.
    pub fn arguments(&self) -> Vec<String> {
        let with_position = |name: &str, track: &Track, position: &Duration| {
            vec![
                name.to_owned(),
                track.id.to_string(),
                position.as_millis().to_string(),
            ]
        };
        match self {
            Self::Changed(track) => vec!["Changed".into(), track.id.to_string()],
            Self::Playing(track, position) => with_position("Playing", track, position),
            Self::Paused(track, position) => with_position("Paused", track, position),
            Self::Stopped(track, position) => with_position("Stopped", track, position),
            Self::EndOfTrack(track) => vec!["EndOfTrack".into(), track.id.to_string()],
            Self::Seeked(track, position) => with_position("Seeked", track, position),
            Self::VolumeChanged(percent) => vec!["VolumeChanged".into(), percent.to_string()],
            Self::DeviceChanged(name) => vec!["DeviceChanged".into(), name.clone()],
        }
    }

    /// The variables added to the command's environment.
    pub fn environment(&self) -> Vec<(&'static str, String)> {
        let track = match self {
            Self::Changed(track)
            | Self::Playing(track, _)
            | Self::Paused(track, _)
            | Self::Stopped(track, _)
            | Self::EndOfTrack(track)
            | Self::Seeked(track, _) => track,
            Self::VolumeChanged(_) | Self::DeviceChanged(_) => return Vec::new(),
        };
        let title = match &track.version {
            Some(version) => format!("{} ({version})", track.title),
            None => track.title.clone(),
        };
        let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
        let mut environment = vec![
            ("TIDAL_PLAYER_TRACK_TITLE", title),
            ("TIDAL_PLAYER_TRACK_ARTISTS", artists.join(", ")),
        ];
        if let Some(album) = track.album_title() {
            environment.push(("TIDAL_PLAYER_TRACK_ALBUM", album.to_owned()));
        }
        if let Some(duration) = track.duration {
            let ms = duration.as_millis().to_string();
            environment.push(("TIDAL_PLAYER_TRACK_DURATION_MS", ms));
        }
        environment
    }

    /// `newer` says all this one would, so it can take its place in line.
    fn replaced_by(&self, newer: &Self) -> bool {
        matches!(
            (self, newer),
            (Self::Seeked(..), Self::Seeked(..)) | (Self::VolumeChanged(_), Self::VolumeChanged(_))
        )
    }
}

/// The events waiting for the command, oldest first.
#[derive(Debug, Clone, Default)]
pub struct HookQueue {
    waiting: VecDeque<HookEvent>,
}

impl HookQueue {
    /// Adds `event`; `true` when the oldest had to go.
    pub fn push(&mut self, event: HookEvent) -> bool {
        if let Some(last) = self.waiting.back_mut()
            && last.replaced_by(&event)
        {
            *last = event;
            return false;
        }
        let full = self.waiting.len() == MAX_WAITING;
        if full {
            self.waiting.pop_front();
        }
        self.waiting.push_back(event);
        full
    }

    /// The next event to run.
    pub fn pop(&mut self) -> Option<HookEvent> {
        self.waiting.pop_front()
    }

    /// Forgets what is waiting.
    pub fn clear(&mut self) {
        self.waiting.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }
}
