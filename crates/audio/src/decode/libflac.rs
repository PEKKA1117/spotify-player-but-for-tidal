//! FLAC frames decoded by libFLAC (spec 0003 "Decode", AC32): symphonia's
//! FLAC decoder gets 32-bit frames with stereo decorrelation wrong, so it
//! only demuxes and every FLAC frame comes here.
//!
//! libFLAC's stream decoder pulls bytes through a read callback and pushes
//! decoded blocks through a write callback. [`FlacDecoder`] hands it one
//! frame at a time: the read callback serves that frame's bytes, then
//! reports the end of the stream, and one `process_single` call decodes it.
//!
//! The only module of the crate with `unsafe`: the libFLAC calls and the
//! three callbacks.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;

use libflac_sys as ffi;

/// The size of a `STREAMINFO` metadata block's body.
const STREAMINFO_LEN: usize = 34;

/// What the callbacks share with [`FlacDecoder`]. libFLAC keeps a pointer to
/// it, so it lives on the heap and is only reached through that pointer.
#[derive(Default)]
struct State {
    /// The bytes libFLAC may read: the stream head, then one frame.
    input: Vec<u8>,
    /// How many of them it has read.
    read: usize,
    /// The decoded block: interleaved, left-justified.
    samples: Vec<i32>,
    channels: usize,
    /// The write callback ran since the last frame was handed over.
    wrote: bool,
    /// What the error or write callback refused.
    error: Option<&'static str>,
}

/// One libFLAC stream decoder, fed frame by frame.
pub struct FlacDecoder {
    raw: NonNull<ffi::FLAC__StreamDecoder>,
    /// From `Box::into_raw`; freed in `drop`, after the decoder.
    state: *mut State,
}

// SAFETY: a libFLAC decoder has no thread affinity, and both pointers are
// owned by this value alone, which is used from one thread at a time.
unsafe impl Send for FlacDecoder {}

impl std::fmt::Debug for FlacDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlacDecoder").finish_non_exhaustive()
    }
}

impl FlacDecoder {
    /// A decoder for the stream whose `STREAMINFO` block body is
    /// `streaminfo` (symphonia's codec extra data).
    pub fn new(streaminfo: &[u8]) -> Result<Self, String> {
        if streaminfo.len() != STREAMINFO_LEN {
            return Err(format!(
                "FLAC STREAMINFO is {} bytes, not {STREAMINFO_LEN}",
                streaminfo.len()
            ));
        }
        // SAFETY: no arguments; a null result is handled.
        let raw = NonNull::new(unsafe { ffi::FLAC__stream_decoder_new() })
            .ok_or("libFLAC: out of memory")?;
        let state = Box::into_raw(Box::<State>::default());
        // From here on `drop` frees both.
        let mut decoder = Self { raw, state };

        // The head of a FLAC stream: the marker, then STREAMINFO as the
        // last metadata block (type 0, 24-bit length).
        let head = decoder.state();
        head.input.extend_from_slice(b"fLaC");
        head.input
            .extend_from_slice(&[0x80, 0, 0, STREAMINFO_LEN as u8]);
        head.input.extend_from_slice(streaminfo);

        // SAFETY: `raw` is a new decoder; the callbacks match libFLAC's
        // signatures and `state` outlives the decoder (see `drop`).
        let status = unsafe {
            ffi::FLAC__stream_decoder_init_stream(
                raw.as_ptr(),
                Some(read_callback),
                None,
                None,
                None,
                None,
                Some(write_callback),
                None,
                Some(error_callback),
                state.cast::<c_void>(),
            )
        };
        if status != ffi::FLAC__STREAM_DECODER_INIT_STATUS_OK {
            return Err(format!("libFLAC: init failed ({status})"));
        }
        // SAFETY: the decoder is initialised; no reference to `state` is
        // alive while libFLAC runs the callbacks.
        let ok = unsafe { ffi::FLAC__stream_decoder_process_until_end_of_metadata(raw.as_ptr()) };
        if let Some(error) = decoder.state().error.take() {
            return Err(format!("libFLAC: {error} in STREAMINFO"));
        }
        if ok == 0 {
            return Err("libFLAC: STREAMINFO was refused".into());
        }
        Ok(decoder)
    }

    /// Decode one whole FLAC frame into `out` (replaced): interleaved,
    /// left-justified samples. Returns the frame's channel count. After an
    /// error the decoder is only good for [`Self::reset`].
    pub fn decode(&mut self, frame: &[u8], out: &mut Vec<i32>) -> Result<usize, String> {
        let state = self.state();
        state.input.clear();
        state.input.extend_from_slice(frame);
        state.read = 0;
        state.samples.clear();
        state.wrote = false;
        state.error = None;

        // SAFETY: the decoder is initialised; no reference to `state` is
        // alive while libFLAC runs the callbacks.
        let ok = unsafe { ffi::FLAC__stream_decoder_process_single(self.raw.as_ptr()) };
        let state = self.state();
        if let Some(error) = state.error.take() {
            return Err(format!("libFLAC: {error}"));
        }
        if ok == 0 || !state.wrote {
            return Err("libFLAC: the frame is incomplete".into());
        }
        std::mem::swap(out, &mut state.samples);
        Ok(state.channels)
    }

