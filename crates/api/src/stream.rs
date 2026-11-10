//! Stream resolution (spec 0003, AC1–AC5): turns a track ID into a
//! [`StreamPlan`] the player can fetch, through
//! `GET /tracks/{id}/playbackinfopostpaywall`.
//!
//! One request per resolve, at the highest quality asked for: Tidal grants
//! less by itself when the track or the account has less, and the granted
//! quality is reported in [`ResolvedStream::quality`]. Nothing is downloaded
//! here; the plan only says where the bytes are.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use serde::Deserialize;
use tidal_player_core::AudioQuality;

use crate::SUB_STATUS_NOT_AVAILABLE;
use crate::auth::{AuthError, Authenticator};

/// How long the `playbackinfopostpaywall` request may take before it fails
/// with [`AuthError::Transport`].
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);

/// An audio codec the player can decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    /// FLAC, as a raw FLAC file or in (fragmented) MP4.
    Flac,
    /// AAC-LC (`mp4a.40.2`) in MP4.
    AacLc,
}

/// One media segment of a [`StreamPlan::Segmented`] stream.
///
/// Times are in ticks of `timescale` per second, as in the MPD, so they add
/// up exactly; [`Segment::start_time`] and [`Segment::duration_time`] convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The DASH segment number (`$Number$`).
    pub number: u64,
    /// The segment's URL.
    pub url: String,
    /// Start, in ticks from the start of the track.
    pub start: u64,
    /// Length, in ticks.
    pub duration: u64,
    /// Ticks per second.
    pub timescale: u32,
}

impl Segment {
    /// Start from the start of the track (rounded down to the nanosecond).
    pub fn start_time(&self) -> Duration {
        ticks_to_duration(self.start, self.timescale)
    }

    /// Length (rounded down to the nanosecond).
    pub fn duration_time(&self) -> Duration {
        ticks_to_duration(self.duration, self.timescale)
    }

    /// End from the start of the track (rounded down to the nanosecond).
    pub fn end_time(&self) -> Duration {
        ticks_to_duration(self.start + self.duration, self.timescale)
    }
}

fn ticks_to_duration(ticks: u64, timescale: u32) -> Duration {
    if timescale == 0 {
        return Duration::ZERO;
    }
    let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(timescale);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// Where a track's bytes are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamPlan {
    /// One file (BTS manifest), fetched with HTTP `Range` requests.
    Single { url: String, codec: Codec },
    /// MPEG-DASH: the init segment, then `segments` in order, joined.
    Segmented {
        init_url: String,
        segments: Vec<Segment>,
        codec: Codec,
    },
}

impl StreamPlan {
    /// The codec of the stream.
    pub fn codec(&self) -> Codec {
        match self {
            Self::Single { codec, .. } | Self::Segmented { codec, .. } => *codec,
        }
    }
}

/// A resolved track: what Tidal granted, and where to fetch it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStream {
    /// The track ID Tidal answered for.
    pub track_id: u64,
    /// The quality Tidal **granted** (may be lower than asked).
    pub quality: AudioQuality,
    /// Bits per sample, when the response says (lossless grants).
    pub bit_depth: Option<u8>,
    /// Sample rate in Hz, when the response says (lossless grants).
    pub sample_rate: Option<u32>,
    pub plan: StreamPlan,
}

/// What made a stream unplayable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    /// An `encryptionType` other than `NONE`, or DASH `ContentProtection`.
    Encrypted,
    /// A `manifestMimeType` other than BTS or DASH.
    Manifest(String),
    /// An `audioMode` other than `STEREO` (Dolby Atmos, Sony 360).
    AudioMode(String),
    /// A codec other than FLAC or AAC-LC (e.g. HE-AAC).
    Codec(String),
    /// An `audioQuality` this player does not know.
    Quality(String),
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encrypted => f.write_str("the stream is encrypted"),
            Self::Manifest(mime) => write!(f, "unknown manifest type {mime:?}"),
            Self::AudioMode(mode) => write!(f, "audio mode {mode} is not supported"),
            Self::Codec(codec) => write!(f, "codec {codec} is not supported"),
            Self::Quality(quality) => write!(f, "unknown audio quality {quality:?}"),
        }
    }
}

