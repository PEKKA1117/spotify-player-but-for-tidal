//! Decode (spec 0003 "Decode"): symphonia over a [`TrackSource`], output as
//! interleaved stereo `i32`, left-justified (a 16-bit sample `s` is
//! `s << 16`). Mono is copied to both channels.
//!
//! The [`Decoder`] is synchronous: it reads the source from inside
//! symphonia's demuxer. The engine runs it on a worker thread (see
//! `engine.rs`) so a slow or stuck source never blocks the engine.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use symphonia::core::codecs::audio::well_known::{CODEC_ID_AAC, CODEC_ID_FLAC};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::{Error as SymError, SeekErrorKind};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{TimeBase, Timestamp};

use crate::engine::EngineError;
use crate::sink::{Codec, SourceFormat};
use crate::source::{ReadOutcome, SourceError, SourceLayout, TrackSource};

/// Decoded frames, interleaved stereo, left-justified `i32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Index (in the track) of the first frame of `samples`.
    pub first_frame: u64,
    pub samples: Vec<i32>,
}

impl Chunk {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
}

/// Result of [`Decoder::seek`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekOutcome {
    /// The next chunk starts exactly at the requested frame.
    Seeked,
    /// The requested frame is at or past the end of the track.
    PastEnd,
}

/// Flags shared between a decoder's reader and the engine (see
/// `engine.rs`).
#[derive(Debug, Default)]
pub(crate) struct Signals {
    /// The engine abandoned this track: give up any read.
    pub cancelled: AtomicBool,
    /// The last source read answered `Pending` (the fetcher is behind).
    pub stalled: AtomicBool,
    /// A seek is waiting: give up a read stuck on `Pending` so the worker
    /// can handle it.
    pub interrupt: AtomicBool,
    /// The source's own error, kept so it is reported as is (not as an
    /// I/O error from inside symphonia).
    pub error: Mutex<Option<SourceError>>,
}

type SharedSource = Arc<Mutex<Box<dyn TrackSource>>>;

fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Adapts a [`TrackSource`] to symphonia's `MediaSource` (`Read + Seek`).
///
/// Bytes below `cache.len()` come from memory: the stream's head, recorded
/// while the first probe read it, so that probing again for a seek fetches
/// nothing before the seek point. Seeks are lazy: the source is only
/// repositioned when a read needs bytes from elsewhere.
struct SourceReader {
    source: SharedSource,
    signals: Arc<Signals>,
    pos: u64,
    len: Option<u64>,
    seekable: bool,
    cache: Arc<[u8]>,
    /// Where the source's next read starts, when known.
    source_pos: Option<u64>,
    /// While probing for the first time: every byte read from offset 0 on.
    recording: Option<Arc<Mutex<Option<Vec<u8>>>>>,
}

impl SourceReader {
    fn new(source: &SharedSource, signals: &Arc<Signals>, layout: &SourceLayout) -> Self {
        let (seekable, len) = match layout {
            SourceLayout::SingleFile { len } => (true, *len),
            SourceLayout::Segmented { .. } => (false, None),
        };
        Self {
            source: source.clone(),
            signals: signals.clone(),
            pos: 0,
            len,
            seekable,
            cache: Arc::from(Vec::new()),
            source_pos: Some(0),
            recording: None,
        }
    }

    fn source_error(&self, e: SourceError) -> io::Error {
        let message = e.to_string();
        *lock(&self.signals.error) = Some(e);
        io::Error::other(message)
    }
}

