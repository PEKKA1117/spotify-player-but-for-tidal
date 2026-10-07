# Audio test fixtures (spec 0003)

Small files generated from sines with `ffmpeg` (6.1 here) and post-processed by
`fixture_tool.py` (python3). Tests only read them; they never call ffmpeg.
`generate.sh` holds the exact commands; run it from this directory to
regenerate everything (`sh generate.sh`).

| File | What | Used by |
|---|---|---|
| `flac16_44.flac` | FLAC 16-bit 44.1 kHz stereo, 1 s (440 Hz L, 660 Hz R), 4608-frame blocks. PADDING replaced by a SEEKTABLE with a point at every frame | AC7, engine tests, AC19 single-file row (seek jumps to a seek point's byte offset) |
| `flac16_44.s16le` | ffmpeg's decode of its first 1024 frames, raw `s16le` | AC7 left-justification |
| `mono16_44.flac` | FLAC 16-bit 44.1 kHz mono, 0.5 s, 550 Hz | AC9, gapless (AC17: same output format as `flac16_44`) |
| `mono16_44.s16le` | ffmpeg's decode of its first 1024 frames | AC9 |
| `flac24_96.flac` | FLAC 24-bit 96 kHz stereo, 0.6 s: the plain-FLAC reference | AC7, AC17 (format change), AC19 |
| `flac24_96.s24le` | ffmpeg's decode of its first 1024 frames, raw `s24le` | AC7 |
| `flac24_96_dash-init.mp4`, `flac24_96_dash-{1,2,3}.m4s` | the same FLAC stream (stream copy) in fragmented MP4, split into an init segment and media segments | AC7 (init + segments joined, non-seekable), AC19 segmented row |
| `flac24_96_dash-segments.txt` | the segments' timeline: `timescale 96000`, then `N start duration` in ticks | the fake segmented source |
| `aac_lc.m4a` | AAC-LC 128 kb/s stereo 44.1 kHz in MP4, `moov` first (faststart), 0.6 s | AC8 |
| `he_aac.m4a` | `aac_lc.m4a` with its AudioSpecificConfig patched to signal SBR (HE-AAC) | AC8 |

## How the DASH segments are made

`ffmpeg -movflags frag_keyframe+empty_moov+default_base_moof -frag_duration
250000` writes `ftyp`, an empty `moov`, then one `moof` + `mdat` pair per
~0.25 s fragment (each FLAC frame is a keyframe), then an `mfra` index.
`fixture_tool.py split-fmp4` cuts it at box boundaries: `ftyp` + `moov` is the
init segment, each `moof` + `mdat` pair a media segment, and the `mfra`
trailer is dropped (Tidal's segments have none). Segment durations are summed
from the `trun` sample durations (or the `tfhd`/`trex` defaults). This is the
shape the probe recorded for Tidal's hi-res DASH streams (init segment ~620
bytes, then `moof` + `mdat` segments), only with shorter segments.

## How the HE-AAC file is made

ffmpeg's native `aac` encoder cannot produce HE-AAC (no SBR) and this
ffmpeg has no `libfdk_aac`. ffmpeg's AAC-LC AudioSpecificConfig is 5 bytes:
LC, 44.1 kHz, stereo, then the backward-compatible sync extension `0x2b7`
with object type 5 (SBR) and `sbrPresentFlag = 0`. `fixture_tool.py he-aac`
sets `sbrPresentFlag = 1` and an extension sampling frequency index in the
padding bits of the same byte, so the file signals explicit SBR (HE-AAC)
without changing any box size. The audio is still LC, which does not matter:
the decoder must refuse the stream from its configuration, before decoding a
frame (symphonia: "aac: aac too complex"). A recording of a real `mp4a.40.5`
stream would test the same path.

Total size is about 125 KiB (the cap is 200 KiB).