/// Why a track could not be resolved.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StreamError {
    /// `401` with `subStatus` `4005`: not playable for this account or
    /// country (or at this quality).
    #[error("the track is not available")]
    NotAvailable,
    /// `500` with `subStatus` `999`: unknown track ID, or not streamable in
    /// the account's country.
    #[error("the track was not found, or cannot be streamed in this country")]
    NotFound,
    /// The response is for a preview, not the full track.
    #[error("the track is only available as a preview")]
    PreviewOnly,
    /// Another `500`.
    #[error("Tidal server error (HTTP {0})")]
    Server(u16),
    /// The stream exists but cannot be played.
    #[error("not playable: {0}")]
    Unsupported(Unsupported),
    /// A `200` whose body or manifest could not be read; says what was
    /// wrong, never quotes the body (its URLs carry tokens).
    #[error("malformed stream response: {0}")]
    Malformed(&'static str),
    /// From the [`Authenticator`], unchanged: transport errors and
    /// timeouts, other statuses (`5xx`, `429`, …), `LoginRequired`.
    #[error(transparent)]
    Auth(#[from] AuthError),
}

/// Resolves tracks to streams through the [`Authenticator`].
#[derive(Debug, Clone)]
pub struct StreamResolver {
    auth: Arc<Authenticator>,
    timeout: Duration,
}

impl StreamResolver {
    /// A resolver whose requests time out after [`RESOLVE_TIMEOUT`].
    pub fn new(auth: Arc<Authenticator>) -> Self {
        Self {
            auth,
            timeout: RESOLVE_TIMEOUT,
        }
    }

    /// Replaces the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Resolves `track_id`, asking for `max_quality` (AC1, AC5).
    ///
    /// Sends exactly one `playbackinfopostpaywall` request, whatever the
    /// outcome (no client-side quality ladder): Tidal downgrades by itself,
    /// and [`ResolvedStream::quality`] says what it granted. Errors map as
    /// in spec 0003 "Quality"; anything not listed there comes back as
    /// [`StreamError::Auth`], unchanged.
    pub async fn resolve_stream(
        &self,
        track_id: u64,
        max_quality: AudioQuality,
    ) -> Result<ResolvedStream, StreamError> {
        let (_, country) = self.auth.account().await;
        let path = format!("tracks/{track_id}/playbackinfopostpaywall");
        let query = [
            ("audioquality", max_quality.tidal_name()),
            ("playbackmode", "STREAM"),
            ("assetpresentation", "FULL"),
            ("countryCode", country.as_str()),
        ];
        let response = self.auth.get(&path, &query, Some(self.timeout)).await?;
        if response.status().is_success() {
            return parse_playback_info(response.body());
        }
        Err(map_error(response.status().as_u16(), response.sub_status()))
    }
}

/// Tidal's `subStatus` on a `500` for an unknown track ID, or one that
/// cannot be streamed in the account's country.
const SUB_STATUS_NOT_FOUND: u64 = 999;

/// Maps a non-2xx `playbackinfopostpaywall` answer (AC5).
fn map_error(status: u16, sub_status: Option<u64>) -> StreamError {
    match (status, sub_status) {
        (401, Some(SUB_STATUS_NOT_AVAILABLE)) => StreamError::NotAvailable,
        (500, Some(SUB_STATUS_NOT_FOUND)) => StreamError::NotFound,
        (500, _) => StreamError::Server(500),
        (status, _) => StreamError::Auth(AuthError::Http { status, code: None }),
    }
}

const MIME_BTS: &str = "application/vnd.tidal.bts";
const MIME_DASH: &str = "application/dash+xml";

/// The fields of a `playbackinfopostpaywall` body that resolution uses.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackInfo {
    track_id: u64,
    asset_presentation: String,
    audio_mode: String,
    audio_quality: String,
    manifest_mime_type: String,
    manifest: String,
    bit_depth: Option<u8>,
    sample_rate: Option<u32>,
}