impl Read for SourceReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(cached) = usize::try_from(self.pos)
            .ok()
            .and_then(|pos| self.cache.get(pos..))
            .filter(|rest| !rest.is_empty())
        {
            let n = cached.len().min(buf.len());
            buf[..n].copy_from_slice(&cached[..n]);
            self.pos += n as u64;
            return Ok(n);
        }
        if self.source_pos != Some(self.pos) {
            let pos = self.pos;
            let sought = lock(&self.source).seek(pos);
            sought.map_err(|e| self.source_error(e))?;
            self.source_pos = Some(pos);
        }
        loop {
            if self.signals.cancelled.load(Ordering::SeqCst) {
                return Err(io::Error::other("read abandoned"));
            }
            let outcome = lock(&self.source).read(buf);
            match outcome {
                Ok(ReadOutcome::Data(n)) => {
                    self.signals.stalled.store(false, Ordering::SeqCst);
                    let n = n.min(buf.len());
                    if let Some(recording) = &self.recording
                        && let Some(head) = lock(recording).as_mut()
                        && head.len() as u64 == self.pos
                    {
                        head.extend_from_slice(&buf[..n]);
                    }
                    self.pos += n as u64;
                    self.source_pos = Some(self.pos);
                    return Ok(n);
                }
                Ok(ReadOutcome::End) => {
                    self.signals.stalled.store(false, Ordering::SeqCst);
                    return Ok(0);
                }
                // The source waited a little already (its contract): ask
                // again, unless a seek wants the worker back. Only a read
                // stuck on a stall is abandoned; the demuxer is probed
                // afresh for the seek anyway.
                Ok(ReadOutcome::Pending) => {
                    self.signals.stalled.store(true, Ordering::SeqCst);
                    if self.signals.interrupt.load(Ordering::SeqCst) {
                        return Err(io::Error::other("read abandoned for a seek"));
                    }
                }
                Err(e) => return Err(self.source_error(e)),
            }
        }
    }
}

impl Seek for SourceReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Current(0) => return Ok(self.pos),
            _ if !self.seekable => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "segmented stream is not seekable",
                ));
            }
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => self.len.and_then(|len| len.checked_add_signed(d)),
        };
        self.pos = target.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad seek"))?;
        Ok(self.pos)
    }
}

