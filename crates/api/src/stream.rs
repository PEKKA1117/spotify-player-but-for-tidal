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

use tidal_player_core::AudioQuality;

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
    /// A `200` whose body or manifest could not be read.
    #[error("malformed stream response: {0}")]
    Malformed(String),
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
    pub async fn resolve_stream(
        &self,
        track_id: u64,
        max_quality: AudioQuality,
    ) -> Result<ResolvedStream, StreamError> {
        // Stub: tidalt's ladder.
        let ladder = [
            AudioQuality::HiResLossless,
            AudioQuality::Lossless,
            AudioQuality::High,
            AudioQuality::Low,
        ];
        let path = format!("tracks/{track_id}/playbackinfopostpaywall");
        for quality in ladder.into_iter().filter(|q| *q <= max_quality) {
            let response = self
                .auth
                .get(
                    &path,
                    &[("audioquality", quality.tidal_name())],
                    Some(self.timeout),
                )
                .await;
            if let Ok(response) = response
                && response.status().is_success()
            {
                return parse_playback_info(response.body());
            }
        }
        Err(StreamError::NotAvailable)
    }
}

/// Parses a `playbackinfopostpaywall` body (AC2–AC4).
pub fn parse_playback_info(body: &[u8]) -> Result<ResolvedStream, StreamError> {
    let _ = (body, parse_bts, parse_mpd);
    Err(StreamError::Unsupported(Unsupported::Manifest(
        String::new(),
    )))
}

/// Parses a BTS manifest (AC2).
fn parse_bts(manifest: &[u8]) -> Result<StreamPlan, StreamError> {
    let _ = manifest;
    Err(StreamError::Unsupported(Unsupported::Manifest(
        String::new(),
    )))
}