/// Parses and validates a `playbackinfopostpaywall` body (AC2–AC4).
///
/// Error messages never quote the body: its URLs carry tokens.
pub fn parse_playback_info(body: &[u8]) -> Result<ResolvedStream, StreamError> {
    let info: PlaybackInfo = serde_json::from_slice(body)
        .map_err(|_| StreamError::Malformed("playback info is not the expected JSON"))?;
    if info.asset_presentation != "FULL" {
        return Err(StreamError::PreviewOnly);
    }
    if info.audio_mode != "STEREO" {
        return Err(StreamError::Unsupported(Unsupported::AudioMode(
            info.audio_mode,
        )));
    }
    let quality = serde_json::from_value(serde_json::Value::String(info.audio_quality.clone()))
        .map_err(|_| StreamError::Unsupported(Unsupported::Quality(info.audio_quality)))?;
    let plan = match info.manifest_mime_type.as_str() {
        MIME_BTS => parse_bts(&decode_manifest(&info.manifest)?)?,
        MIME_DASH => {
            let xml = String::from_utf8(decode_manifest(&info.manifest)?)
                .map_err(|_| StreamError::Malformed("MPD is not UTF-8"))?;
            parse_mpd(&xml)?
        }
        _ => {
            return Err(StreamError::Unsupported(Unsupported::Manifest(
                info.manifest_mime_type,
            )));
        }
    };
    Ok(ResolvedStream {
        track_id: info.track_id,
        quality,
        bit_depth: info.bit_depth,
        sample_rate: info.sample_rate,
        plan,
    })
}

fn decode_manifest(manifest: &str) -> Result<Vec<u8>, StreamError> {
    BASE64
        .decode(manifest)
        .map_err(|_| StreamError::Malformed("manifest is not base64"))
}

/// Maps a manifest's codec string; HE-AAC and anything else is unsupported.
fn codec(codecs: &str) -> Result<Codec, StreamError> {
    match codecs.trim().to_ascii_lowercase().as_str() {
        "flac" => Ok(Codec::Flac),
        "mp4a.40.2" => Ok(Codec::AacLc),
        _ => Err(StreamError::Unsupported(Unsupported::Codec(
            codecs.to_owned(),
        ))),
    }
}

/// A BTS manifest.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bts {
    codecs: String,
    encryption_type: String,
    urls: Vec<String>,
}

/// Parses a BTS manifest (AC2): one file, `urls[0]`.
fn parse_bts(manifest: &[u8]) -> Result<StreamPlan, StreamError> {
    let bts: Bts = serde_json::from_slice(manifest)
        .map_err(|_| StreamError::Malformed("BTS manifest is not the expected JSON"))?;
    if bts.encryption_type != "NONE" {
        return Err(StreamError::Unsupported(Unsupported::Encrypted));
    }
    let codec = codec(&bts.codecs)?;
    let url = bts
        .urls
        .into_iter()
        .next()
        .ok_or(StreamError::Malformed("BTS manifest has no URL"))?;
    Ok(StreamPlan::Single { url, codec })
}

/// Upper bound on the segments of one track (about 4 days of 4 s segments),
/// so a hostile `r` cannot exhaust memory.
const MAX_SEGMENTS: u64 = 100_000;

/// One `<S>` entry of a `SegmentTimeline`.
struct TimelineEntry {
    t: Option<u64>,
    d: u64,
    r: u64,
}

/// What the MPD parser collects.
#[derive(Default)]
struct Mpd {
    encrypted: bool,
    representations: usize,
    adaptation_codecs: Option<String>,
    representation_codecs: Option<String>,
    timescale: Option<u32>,
    initialization: Option<String>,
    media: Option<String>,
    start_number: Option<u64>,
    timeline: Vec<TimelineEntry>,
}