impl MediaSource for SourceReader {
    fn is_seekable(&self) -> bool {
        self.seekable
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

/// A track being decoded.
pub struct Decoder {
    source: SharedSource,
    signals: Arc<Signals>,
    layout: SourceLayout,
    hint: Option<String>,
    /// The single-file stream's head, as the first probe read it.
    head: Arc<[u8]>,
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    format: SourceFormat,
    total_frames: Option<u64>,
    /// Added to packet times: the start of the segment a segmented stream
    /// was restarted at.
    frame_offset: u64,
    /// Frames before this one are dropped (accurate seek).
    skip_until: u64,
    /// Where the next packet must start; a gap means the demuxer skipped a
    /// corrupt frame, which fails the track (spec AC16) rather than
    /// playing on with a hole. `None` after opening or seeking.
    next_frame: Option<u64>,
    scratch: Vec<i32>,
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder")
            .field("format", &self.format)
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// What a probe found.
struct Opened {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    format: SourceFormat,
    num_frames: Option<u64>,
}

impl Decoder {
    /// Probe the container and set up the decoder. Fails with
    /// [`EngineError::Unsupported`] for anything but FLAC and AAC-LC in one
    /// or two channels (spec AC8), without decoding a frame.
    pub fn open(source: Box<dyn TrackSource>) -> Result<Self, EngineError> {
        Self::open_with(source, Arc::default())
    }

    pub(crate) fn open_with(
        source: Box<dyn TrackSource>,
        signals: Arc<Signals>,
    ) -> Result<Self, EngineError> {
        let layout = source.layout();
        let hint = source.extension_hint();
        let source: SharedSource = Arc::new(Mutex::new(source));
        let recording = Arc::new(Mutex::new(Some(Vec::new())));
        let mut reader = SourceReader::new(&source, &signals, &layout);
        reader.recording = Some(recording.clone());
        let opened = probe(reader, &signals, hint.as_deref())?;
        let head = Arc::from(lock(&recording).take().unwrap_or_default());
        let total_frames = match &layout {
            SourceLayout::Segmented {
                timescale,
                segments,
            } => segments
                .last()
                .map(|s| ticks_to_frames(s.end(), *timescale, opened.format.sample_rate)),
            SourceLayout::SingleFile { .. } => opened
                .num_frames
                .map(|n| ts_to_frames(n, opened.time_base, opened.format.sample_rate)),
        };
        Ok(Self {
            source,
            signals,
            layout,
            hint,
            head,
            reader: opened.reader,
            decoder: opened.decoder,
            track_id: opened.track_id,
            time_base: opened.time_base,
            format: opened.format,
            total_frames,
            frame_offset: 0,
            skip_until: 0,
            next_frame: None,
            scratch: Vec::new(),
        })
    }

    /// What the decoder found in the stream.
    pub fn format(&self) -> SourceFormat {
        self.format
    }

    /// The track's length in frames, when the stream says.
    pub fn total_frames(&self) -> Option<u64> {
        self.total_frames
    }

    /// The next decoded frames, or `None` at the end of the track.
    pub fn next_chunk(&mut self) -> Result<Option<Chunk>, EngineError> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(None),
                Err(e) => return Err(self.map_error(e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let first = self.frame_offset
                + ts_to_frames(
                    u64::try_from(packet.pts.get()).unwrap_or(0),
                    self.time_base,
                    self.format.sample_rate,
                );
            let decoded = self
                .decoder
                .decode(&packet)
                .map_err(|e| map_symphonia(e, &self.signals))?;
            let channels = decoded.spec().channels().count();
            self.scratch.clear();
            decoded.copy_to_vec_interleaved(&mut self.scratch);
            let frames = self.scratch.len() / channels.max(1);
            if let Some(expected) = self.next_frame
                && first > expected
            {
                return Err(EngineError::Decode(format!(
                    "corrupt stream: frames {expected}..{first} are missing"
                )));
            }
            self.next_frame = Some(first + frames as u64);
            if first + frames as u64 <= self.skip_until {
                continue;
            }
            let skip = self.skip_until.saturating_sub(first) as usize;
            let samples = match channels {
                1 => self.scratch[skip..].iter().flat_map(|&s| [s, s]).collect(),
                _ => self.scratch[skip * 2..].to_vec(),
            };
            return Ok(Some(Chunk {
                first_frame: first + skip as u64,
                samples,
            }));
        }
    }

    /// Continue from frame `frame` (accurate to the frame, spec AC19).
    ///
    /// A single-file stream seeks by byte offset through the container's
    /// index; a segmented one restarts at the segment that contains the
    /// frame, so no earlier segment is fetched.
    pub fn seek(&mut self, frame: u64) -> Result<SeekOutcome, EngineError> {
        if self.total_frames.is_some_and(|total| frame >= total) {
            return Ok(SeekOutcome::PastEnd);
        }
        let rate = self.format.sample_rate;
        match &self.layout {
            SourceLayout::SingleFile { .. } => {
                // A fresh demuxer (its head from memory): symphonia's FLAC
                // reader keeps stale parser state when a seek lands exactly
                // on a seek point, and a read abandoned on a stall leaves
                // the old one in an unknown state.
                let mut reader = SourceReader::new(&self.source, &self.signals, &self.layout);
                reader.cache = self.head.clone();
                reader.source_pos = None;
                self.reopen(reader)?;
                let ts = frames_to_ts(frame, self.time_base, rate);
                let to = SeekTo::Timestamp {
                    ts: Timestamp::new(i64::try_from(ts).unwrap_or(i64::MAX)),
                    track_id: self.track_id,
                };
                match self.reader.seek(SeekMode::Accurate, to) {
                    Ok(_) => {}
                    Err(SymError::SeekError(SeekErrorKind::OutOfRange)) => {
                        return Ok(SeekOutcome::PastEnd);
                    }
                    Err(e) => return Err(self.map_error(e)),
                }
                self.decoder.reset();
                self.frame_offset = 0;
            }
            SourceLayout::Segmented {
                timescale,
                segments,
            } => {
                let timescale = *timescale;
                let Some(index) = segments
                    .iter()
                    .rposition(|s| ticks_to_frames(s.start, timescale, rate) <= frame)
                else {
                    return Ok(SeekOutcome::PastEnd);
                };
                let start = ticks_to_frames(segments[index].start, timescale, rate);
                lock(&self.source)
                    .restart_at_segment(index)
                    .map_err(EngineError::Source)?;
                self.reopen(SourceReader::new(&self.source, &self.signals, &self.layout))?;
                self.frame_offset = start;
            }
        }
        self.skip_until = frame;
        self.next_frame = None;
        Ok(SeekOutcome::Seeked)
    }

    /// Probe again through `reader`: from the cached head of a single-file
    /// stream, or from the init segment of a restarted segmented one.
    fn reopen(&mut self, reader: SourceReader) -> Result<(), EngineError> {
        let opened = probe(reader, &self.signals, self.hint.as_deref())?;
        self.reader = opened.reader;
        self.decoder = opened.decoder;
        self.track_id = opened.track_id;
        self.time_base = opened.time_base;
        Ok(())
    }

    fn map_error(&self, e: SymError) -> EngineError {
        map_symphonia(e, &self.signals)
    }
}

fn probe(
    reader: SourceReader,
    signals: &Arc<Signals>,
    hint: Option<&str>,
) -> Result<Opened, EngineError> {
    let stream = MediaSourceStream::new(Box::new(reader), Default::default());
    let mut probe_hint = Hint::new();
    if let Some(ext) = hint {
        probe_hint.with_extension(ext);
    }
    let reader = symphonia::default::get_probe()
        .probe(
            &probe_hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| map_symphonia(e, signals))?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or_else(|| EngineError::Unsupported("no audio track".into()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| EngineError::Unsupported("not an audio track".into()))?;
    let codec = match params.codec {
        CODEC_ID_FLAC => Codec::Flac,
        CODEC_ID_AAC => Codec::AacLc,
        other => return Err(EngineError::Unsupported(format!("codec {other:?}"))),
    };
    // symphonia rejects hierarchical SBR signalling itself, but decodes a
    // backward-compatible one as its AAC-LC core (wrong rate, no SBR).
    if codec == Codec::AacLc && params.extra_data.as_deref().is_some_and(aac_signals_sbr) {
        return Err(EngineError::Unsupported("HE-AAC (SBR)".into()));
    }
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| map_symphonia(e, signals))?;
    // The decoder's parameters are amended from the codec configuration
    // (AAC's AudioSpecificConfig), so read the format from them.
    let decoded = decoder.codec_params();
    let sample_rate = decoded
        .sample_rate
        .or(params.sample_rate)
        .ok_or_else(|| EngineError::Unsupported("unknown sample rate".into()))?;
    let channels = decoded
        .channels
        .as_ref()
        .or(params.channels.as_ref())
        .map(|c| c.count())
        .ok_or_else(|| EngineError::Unsupported("unknown channel count".into()))?;
    if !(1..=2).contains(&channels) {
        return Err(EngineError::Unsupported(format!("{channels} channels")));
    }
    let bits_per_sample = match codec {
        Codec::Flac => Some(
            params
                .bits_per_sample
                .and_then(|b| u16::try_from(b).ok())
                .ok_or_else(|| EngineError::Unsupported("unknown FLAC bit depth".into()))?,
        ),
        Codec::AacLc => None,
    };
    Ok(Opened {
        track_id: track.id,
        time_base: track.time_base,
        num_frames: track.num_frames,
        format: SourceFormat {
            codec,
            sample_rate,
            channels: channels as u16,
            bits_per_sample,
        },
        reader,
        decoder,
    })
}

fn map_symphonia(e: SymError, signals: &Signals) -> EngineError {
    match e {
        SymError::IoError(io) => match lock(&signals.error).clone() {
            Some(source) => EngineError::Source(source),
            None if io.kind() == io::ErrorKind::UnexpectedEof => {
                EngineError::Decode("the stream ended unexpectedly".into())
            }
            None => EngineError::Source(SourceError::Other(io.to_string())),
        },
        SymError::Unsupported(what) => EngineError::Unsupported(what.into()),
        other => EngineError::Decode(other.to_string()),
    }
}

/// Whether an MPEG-4 AudioSpecificConfig signals SBR (HE-AAC, v1 or v2),
/// hierarchically (object type 5 or 29) or backward-compatibly (an AAC-LC
/// config followed by the `0x2b7` sync extension with `sbrPresentFlag`).
fn aac_signals_sbr(asc: &[u8]) -> bool {
    const SBR: u32 = 5;
    const PS: u32 = 29;
    const LC: u32 = 2;
    let mut bits = Bits { data: asc, pos: 0 };
    let object_type = |bits: &mut Bits| -> Option<u32> {
        match bits.read(5)? {
            31 => Some(32 + bits.read(6)?),
            aot => Some(aot),
        }
    };
    let parse = |bits: &mut Bits| -> Option<bool> {
        let aot = object_type(bits)?;
        if bits.read(4)? == 15 {
            bits.read(24)?;
        }
        let channel_config = bits.read(4)?;
        if aot == SBR || aot == PS {
            return Some(true);
        }
        if aot != LC || channel_config == 0 {
            return Some(false);
        }
        // GASpecificConfig: frameLengthFlag, dependsOnCoreCoder (+ delay),
        // extensionFlag (always 0 for AAC-LC).
        bits.read(1)?;
        if bits.read(1)? == 1 {
            bits.read(14)?;
        }
        bits.read(1)?;
        if bits.left() < 16 || bits.read(11)? != 0x2B7 {
            return Some(false);
        }
        match object_type(bits)? {
            SBR | PS => Some(bits.read(1)? == 1),
            _ => Some(false),
        }
    };
    parse(&mut bits).unwrap_or(false)
}

/// A most-significant-bit-first reader.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn left(&self) -> usize {
        (self.data.len() * 8).saturating_sub(self.pos)
    }

    fn read(&mut self, n: usize) -> Option<u32> {
        if n > self.left() {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..n {
            let bit = (self.data[self.pos / 8] >> (7 - self.pos % 8)) & 1;
            value = (value << 1) | u32::from(bit);
            self.pos += 1;
        }
        Some(value)
    }
}

/// Track timestamps (in `time_base` units) to frames at `rate`.
fn ts_to_frames(ts: u64, time_base: Option<TimeBase>, rate: u32) -> u64 {
    match time_base {
        Some(tb) => {
            let frames = u128::from(ts) * u128::from(tb.numer.get()) * u128::from(rate)
                / u128::from(tb.denom.get());
            u64::try_from(frames).unwrap_or(u64::MAX)
        }
        None => ts,
    }
}

/// Frames at `rate` to track timestamps (in `time_base` units).
fn frames_to_ts(frames: u64, time_base: Option<TimeBase>, rate: u32) -> u64 {
    match time_base {
        Some(tb) => {
            let ts = u128::from(frames) * u128::from(tb.denom.get())
                / (u128::from(tb.numer.get()) * u128::from(rate));
            u64::try_from(ts).unwrap_or(u64::MAX)
        }
        None => frames,
    }
}

fn ticks_to_frames(ticks: u64, timescale: u32, rate: u32) -> u64 {
    if timescale == 0 {
        return 0;
    }
    let frames = u128::from(ticks) * u128::from(rate) / u128::from(timescale);
    u64::try_from(frames).unwrap_or(u64::MAX)
}

/// Decode a whole track: its format and every sample (interleaved stereo).
pub fn decode_all(source: Box<dyn TrackSource>) -> Result<(SourceFormat, Vec<i32>), EngineError> {
    let mut decoder = Decoder::open(source)?;
    let mut samples = Vec::new();
    while let Some(chunk) = decoder.next_chunk()? {
        samples.extend_from_slice(&chunk.samples);
    }
    Ok((decoder.format(), samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC8: HE-AAC is recognised from its AudioSpecificConfig, however it is
    /// signalled.
    #[test]
    fn ac8_aac_signals_sbr() {
        let cases: [(&str, &[u8], bool); 7] = [
            ("AAC-LC 44.1 kHz stereo", &[0x12, 0x10], false),
            // ffmpeg's LC config: sync extension 0x2b7, SBR, sbrPresentFlag 0.
            (
                "LC + explicit no-SBR",
                &[0x12, 0x10, 0x56, 0xE5, 0x00],
                false,
            ),
            (
                "LC + backward-compatible SBR",
                &[0x12, 0x10, 0x56, 0xE5, 0x98],
                true,
            ),
            // Object type 5, 22.05 kHz core, stereo, 44.1 kHz, then LC.
            ("hierarchical SBR", &[0x2B, 0x92, 0x08, 0x00], true),
            // Object type 29 (PS), mono core.
            ("hierarchical PS", &[0xEB, 0x8A, 0x08, 0x00], true),
            ("truncated", &[0x12], false),
            ("empty", &[], false),
        ];
        for (name, asc, sbr) in cases {
            assert_eq!(aac_signals_sbr(asc), sbr, "{name}");
        }
    }

    #[test]
    fn timestamp_conversions() {
        let tb = TimeBase::try_new(1, 96_000);
        assert_eq!(ts_to_frames(24_576, tb, 96_000), 24_576);
        assert_eq!(frames_to_ts(24_576, tb, 96_000), 24_576);
        let tb = TimeBase::try_new(1, 1_000);
        assert_eq!(ts_to_frames(500, tb, 44_100), 22_050);
        assert_eq!(frames_to_ts(22_050, tb, 44_100), 500);
        assert_eq!(ts_to_frames(7, None, 44_100), 7);
        assert_eq!(ticks_to_frames(176_128, 44_100, 44_100), 176_128);
        assert_eq!(ticks_to_frames(48_000, 96_000, 44_100), 22_050);
        assert_eq!(ticks_to_frames(1, 0, 44_100), 0);
    }
}