    /// Forget the stream position: the next frame may be any frame.
    pub fn reset(&mut self) {
        // SAFETY: the decoder is initialised.
        unsafe { ffi::FLAC__stream_decoder_flush(self.raw.as_ptr()) };
        let state = self.state();
        state.input.clear();
        state.read = 0;
        state.error = None;
    }

    fn state(&mut self) -> &mut State {
        // SAFETY: `state` is valid until `drop`, and libFLAC only uses it
        // inside the calls above, none of which is running here.
        unsafe { &mut *self.state }
    }
}

impl Drop for FlacDecoder {
    fn drop(&mut self) {
        // SAFETY: `raw` and `state` are owned by this value and not used
        // again; the decoder goes first, since it points to the state.
        unsafe {
            ffi::FLAC__stream_decoder_finish(self.raw.as_ptr());
            ffi::FLAC__stream_decoder_delete(self.raw.as_ptr());
            drop(Box::from_raw(self.state));
        }
    }
}

/// Serves what is left of `State::input`, then the end of the stream.
unsafe extern "C" fn read_callback(
    _decoder: *const ffi::FLAC__StreamDecoder,
    buffer: *mut ffi::FLAC__byte,
    bytes: *mut usize,
    client_data: *mut c_void,
) -> ffi::FLAC__StreamDecoderReadStatus {
    // SAFETY: `client_data` is the `State` given to `init_stream`, with no
    // other reference alive; libFLAC passes a buffer of `*bytes` bytes.
    unsafe {
        let state = &mut *client_data.cast::<State>();
        let rest = state.input.get(state.read..).unwrap_or_default();
        let n = rest.len().min(*bytes);
        *bytes = n;
        if n == 0 {
            return ffi::FLAC__STREAM_DECODER_READ_STATUS_END_OF_STREAM;
        }
        std::ptr::copy_nonoverlapping(rest.as_ptr(), buffer, n);
        state.read += n;
    }
    ffi::FLAC__STREAM_DECODER_READ_STATUS_CONTINUE
}

/// Interleaves and left-justifies the decoded block into `State::samples`.
unsafe extern "C" fn write_callback(
    _decoder: *const ffi::FLAC__StreamDecoder,
    frame: *const ffi::FLAC__Frame,
    buffer: *const *const ffi::FLAC__int32,
    client_data: *mut c_void,
) -> ffi::FLAC__StreamDecoderWriteStatus {
    // SAFETY: `client_data` as in `read_callback`; `frame` is valid for the
    // call and `buffer` holds `channels` arrays of `blocksize` samples.
    unsafe {
        let state = &mut *client_data.cast::<State>();
        let header = &(*frame).header;
        let (channels, frames) = (header.channels as usize, header.blocksize as usize);
        if !(1..=2).contains(&channels) || !(1..=32).contains(&header.bits_per_sample) {
            state.error = Some("unsupported frame");
            return ffi::FLAC__STREAM_DECODER_WRITE_STATUS_ABORT;
        }
        let shift = 32 - header.bits_per_sample;
        let left = std::slice::from_raw_parts(*buffer, frames);
        state.samples.reserve(frames * channels);
        if channels == 2 {
            let right = std::slice::from_raw_parts(*buffer.add(1), frames);
            for (l, r) in left.iter().zip(right) {
                state.samples.extend([l << shift, r << shift]);
            }
        } else {
            state.samples.extend(left.iter().map(|s| s << shift));
        }
        state.channels = channels;
        state.wrote = true;
    }
    ffi::FLAC__STREAM_DECODER_WRITE_STATUS_CONTINUE
}

/// Records why libFLAC refused the frame.
unsafe extern "C" fn error_callback(
    _decoder: *const ffi::FLAC__StreamDecoder,
    status: ffi::FLAC__StreamDecoderErrorStatus,
    client_data: *mut c_void,
) {
    let error = match status {
        // A frame number that does not follow the last one: every seek
        // does that, and `Decoder` checks the frames' positions itself.
        ffi::FLAC__STREAM_DECODER_ERROR_STATUS_MISSING_FRAME => return,
        ffi::FLAC__STREAM_DECODER_ERROR_STATUS_LOST_SYNC => "lost sync",
        ffi::FLAC__STREAM_DECODER_ERROR_STATUS_BAD_HEADER => "bad frame header",
        ffi::FLAC__STREAM_DECODER_ERROR_STATUS_FRAME_CRC_MISMATCH => "frame CRC mismatch",
        ffi::FLAC__STREAM_DECODER_ERROR_STATUS_UNPARSEABLE_STREAM => "unparseable stream",
        ffi::FLAC__STREAM_DECODER_ERROR_STATUS_BAD_METADATA => "bad metadata",
        ffi::FLAC__STREAM_DECODER_ERROR_STATUS_OUT_OF_BOUNDS => "sample out of bounds",
        _ => "decode error",
    };
    // SAFETY: `client_data` as in `read_callback`.
    unsafe {
        let state = &mut *client_data.cast::<State>();
        state.error.get_or_insert(error);
    }
}