/// Parses a DASH MPD (AC3): one audio `Representation` whose
/// `SegmentTemplate` has `initialization`, `media` with `$Number$`,
/// `startNumber`, `timescale` and a `SegmentTimeline`. URLs must be
/// absolute (Tidal's are; `BaseURL` is not supported).
fn parse_mpd(xml: &str) -> Result<StreamPlan, StreamError> {
    let mpd = read_mpd(xml)?;
    if mpd.encrypted {
        return Err(StreamError::Unsupported(Unsupported::Encrypted));
    }
    if mpd.representations != 1 {
        return Err(StreamError::Malformed(
            "MPD must have exactly one Representation",
        ));
    }
    let codecs = mpd
        .representation_codecs
        .or(mpd.adaptation_codecs)
        .ok_or(StreamError::Malformed("MPD has no codecs"))?;
    let codec = codec(&codecs)?;
    let init_url = mpd
        .initialization
        .ok_or(StreamError::Malformed("MPD has no initialization segment"))?;
    let media = mpd
        .media
        .filter(|m| m.contains("$Number$"))
        .ok_or(StreamError::Malformed(
            "MPD has no media template with $Number$",
        ))?;
    let timescale = mpd.timescale.unwrap_or(1);
    if timescale == 0 {
        return Err(StreamError::Malformed("MPD timescale is 0"));
    }
    if mpd.timeline.is_empty() {
        return Err(StreamError::Malformed("MPD has no SegmentTimeline"));
    }
    let total: u64 = mpd
        .timeline
        .iter()
        .map(|s| s.r.saturating_add(1))
        .fold(0, u64::saturating_add);
    if total > MAX_SEGMENTS {
        return Err(StreamError::Malformed("MPD has too many segments"));
    }

    let mut segments = Vec::with_capacity(total as usize);
    let mut time = 0u64;
    let mut number = mpd.start_number.unwrap_or(1);
    for entry in &mpd.timeline {
        if let Some(t) = entry.t {
            time = t;
        }
        for _ in 0..=entry.r {
            segments.push(Segment {
                number,
                url: media.replace("$Number$", &number.to_string()),
                start: time,
                duration: entry.d,
                timescale,
            });
            time = time
                .checked_add(entry.d)
                .ok_or(StreamError::Malformed("MPD timeline overflows"))?;
            number += 1;
        }
    }
    Ok(StreamPlan::Segmented {
        init_url,
        segments,
        codec,
    })
}

/// Collects the parts of the MPD that [`parse_mpd`] uses.
fn read_mpd(xml: &str) -> Result<Mpd, StreamError> {
    const BAD_XML: StreamError = StreamError::Malformed("MPD is not well-formed XML");
    let mut reader = Reader::from_str(xml);
    let mut mpd = Mpd::default();
    loop {
        let element = match reader.read_event().map_err(|_| BAD_XML)? {
            Event::Start(e) | Event::Empty(e) => e,
            Event::Eof => break,
            _ => continue,
        };
        let attrs = attributes(&element)?;
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        match element.local_name().as_ref() {
            "ContentProtection" => mpd.encrypted = true,
            "AdaptationSet" => mpd.adaptation_codecs = get("codecs"),
            "Representation" => {
                mpd.representations += 1;
                mpd.representation_codecs = get("codecs");
            }
            "SegmentTemplate" => {
                mpd.timescale = get("timescale").map(|v| number(&v)).transpose()?;
                mpd.initialization = get("initialization");
                mpd.media = get("media");
                mpd.start_number = get("startNumber").map(|v| number(&v)).transpose()?;
            }
            "S" => {
                let d = get("d").ok_or(StreamError::Malformed("MPD segment has no duration"))?;
                let d: u64 = number(&d)?;
                if d == 0 {
                    return Err(StreamError::Malformed("MPD segment has no duration"));
                }
                mpd.timeline.push(TimelineEntry {
                    t: get("t").map(|v| number(&v)).transpose()?,
                    d,
                    // A negative `r` (open-ended) does not parse as u64.
                    r: get("r").map(|v| number(&v)).transpose()?.unwrap_or(0),
                });
            }
            _ => {}
        }
    }
    Ok(mpd)
}

/// The element's attributes as `(local name, unescaped value)`.
fn attributes(element: &BytesStart<'_>) -> Result<Vec<(String, String)>, StreamError> {
    const BAD_ATTR: StreamError = StreamError::Malformed("MPD has a malformed attribute");
    element
        .attributes()
        .map(|attr| {
            let attr = attr.map_err(|_| BAD_ATTR)?;
            let key = attr.key.local_name().as_ref().to_owned();
            let value = attr
                .normalized_value(XmlVersion::Implicit1_0)
                .map_err(|_| BAD_ATTR)?
                .into_owned();
            Ok((key, value))
        })
        .collect()
}

fn number<T: std::str::FromStr>(value: &str) -> Result<T, StreamError> {
    value
        .trim()
        .parse()
        .map_err(|_| StreamError::Malformed("MPD has a malformed number"))
}

#[cfg(test)]
mod tests;