/// Parses a DASH MPD (AC3).
fn parse_mpd(xml: &str) -> Result<StreamPlan, StreamError> {
    let _ = xml;
    Ok(StreamPlan::Segmented {
        init_url: String::new(),
        segments: Vec::new(),
        codec: Codec::Flac,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde_json::Value;

    const HIRES: &str = include_str!("../tests/fixtures/stream/playbackinfo_hires_dash.json");
    const LOSSLESS: &str = include_str!("../tests/fixtures/stream/playbackinfo_lossless_dash.json");
    const HIGH: &str = include_str!("../tests/fixtures/stream/playbackinfo_high_bts.json");
    const BTS_FLAC: &str =
        include_str!("../tests/fixtures/stream/playbackinfo_lossless_bts_flac.json");
    const MPD_HIRES: &str = include_str!("../tests/fixtures/stream/manifest_hires_24_96.mpd");
    const MPD_LOSSLESS: &str = include_str!("../tests/fixtures/stream/manifest_lossless_16_44.mpd");

    /// `fixture` with top-level fields replaced.
    fn edit(fixture: &str, fields: &[(&str, Value)]) -> Vec<u8> {
        let mut info: Value = serde_json::from_str(fixture).unwrap();
        for (key, value) in fields {
            info[*key] = value.clone();
        }
        serde_json::to_vec(&info).unwrap()
    }

    /// The decoded manifest of `fixture`.
    fn manifest(fixture: &str) -> String {
        let info: Value = serde_json::from_str(fixture).unwrap();
        let bytes = STANDARD.decode(info["manifest"].as_str().unwrap()).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    /// `fixture` with its manifest passed through `change`.
    fn edit_manifest(fixture: &str, change: impl Fn(String) -> String) -> Vec<u8> {
        let encoded = STANDARD.encode(change(manifest(fixture)));
        edit(fixture, &[("manifest", Value::String(encoded))])
    }

    /// The `HIGH` fixture with one field of its BTS manifest replaced.
    fn bts_with(field: &str, value: Value) -> Vec<u8> {
        edit_manifest(HIGH, |m| {
            let mut bts: Value = serde_json::from_str(&m).unwrap();
            bts[field] = value.clone();
            bts.to_string()
        })
    }

    fn unsupported(what: Unsupported) -> StreamError {
        StreamError::Unsupported(what)
    }

    #[test]
    fn ac2_bts_manifest() {
        let rows: Vec<(&str, Vec<u8>, Result<StreamPlan, StreamError>)> = vec![
            (
                "FLAC",
                BTS_FLAC.as_bytes().to_vec(),
                Ok(StreamPlan::Single {
                    url: "https://amz-pr-fa.audio.tidal.com/FAKE.flac?token=FAKE".into(),
                    codec: Codec::Flac,
                }),
            ),
            (
                "AAC-LC",
                HIGH.as_bytes().to_vec(),
                Ok(StreamPlan::Single {
                    url: "https://amz-pr-fa.audio.tidal.com/FAKE.mp4?token=FAKE".into(),
                    codec: Codec::AacLc,
                }),
            ),
            (
                "encrypted BTS",
                bts_with("encryptionType", "OLD_AES".into()),
                Err(unsupported(Unsupported::Encrypted)),
            ),
            (
                "encrypted DASH",
                edit_manifest(HIRES, |m| {
                    m.replace(
                        "<Role ",
                        r#"<ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" value="cenc"/><Role "#,
                    )
                }),
                Err(unsupported(Unsupported::Encrypted)),
            ),
            (
                "unknown manifest type",
                edit(
                    HIGH,
                    &[("manifestMimeType", "application/vnd.tidal.emu".into())],
                ),
                Err(unsupported(Unsupported::Manifest(
                    "application/vnd.tidal.emu".into(),
                ))),
            ),
        ];
        for (name, body, expected) in rows {
            let got = parse_playback_info(&body).map(|r| r.plan);
            assert_eq!(got, expected, "{name}");
        }

        for (name, body) in [
            ("no URL", bts_with("urls", serde_json::json!([]))),
            ("not base64", edit(HIGH, &[("manifest", "%%%".into())])),
            ("not JSON", edit_manifest(HIGH, |_| "nope".into())),
        ] {
            let got = parse_playback_info(&body);
            assert!(
                matches!(got, Err(StreamError::Malformed(_))),
                "{name}: {got:?}"
            );
        }
    }

    fn segments(plan: &StreamPlan) -> (&str, &[Segment], Codec) {
        match plan {
            StreamPlan::Segmented {
                init_url,
                segments,
                codec,
            } => (init_url, segments, *codec),
            other => panic!("not segmented: {other:?}"),
        }
    }

    /// Checks numbering, contiguity and the total of `segments`.
    fn assert_timeline(segments: &[Segment], first_number: u64, total_ticks: u64) {
        let mut next_start = segments.first().map_or(0, |s| s.start);
        for (i, segment) in segments.iter().enumerate() {
            assert_eq!(segment.number, first_number + i as u64, "number of #{i}");
            assert_eq!(segment.start, next_start, "start of #{i}");
            next_start += segment.duration;
        }
        let total: u64 = segments.iter().map(|s| s.duration).sum();
        assert_eq!(total, total_ticks);
    }

    #[test]
    fn ac3_dash_manifest() {
        // The recorded hi-res MPD: 73 × 380928 + 129030 ticks at 96 kHz.
        let plan = parse_mpd(MPD_HIRES).unwrap();
        let (init_url, segs, codec) = segments(&plan);
        assert_eq!(segs.len(), 74);
        assert_eq!(codec, Codec::Flac);
        assert_eq!(
            init_url,
            "https://sp-ad-fa.audio.tidal.com/mediatracks/FAKE/0.mp4?token=FAKE"
        );
        assert_timeline(segs, 1, 27_936_774);
        let url =
            |n| format!("https://sp-ad-fa.audio.tidal.com/mediatracks/FAKE/{n}.mp4?token=FAKE");
        assert_eq!(segs[0].url, url(1));
        assert_eq!(segs[1].url, url(2));
        assert_eq!(segs[73].url, url(74));
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[0].start_time(), Duration::ZERO);
        assert_eq!(segs[1].start_time(), Duration::from_millis(3968));
        assert_eq!(segs[72].duration, 380_928);
        assert_eq!(segs[73].start, 73 * 380_928);
        assert_eq!(segs[73].duration, 129_030);
        assert!(segs.iter().all(|s| s.timescale == 96_000));
        // = mediaPresentationDuration PT4M51.008S, to the millisecond.
        assert_eq!(segs[73].end_time().as_millis(), 291_008);

        // The recorded 16/44.1 MPD: `&amp;` in the signed URLs is unescaped.
        let plan = parse_mpd(MPD_LOSSLESS).unwrap();
        let (init_url, segs, _) = segments(&plan);
        let query = "Policy=FAKE-POLICY&Signature=FAKE-SIG&Key-Pair-Id=FAKE-KEY";
        assert_eq!(
            init_url,
            format!("https://sp-ad-cf.audio.tidal.com/mediatracks/FAKE/0.mp4?{query}")
        );
        assert_eq!(segs.len(), 57);
        assert_timeline(segs, 1, 56 * 176_128 + 52_930);
        assert_eq!(
            segs[56].url,
            format!("https://sp-ad-cf.audio.tidal.com/mediatracks/FAKE/57.mp4?{query}")
        );
        // = mediaPresentationDuration PT3M44.854S, to the millisecond.
        assert_eq!(segs[56].end_time().as_millis(), 224_854);

        // Hand-written: `t` on the first entry, repeats, startNumber 5.
        let mpd = r#"<MPD><Period><AdaptationSet><Representation codecs="flac">
            <SegmentTemplate timescale="10" initialization="i.mp4" media="s$Number$.m4s" startNumber="5">
              <SegmentTimeline><S t="100" d="20" r="2"/><S d="7"/><S d="3" r="1"/></SegmentTimeline>
            </SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
        let plan = parse_mpd(mpd).unwrap();
        let (init_url, segs, _) = segments(&plan);
        assert_eq!(init_url, "i.mp4");
        let got: Vec<(u64, &str, u64, u64)> = segs
            .iter()
            .map(|s| (s.number, s.url.as_str(), s.start, s.duration))
            .collect();
        assert_eq!(
            got,
            [
                (5, "s5.m4s", 100, 20),
                (6, "s6.m4s", 120, 20),
                (7, "s7.m4s", 140, 20),
                (8, "s8.m4s", 160, 7),
                (9, "s9.m4s", 167, 3),
                (10, "s10.m4s", 170, 3),
            ]
        );
        assert_eq!(segs[0].start_time(), Duration::from_secs(10));
    }

    #[test]
    fn ac4_validate_playbackinfo() {
        let errors: Vec<(&str, Vec<u8>, StreamError)> = vec![
            (
                "preview",
                edit(HIRES, &[("assetPresentation", "PREVIEW".into())]),
                StreamError::PreviewOnly,
            ),
            (
                "Dolby Atmos",
                edit(HIRES, &[("audioMode", "DOLBY_ATMOS".into())]),
                unsupported(Unsupported::AudioMode("DOLBY_ATMOS".into())),
            ),
            (
                "HE-AAC (LOW)",
                bts_with("codecs", "mp4a.40.5".into()),
                unsupported(Unsupported::Codec("mp4a.40.5".into())),
            ),
            (
                "HE-AAC v2",
                bts_with("codecs", "mp4a.40.29".into()),
                unsupported(Unsupported::Codec("mp4a.40.29".into())),
            ),
            (
                "HE-AAC in DASH",
                edit_manifest(HIRES, |m| {
                    m.replace(r#"codecs="flac""#, r#"codecs="mp4a.40.5""#)
                }),
                unsupported(Unsupported::Codec("mp4a.40.5".into())),
            ),
            (
                "unknown quality",
                edit(HIRES, &[("audioQuality", "HI_RES".into())]),
                unsupported(Unsupported::Quality("HI_RES".into())),
            ),
        ];
        for (name, body, expected) in errors {
            assert_eq!(parse_playback_info(&body), Err(expected), "{name}");
        }

        let grants = [
            (
                "hi-res",
                HIRES,
                1001,
                AudioQuality::HiResLossless,
                Some(24),
                Some(96_000),
                Codec::Flac,
            ),
            (
                "lossless",
                LOSSLESS,
                1002,
                AudioQuality::Lossless,
                Some(16),
                Some(44_100),
                Codec::Flac,
            ),
            (
                "high",
                HIGH,
                1002,
                AudioQuality::High,
                None,
                None,
                Codec::AacLc,
            ),
        ];
        for (name, body, track_id, quality, bits, rate, codec) in grants {
            let got = parse_playback_info(body.as_bytes());
            let got = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(got.track_id, track_id, "{name}");
            assert_eq!(got.quality, quality, "{name}");
            assert_eq!(got.bit_depth, bits, "{name}");
            assert_eq!(got.sample_rate, rate, "{name}");
            assert_eq!(got.plan.codec(), codec, "{name}");
        }
    }
}
