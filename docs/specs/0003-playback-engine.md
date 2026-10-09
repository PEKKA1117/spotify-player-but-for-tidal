# 0003 — Playback engine: stream resolution, decode, ALSA output

- **Status**: implemented (2026-10-07; the manual checks under "Test plan" are run on the user's machine)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0001 (implemented), 0002 (implemented; AC18 and AC19 are added by this work, see 0002 "Bugs")
- **User docs**: [`docs/playback.md`](../playback.md) (written by this spec's implementation, AC27)

## Context

This spec makes the player produce sound: resolve a Tidal track to a stream, decode it, and send it to an ALSA device, bit-perfect when the device allows it. It gives the engine the primitives that later specs build on (play, preload the next track, pause, resume, seek, stop, switch device). It does not cover the queue, the player state machine, TUI controls or the daemon: those are 0004 and 0005. Until they exist, a headless `tidal-player play <track-id>` command drives the engine so it can be accepted on real hardware.

### What tidalt did

Read from `internal/tidal/api.go` and `internal/player/{mpv.go,alsa.c,avcodec.c,shared.go}`, and the history of its `fix(player)` commits:

- **Resolution**: `GET /v1/tracks/{id}/urlpostpaywall?audioquality=Q&urlusagemode=STREAM&assetpresentation=FULL`, trying `HI_RES_LOSSLESS → LOSSLESS → HIGH → LOW` and taking the first `200`. It played whatever single URL came back and guessed the format from its file extension. Per the probe, that endpoint refuses `HI_RES_LOSSLESS` for this client, so tidalt never got hi-res. It did get 16/44.1 FLAC at `LOSSLESS` until about May 2026 (its FLAC-only decoder played music from 2026-03-13; commit 0573cda on 2026-05-09 notes Tidal returning AAC at `LOSSLESS`), when Tidal started answering `LOSSLESS` on this client ID with AAC `HIGH` (python-tidal issue #404, 2026-04-27). Since then it got AAC 320 for CD-quality tracks
- **Decode**: FFmpeg via CGO (libavformat/libavcodec/libswresample), fed from the HTTP body through a custom AVIO callback, always converted to S32LE. This was the build and packaging burden that spec 0001 removed
- **Output**: ALSA `hw:` opened directly, format negotiated per device (16-bit sources: `S32_LE → S16_LE → S24_3LE → S24_LE`; 24-bit sources: `S24_3LE → S24_LE → S32_LE`), the card claimed from PipeWire with `org.freedesktop.ReserveDevice1`, a `plughw:` fallback only when format negotiation is refused (never on a busy device), memoised per device. A "shared" mode (`default` PCM) skipped the reservation and used a bigger buffer. Buffer: 1024-frame periods, 4 periods on `hw:`, 8 when shared
- **Timing budgets**: `RequestRelease` call 500 ms, settle 200 ms, `EBUSY` open retries 800 ms at 100 ms intervals, shutdown wait 3 s

### What went wrong there, and the criterion that covers it here

1. **The quality ladder treated every error as "this quality is unavailable".** A timeout or a `5xx` on `HI_RES_LOSSLESS` silently played the track in `LOW`. And `urlpostpaywall` never grants `HI_RES_LOSSLESS` to this client (`401`/`4005`, probe), so tidalt could never play hi-res at all → AC1, AC5
2. **An aborted playback looked like a finished track.** Before fix #8 an error mid-track closed the "done" channel, so the UI auto-advanced into the same broken state, track after track → AC16
3. **Two playback loops could run at once** when the old one did not stop within its timeout, fighting over the device and the reservation (fix #8), and a pause → skip path dereferenced a closed PCM handle (SIGSEGV) → the engine is one thread that alone owns the device and handles commands one at a time (AC15, AC22)
4. **A click at every auto-advance** because the PCM was not drained at end of track (fix #13). And the fix drained between tracks of the same format, so playback was "silent-gap" rather than gapless → AC17
5. **Silent playback on fixed-format interfaces** (Focusrite Vocaster) until the `plughw:` fallback (#7) → AC12
6. **Volume was applied by scaling every sample** while the UI kept saying "bit-perfect" → this spec defines `bit_perfect` precisely (AC12, AC15); volume itself is 0004, which must clear the flag whenever gain ≠ 1
7. **The rate was set "nearest"** (`snd_pcm_hw_params_set_rate_near`, tidalt `internal/player/alsa.c:72`): a device without the source rate could open at another one and play through a hidden resampler or at the wrong speed → AC12 (exact rate on `hw:`, read back after `hw_params`; a mismatch counts as refused and goes to the fallback, which is reported)
8. **Seek re-downloaded the stream from byte 0 and decoded everything up to the target**, so a seek near the end of a hi-res track took seconds → AC19
9. **Pausing lost the audio still in the device buffer** (the PCM was dropped and closed on pause), so resume skipped ahead by up to a buffer → AC18 (releasing the device while paused is 0005's; it must not lose frames either)
10. **Positions came from "frames written"**, which runs ahead of what the listener hears by the device buffer → AC20

## Behaviour

### Commands

| Command | What it does |
|---|---|
| `tidal-player play <track-id> [--quality Q] [--device PCM] [--start SECONDS]` | Resolves and plays one track in the foreground, headless (no TUI), and exits when it ends. A permanent CLI command (decision 3); also how the engine is accepted before 0004 adds the queue and the TUI controls. Needs a stored session (spec 0002); without one it exits 1 with `Not logged in: run "tidal-player login"` |
| `tidal-player devices` | Lists the playback devices: `default` (shared) first, then every `hw:C,D` with a playback stream, with the card's name. The device that `play` would use is marked `*` |

`play` prints, then a progress line that redraws in place when stdout is a terminal (and nothing more when it is not):

```
Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo
Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect
  1:23 / 4:56
```

- The first line gives the quality **Tidal granted** (it may be lower than asked, AC5) and the source format the decoder found
- The second line gives the device actually opened, its kind, the negotiated sample format, and `bit-perfect` or the reason it is not: `resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)`, `shared (system mixer)`, `lossy source`
- Duration shows `?:??` when the stream does not say
- Exit codes: `0` the track played to its end; `130` Ctrl-C (the device is closed and released first); `1` any error, with one line on stderr (examples under "Edge cases & errors"); `2` bad arguments

### Settings (until spec 0008 adds `app.toml`)

| Setting | Flag | Environment | Default |
|---|---|---|---|
| Highest quality to ask for | `--quality` (`hi-res`, `lossless`, `high`) | `TIDAL_PLAYER_QUALITY` | `hi-res` |
| Output device | `--device` (any ALSA PCM name) | `TIDAL_PLAYER_DEVICE` | `default` (shared; decision 2) |

Flag beats environment beats default. 0008 moves both into `app.toml` and keeps the variables working.

### Output kinds

| Device given | Kind | Reservation | Resampling | Buffer | `bit_perfect` can be true |
|---|---|---|---|---|---|
| `hw:C,D` (or `hw:C`) | **exclusive** | ReserveDevice1 on card C | off (stated explicitly) | 4 × 1024-frame periods | yes |
| `hw:C,D` whose format negotiation was refused | **fallback** → opened as `plughw:C,D` | as exclusive | on | 4 periods | no |
| `plughw:…` given by the user | **fallback** | as exclusive | on | 4 periods | no |
| `default` or any other name | **shared** | none | on | 8 periods | no |

`bit_perfect` is true iff: the kind is exclusive, the source is lossless (FLAC), the device runs at the source's exact rate and channel count, and the negotiated sample format holds at least the source's bits (so conversion is zero-padding only). 0004 adds "and the gain is 1".

### Stream resolution

`GET {api_base}/tracks/{id}/playbackinfopostpaywall?audioquality=Q&playbackmode=STREAM&assetpresentation=FULL&countryCode={session country}` through the `Authenticator` (spec 0002). The response (shape assumed, see "Facts") carries the granted `audioQuality`, `assetPresentation`, `audioMode`, `bitDepth`, `sampleRate`, `manifestMimeType` and a base64 `manifest`:

- `application/vnd.tidal.bts`: JSON `{mimeType, codecs, encryptionType, urls: [..]}` → a **single-file** stream (`urls[0]`)
- `application/dash+xml`: an MPEG-DASH MPD with one audio `Representation` whose `SegmentTemplate` has `initialization`, `media` (with `$Number$`), `startNumber`, `timescale` and a `SegmentTimeline` → a **segmented** stream: the init segment, then every media segment with its start time and duration
- anything else, `encryptionType` other than `NONE`, `assetPresentation` other than `FULL`, `audioMode` other than `STEREO`, or a codec outside the table below → a `StreamError` naming what was unsupported. Nothing is downloaded

Supported codecs: FLAC (`flac`, in a raw FLAC file or in (fragmented) MP4) and AAC-LC (`mp4a.40.2`, in MP4). HE-AAC (`mp4a.40.5`, `mp4a.40.29`) is **not** supported (symphonia rejects SBR, see "Facts").

**Quality**: one request, at the configured highest quality. Tidal downgrades by itself: asked for `HI_RES_LOSSLESS` on a CD-quality track it answers `200` with `audioQuality: HIGH` (probe, 2026-10-07), so there is no client-side ladder to walk. The granted quality is taken as is and shown to the user. Failures are returned as is, with no second request:

| Response | Result |
|---|---|
| `401` with `subStatus` `4005` ("Asset is not ready for playback") | `StreamError::NotAvailable`. This is **not** an auth failure: the `Authenticator` must not refresh the token for it (spec 0002, Bugs) |
| `500` with `subStatus` `999` (Tidal's answer for an unknown track ID, and for a track that cannot be streamed in the country) | `StreamError::NotFound` |
| other `500` | `StreamError::Server(500)` |
| other `5xx`, `429`, transport error, timeout | the transient error, unchanged |
| `LoginRequired` from the `Authenticator` | unchanged |

`LOW` is HE-AAC (`mp4a.40.5`, probe), which symphonia cannot decode, so `low` is not accepted as a setting (decision 4).

### Fetching

The bytes are fetched on a separate fetch thread that reads ahead into a bounded buffer (target 10 s of audio, at most 16 MiB), so the audio thread never waits on the network as long as the buffer holds:

- Single-file streams use HTTP `Range` requests; seeking jumps by byte offset (FLAC seek table / MP4 sample table, via symphonia)
- Segmented streams fetch the init segment, then the media segments in order; a seek restarts at the segment that contains the target, so no earlier segment is fetched
- A transport error or a body cut short is retried from the exact byte offset reached, 3 times, after 0.5 s, 1 s and 2 s. Then the track fails with `SourceError::Network`
- A `403` or `410` on a stream URL (signed URLs expire, see "Facts") re-resolves the track **once** and continues from the same offset or segment in the new plan, as long as it reports the same codec, rate and bit depth. Otherwise the track fails

### Decode

symphonia 0.6, pure Rust. The decoder output is interleaved `i32`, left-justified (a 16-bit sample `s` becomes `s << 16`), which is the `Sink` contract from 0001. The source format (rate, channels, bits per sample; lossy has no bits) comes from the codec parameters. Mono sources are copied to both channels, sample for sample, so the output is always stereo (Tidal's mono tracks then stay bit-perfect on DACs that refuse one channel).

### Output (ALSA)

- **Exclusive** (`hw:`): acquire the reservation (below), open the PCM, retrying `EBUSY` from the open itself for 800 ms at 100 ms intervals, then negotiate: interleaved access, the **exact** source rate and 2 channels, and the first sample format in the preference list for the source's bits that the device accepts:
  - 16-bit source: `S32_LE → S16_LE → S24_3LE → S24_LE` (carried over: some DACs have a broken `S16_LE` endpoint)
  - 24-bit source: `S24_3LE → S24_LE → S32_LE`
  - lossy source: `S32_LE → S24_3LE → S24_LE → S16_LE` (not bit-perfect anyway; keeps the most of the decoder's precision)

  If negotiation is refused (no listed format, rate or channel count accepted), close, and open `plughw:` on the same card (kind *fallback*). Remember that per device for the rest of the process, so later tracks skip the failing attempt. A busy device (still `EBUSY` after the budget, or a reservation refused or timed out) fails with `OutputError::Busy` and is **never** downgraded
- **Shared**: open the name as given, no reservation, resampling on, `S32_LE` first then the same list as exclusive
- Samples are packed from left-justified `i32` into the negotiated format by dropping low bits only (`S16_LE`: `>> 16`; `S24_LE`, `S24_3LE`: `>> 8`; `S32_LE`: as is). For a source whose bits fit the format, this is lossless
- Underrun (`EPIPE`) and suspend (`ESTRPIPE`) are recovered (`snd_pcm_recover`) and playback continues; each is counted and logged at `warn`. Any other write error (device unplugged: `ENODEV`, `EIO`) fails the track with `OutputError::Lost`
- **Reservation** (`org.freedesktop.ReserveDevice1.Audio{C}`, on the session bus), as tidalt did:

  | Situation | Result |
  |---|---|
  | No session bus | Proceed without a reservation |
  | Name has no owner (the `RequestRelease` call errors with "no owner") | Claim the name, proceed |
  | Owner replies `true` | Wait 200 ms (settle), claim the name (`ReplaceExisting`, `AllowReplacement`), proceed |
  | Owner replies `false` | `OutputError::Busy` ("in use by <owner's application name, if known>") |
  | No reply within 500 ms | `OutputError::Busy` (never steal the name from a slow owner) |

  The name is released when the engine closes the device. Answering another application's `RequestRelease` is out of scope (0005); until then the engine refuses it while playing

### Engine

`tidal-player-audio` runs the engine on one dedicated OS thread. It owns the decoder and the output device; nothing else opens the device. It takes commands from a channel and reports events on another. Commands are handled one at a time, in order:

| Command | Effect |
|---|---|
| `Play { source, start_at }` | Stop whatever is playing, then open, decode and play `source` from `start_at` |
| `Preload { source }` | Open `source` as the next track, ready for a gapless transition. Replaces any earlier preload |
| `Pause` / `Resume` | Stop / restart output. No frame is lost or repeated; the device stays open (releasing it while paused is 0005) |
| `Seek(position)` | Continue from `position` (accurate to the frame). Keeps the paused/playing state |
| `SetDevice(device)` | Use `device` from now on; a playing track moves to it at its current position |
| `Stop` | Stop, close the device, release the reservation, forget any preload |
| `Shutdown` | `Stop`, then end the thread |

| Event | When |
|---|---|
| `Started { source: SourceFormat, output: OutputInfo }` | The first frame of a track has been written |
| `Position(Duration)` | At least every 250 ms while playing, and once after each seek, pause and resume. The position is what the listener hears: frames written minus the device's reported delay |
| `Buffering` / `Buffered` | The fetch buffer ran dry / refilled to 2 s of audio. Nothing is written in between; the position does not move |
| `Transitioned { source, output }` | Playback moved into the preloaded track (gapless or not) |
| `TrackEnded` | The last frame of the track has been **played** (drained) and no preload was waiting. Only for a natural end |
| `Paused` / `Resumed` / `Stopped` | After the matching command took effect |
| `Error(EngineError)` | The track failed (source, decode, output, unsupported). The engine is then idle and ready for the next command. `TrackEnded` never follows an `Error` for the same track |

End of track:

- **Preload waiting, same output format** (rate, channels, sample format): the next track's first frame is written right after the current track's last one, with no drain, no reopen and no inserted silence (gapless, AC17)
- **Preload waiting, different format**: drain, reopen with the new format, continue (a gap is expected)
- **No preload**: drain, `TrackEnded`, close the device and release the reservation

`OutputInfo` holds the requested device, the device actually opened, the kind, the sample format, rate and channel count, and `bit_perfect` with a reason when false. `SourceFormat` holds codec, rate, channels and bits (`None` for lossy); the granted Tidal quality comes from the resolved stream and is joined to it by the caller (the audio crate does not know Tidal's tiers).

## Acceptance criteria

Stream resolution (`tidal-player-api::stream`, `tidal-player-core`):

- **AC1** — `resolve_stream(track_id, max_quality)` sends `GET {api_base}/tracks/{id}/playbackinfopostpaywall` with exactly the query parameters `audioquality`, `playbackmode=STREAM`, `assetpresentation=FULL` and `countryCode` from the session, through the `Authenticator` (bearer header)
- **AC2** — A BTS manifest maps to `StreamPlan::Single { url, codec }` (table over fixtures: FLAC, AAC-LC). `encryptionType` other than `NONE` → `StreamError::Unsupported(Encrypted)`; an unknown `manifestMimeType` → `Unsupported(Manifest(mime))`
- **AC3** — A DASH manifest maps to `StreamPlan::Segmented { init_url, segments, codec }`, where `segments` lists every media URL in order, with `$Number$` filled in from `startNumber`, and each segment's start time and duration taken from the `SegmentTimeline` (`t`, `d`, repeats `r`) and `timescale`. The sum of durations equals the timeline's total. Fixture: the MPD shape recorded by the probe (`timescale` 96000, `startNumber` 1, `<S d="380928" r="72"/><S d="129030"/>` without `t`, so the first segment starts at 0) gives 74 segments, numbered 1–74, totalling 27 936 774 ticks = `PT4M51.008S`, the MPD's `mediaPresentationDuration`
- **AC4** — `assetPresentation` other than `FULL` → `StreamError::PreviewOnly`; `audioMode` other than `STEREO` → `Unsupported(AudioMode)`; codec `mp4a.40.5` or `mp4a.40.29` → `Unsupported(Codec)`; the plan carries the granted quality, and the source's bit depth and sample rate when the response has them
- **AC5** — Quality: `resolve_stream` sends exactly **one** request per call, at the configured highest quality, whatever the outcome; a `200` with a lower `audioQuality` gives a plan with that granted quality; the failures map as in the table under "Quality" (`401`/`4005` → `NotAvailable` with **no** token refresh request; `500`/`999` → `NotFound`; `503`, `429`, timeout and `LoginRequired` unchanged)
- **AC6** — `tidal_player_core::AudioQuality` has `Low < High < Lossless < HiResLossless`, serialises as Tidal's names (`LOW`, …, `HI_RES_LOSSLESS`), and parses the CLI names `high`, `lossless`, `hi-res` (`low` is rejected with a message saying LOW streams are HE-AAC, which is not supported)

Decode (`tidal-player-audio`):

- **AC7** — Decoding these fixtures yields the same samples as decoding the plain-FLAC fixture made from the same PCM, and reports the right source format: FLAC 16-bit 44.1 kHz (raw FLAC); FLAC 24-bit 96 kHz in fragmented MP4, read as init segment + media segments joined in order from a non-seekable reader. Samples are left-justified (`decoded == pcm << (32 - bits)`)
- **AC8** — An AAC-LC MP4 decodes and reports `bits: None` (lossy); an HE-AAC MP4 fails with `EngineError::Unsupported` **before** the output device is opened (the fake sink was never opened)
- **AC9** — A mono FLAC is output as stereo with both channels equal to the source sample for sample

Output (`tidal-player-audio`, decision logic behind a `PcmBackend` trait faked in tests; the real ALSA calls are a thin layer over it):

- **AC10** — `choose_format(source_bits, accepted)` returns the first format of the preference list for that source that the device accepts, or `None` (table over: 16-bit with every accepted subset of the four formats, 24-bit likewise, lossy, nothing accepted)
- **AC11** — `pack(samples, format)` is bit-exact (table: 16-bit and 24-bit sources into each of the four formats, including negative full-scale values; `S24_3LE` is 3 bytes little-endian per sample; `S24_LE` is 4 bytes with the sample in the low 24 bits, sign-extended)
- **AC12** — `open_output` against a fake backend: exclusive → reservation first, then open, then negotiation with the exact rate, resampling off and 4 periods; negotiation refused → reopened as `plughw:` on the same card, kind fallback, `bit_perfect: false` with a reason, and a second `open_output` for the same device goes straight to `plughw:`; `EBUSY` from the open is retried at 100 ms until it succeeds or 800 ms have passed, then `OutputError::Busy`, and the backend never sees a `plughw:` open; shared → no reservation, resampling on, 8 periods; `bit_perfect` true only in the cases listed under "Output kinds" (table over kind × source bits × negotiated format × rate match)
- **AC13** — The reservation decision follows the table under "Reservation" (table over the fake bus's behaviours: no bus, no owner, `true`, `false`, no reply). Time is injected; the settle wait is 200 ms and the reply timeout 500 ms. The name is released when the device is closed, on every path (end, stop, error, device switch)
- **AC14** — A fake device that reports an underrun on a write gets recovered and receives every frame exactly once (the underrun is counted); one that reports `ENODEV` makes the track fail with `Error(Output(Lost))`, and no `TrackEnded` follows

Engine (`tidal-player-audio`, driven with fixture sources, a fake clock and `MemorySink` grown into a fake device with a configurable delay):

- **AC15** — `Play` → `Started` (with the fixture's `SourceFormat` and the sink's `OutputInfo`) → every decoded frame reaches the sink once and in order → drain → `TrackEnded` once → the device is closed. A second `Play` while one is playing stops the first: the sink never receives frames of both interleaved, and only one device is open at any time
- **AC16** — A source that fails mid-track (after the retries of AC24), a corrupt FLAC frame, and an output error each produce exactly one `Error(...)` and **no** `TrackEnded`; the engine then accepts a new `Play` and plays it
- **AC17** — Gapless: with a same-format track preloaded, the sink receives the first track's frames immediately followed by the second's (exact concatenation), with no drain or reopen between them, and `Transitioned` is sent. With a different-format preload, the device is drained, closed and reopened with the new format before the second track's first frame
- **AC18** — Pause/resume: the frames the sink receives over pause → resume are exactly the track's frames with none missing or repeated; while paused, nothing is written and `Position` does not change; the position reported right after `Resume` equals the one reported at `Pause`
- **AC19** — Seek: after `Seek(t)` the first frame the sink receives is the track's frame `round(t × rate)` (compared with a full decode of the fixture), and the next `Position` is `t`. A seek while paused stays paused. A seek past the end gives `TrackEnded`. On a segmented source, the fake source records no fetch of any segment that ends at or before `t`; on a single-file source, no read of bytes before the seek point's byte offset after the seek
- **AC20** — `Position` = (frames written − the fake device's delay) / rate; events come at most 250 ms apart while playing (fake clock) and never go backwards except after a seek
- **AC21** — `SetDevice` while playing closes the old device (and releases its reservation) before opening the new one, and the new device's first frame is the frame at the position reached on the old one, with nothing lost or repeated beyond the old device's undrained delay
- **AC22** — `Stop` and `Shutdown` take effect within 1 s even when the source is blocked in a read that never returns (fake), close the device and release the reservation; after `Shutdown` the thread has ended (joined in the test)
- **AC23** — A source that stalls gives `Buffering`, then no writes and a frozen position, then `Buffered` and normal playback once 2 s of audio are available again; no underrun is counted

Fetching and CLI (`tidal-player`):

- **AC24** — `HttpSource` (against `wiremock`): reads a single-file stream with `Range` requests; a body cut short is resumed with `Range: bytes=<offset>-` from the exact offset, with retries after 0.5, 1 and 2 s (injected sleeper), then `SourceError::Network`; a `403` asks its re-resolve callback once and continues from the same offset on the new URL (a second `403` fails); a segmented plan fetches the init segment and then the segments in order; the read-ahead never holds more than the configured cap
- **AC25** — `tidal-player devices` output comes from a pure parser over `/proc/asound/cards` and `/proc/asound/pcm` contents (fixtures: no cards, one onboard card, onboard + USB DAC, a capture-only device which is not listed): `default` first, then each `hw:C,D` with a playback stream and its card name; the configured device is marked `*`
- **AC26** — `play`: settings come from flag, then environment, then default (table); `--quality` and `TIDAL_PLAYER_QUALITY` with an unknown value exit 2; without a session `play` exits 1 with the login hint; the "Track" and "Output" lines are formatted as under "Commands" (table over granted quality, source format, output kind and `bit_perfect` reason); the progress line is printed only when stdout is a terminal
- **AC27** — `docs/playback.md` documents `play`, `devices`, the settings, the output kinds (exclusive / fallback / shared) and what `bit-perfect` means, how to find the device for a DAC, and the errors below with what to do about them; this spec links to it

## Edge cases & errors

| Situation | What the user sees (from `play`; 0004 shows the same messages in the TUI) |
|---|---|
| Account or track below the asked quality | Tidal grants less (AC5); the "Track" line shows what was granted |
| Track is preview-only for this account/country | `Track 123 is only available as a preview for this account` (exit 1) |
| Track not playable for this account or country (`401`/`4005`) | `Track 123 is not available in <country>` (exit 1) |
| Unknown track ID, or not streamable in the country (Tidal answers `500`/`999`) | `Track 123 was not found, or cannot be streamed in <country>` (exit 1) |
| Encrypted stream, unsupported codec (HE-AAC), Dolby Atmos / 360 mode | `Track 123 is not playable: <what>` (exit 1). Never a burst of noise |
| Device busy (another app holds `hw:`), or reservation refused | `Output hw:1,0 is busy (used by <app>): close it, or use --device default` (exit 1). Never a silent downgrade to `plughw:` or shared |
| Device does not exist (`hw:5,0`, typo) | `No such output device hw:5,0: see "tidal-player devices"` (exit 1) |
| DAC unplugged while playing | `Output hw:1,0 was lost` (exit 1), device and reservation released, no panic (AC14) |
| Device refuses the source format | Plays through `plughw:`, the "Output" line says why it is not bit-perfect (AC12) |
| Network drops mid-track | Retries (AC24); the buffer covers short gaps (AC23); after the retries: `Network error while streaming track 123` (exit 1). Never reported as "track finished" (AC16) |
| Stream URL expired (long pause, seek after a while) | Re-resolved once, transparently (AC24) |
| Session expired while resolving | The `Authenticator`'s `LoginRequired` (spec 0002): `Session expired: run "tidal-player login"` (exit 1) |
| Ctrl-C while the device is being reserved or opened | Exits 130 within 1 s, nothing left reserved (AC22) |
| Sample-rate change between tracks | Drain, reopen with the new rate (AC17) |
| AAC encoder delay/padding | Not trimmed: lossy tracks are not sample-exact gapless (out of scope) |
| A seek to exactly the end, or past it | `TrackEnded` (AC19) |
| Pause on a device without hardware pause (`snd_pcm_pause` unsupported) | Up to one buffer (about 0.1 s) keeps playing after Pause; on Resume the next write recovers the underrun, so nothing is lost or repeated. Found at slice B acceptance; 0004/0005 may re-feed the dropped buffer instead |
| No ALSA at all (`--no-default-features` build) | `play` exits 1: `this build has no ALSA output` |

## Test plan

Each automated test is named after its criterion (`ac5_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in specs 0001 and 0002. HTTP tests use `wiremock` with fixtures under `crates/api/tests/fixtures/stream/` (written from the probe's recorded shapes, URLs and IDs replaced by fakes) and never touch the network. Audio fixtures are small generated files under `crates/audio/tests/fixtures/` (under 200 KiB in total), made from a sine by the `ffmpeg` commands recorded in that directory's `README.md`; tests never call `ffmpeg`. Time and sleeps are injected; no test sleeps for real or opens a real audio device.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/api/tests/stream.rs` :: `ac1_playbackinfo_request` | mock expects the path, exactly the four query params (country from the session) and the bearer header, `expect(1)` | stub `resolve_stream` returns `Err` without sending |
| AC2 | `crates/api/src/stream.rs` :: `ac2_bts_manifest` (table over fixtures) | plan or error per row | stub parser returns `Unsupported(Manifest)` for everything |
| AC3 | `crates/api/src/stream.rs` :: `ac3_dash_manifest` | segment count, first/last URL, start times, total duration of the recorded MPD; a hand-written MPD with `r` repeats | stub returns an empty segment list |
| AC4 | `crates/api/src/stream.rs` :: `ac4_validate_playbackinfo` (table) | each invalid field → its error; granted quality, bits and rate carried | stub accepts everything |
| AC5 | `crates/api/tests/stream.rs` :: `ac5_one_request` (table: grant equal, grant lower, `401`/`4005`, `500`/`999`, 503, 429, timeout, `LoginRequired`) | the mock saw exactly one `playbackinfopostpaywall` request and no `/token` request; the result per row | stub walks tidalt's ladder on any error, so the 503 row sees 4 requests |
| AC6 | `crates/core/src/quality.rs` :: `ac6_audio_quality` | ordering, serde names, CLI names, `low` rejected | stub parser accepts nothing |
| AC7 | `crates/audio/tests/decode.rs` :: `ac7_flac_raw_and_fmp4` | samples equal the plain-FLAC reference; source format; left-justification | stub decoder yields no samples |
| AC8 | `crates/audio/tests/decode.rs` :: `ac8_aac_lc_and_he_aac` | LC decodes with `bits: None`; HE-AAC → `Unsupported`, fake sink never opened | stub reports `bits: Some(16)` and opens the sink first |
| AC9 | `crates/audio/tests/decode.rs` :: `ac9_mono_to_stereo` | L == R == source | stub outputs one channel |
| AC10 | `crates/audio/src/output/format.rs` :: `ac10_choose_format` (table) | as in AC10 | stub always returns `S32_LE` |
| AC11 | `crates/audio/src/output/format.rs` :: `ac11_pack` (table) | bytes per row | stub writes zeros |
| AC12 | `crates/audio/src/output/open.rs` :: `ac12_open_output` (table, fake `PcmBackend` recording calls) | call sequence and resulting `OutputInfo` per row | stub opens the device as given with no negotiation and `bit_perfect: true` |
| AC13 | `crates/audio/src/output/reserve.rs` :: `ac13_reservation` (table, fake bus + fake clock) | result per row; settle and timeout durations; release on every close path | stub always proceeds |
| AC14 | `crates/audio/tests/engine.rs` :: `ac14_underrun_recovered`, `ac14_device_lost` | frames exactly once + count; one `Error`, no `TrackEnded` | stub engine stops at the first write error |
| AC15 | `crates/audio/tests/engine.rs` :: `ac15_play_to_end`, `ac15_play_replaces_play` | event sequence; sink content; single open device | stub engine emits nothing |
| AC16 | `crates/audio/tests/engine.rs` :: `ac16_errors_never_end_track` (table: source error, corrupt frame, output error) | one `Error`, no `TrackEnded`, next `Play` plays | stub sends `TrackEnded` after any stop (tidalt's bug) |
| AC17 | `crates/audio/tests/engine.rs` :: `ac17_gapless_same_format`, `ac17_reopen_on_format_change` | exact concatenation, no drain/reopen in the fake's call log; drain + reopen otherwise | stub drains and reopens between all tracks |
| AC18 | `crates/audio/tests/engine.rs` :: `ac18_pause_resume_loses_nothing` | sink content == track; positions | stub drops the device buffer on pause |
| AC19 | `crates/audio/tests/engine.rs` :: `ac19_seek` (table: playing, paused, past end, segmented, single-file) | first frame after seek; position; fetch log | stub restarts from frame 0 and decodes up to the target, fetching from the start |
| AC20 | `crates/audio/tests/engine.rs` :: `ac20_position_tracks_delay` | formula and cadence with the fake clock | stub reports frames written |
| AC21 | `crates/audio/tests/engine.rs` :: `ac21_set_device_moves_playback` | close-before-open order; continuity | stub ignores `SetDevice` until the next `Play` |
| AC22 | `crates/audio/tests/engine.rs` :: `ac22_stop_with_blocked_source` | `Stopped` arrives and the thread is joined within 1 s of real time (a bounded wait in the test, no sleep); device closed, reservation released | stub waits on the blocked read |
| AC23 | `crates/audio/tests/engine.rs` :: `ac23_buffering` | events, no writes, frozen position, no underrun | stub keeps writing silence |
| AC24 | `crates/app/tests/http_source.rs` :: `ac24_range_resume`, `ac24_retry_then_fail`, `ac24_reresolve_on_403`, `ac24_segmented_order`, `ac24_readahead_cap` | `Range` headers on the mock, sleeper log, callback count | stub reads the first response only |
| AC25 | `crates/audio/src/devices.rs` :: `ac25_parse_devices` (table over fixtures) + `crates/app/tests/cli.rs` :: `ac25_devices_marks_configured` (fixture files via `TIDAL_PLAYER_ASOUND_DIR`) | listed devices and order; `*` mark | stub returns only `default` |
| AC26 | `crates/app/src/play.rs` :: `ac26_settings_precedence`, `ac26_status_lines` (tables) + `crates/app/tests/cli.rs` :: `ac26_bad_quality`, `ac26_play_needs_login` | precedence; exact lines; exit codes and stderr | stub ignores the environment and prints nothing |
| AC27 | — reviewed at acceptance | `docs/playback.md` exists, matches this spec, is linked here | — |

Not covered by automated tests, on purpose, and checked by hand at acceptance with a real account on real hardware (results go in the PR description):

- The real ALSA backend, behind the `PcmBackend` seam: `play` on a USB DAC with a 16/44.1 and a 24/96 (or higher) track, confirming bit-perfect output by reading `/proc/asound/cardN/pcm0p/sub0/hw_params` while playing (format and rate must match the "Output" line, and the rate must equal the source's)
- The real ReserveDevice1 handshake with PipeWire running: PipeWire gives up the card while `play` runs and gets it back afterwards; a second `play` on the same `hw:` reports busy
- Shared mode with other audio playing at the same time
- The `plughw:` fallback, on a fixed-format interface if one is at hand (otherwise forced with a device that lacks the source rate)
- Gapless by ear across two tracks of an album (needs 0004's queue; until then by the engine test only)
- Unplugging the DAC while playing; Ctrl-C during reservation

## Crate placement

- `tidal-player-core`: `AudioQuality` (AC6). No I/O
- `tidal-player-api::stream`: `resolve_stream`, `StreamPlan`, `StreamError`, the BTS and MPD parsers, the response/error mapping. MPD parsing needs an XML parser: `quick-xml` (new workspace dependency, pure Rust)
- `tidal-player-audio`: the engine thread, the `ByteSource`/`TrackSource` traits that the engine reads from, decode (symphonia, newly used: features `flac`, `isomp4`, `aac`), format choice and packing, `PcmBackend` + the decision logic + the ALSA implementation (feature `alsa`), the device-list parser, the `Reserver` trait for ReserveDevice1 (the D-Bus implementation lives in the binary, so the audio crate gains no D-Bus dependency). Still no `tokio`, no `core`, no `api` (0001's layering)
- `tidal-player` (binary): `play` and `devices` subcommands, `HttpSource` (reqwest on its own fetch thread), the `Reserver` implementation with `zbus` (blocking API; new workspace dependency, already planned for MPRIS), wiring `resolve_stream` into the source's re-resolve callback
- `xtask layering`: no change (the audio crate's new dependencies are allowed; `core` still gets none of them)

## Facts vs. assumptions

Verified on 2026-10-06 in the dev container (scratch crate against `symphonia` 0.6.1 with ffmpeg-generated files; not against Tidal's real streams):

- symphonia 0.6.1 decodes FLAC 16-bit/44.1 kHz and 24-bit/96 kHz both as raw FLAC and inside **fragmented MP4** (`frag_keyframe+empty_moov+default_base_moof`), bit-exact against ffmpeg's decode, including from a non-seekable reader (`ReadOnlySource`). This answers spec 0001's open assumption: the probe (below) confirmed that Tidal's hi-res DASH segments are this shape
- symphonia's `SeekMode::Accurate` on those files lands on a frame boundary at or before the target (`actual_ts` ≤ `required_ts`); the caller discards frames up to `required_ts`
- symphonia decodes AAC-LC in MP4, but rejects HE-AAC only when SBR is signalled hierarchically (object type 5/29: "aac too complex"); backward-compatible signalling (the `0x2b7` sync extension) and implicit SBR are decoded as their LC core only. The engine therefore checks the AudioSpecificConfig itself and reports HE-AAC as `Unsupported` in every form (found at slice C acceptance). It does not trim AAC encoder delay (a 441 000-frame source decoded to 444 416 frames)
- A **non-fragmented** MP4 with `moov` at the end cannot be probed from a non-seekable reader ("missing moov atom"): single-file MP4 streams need a seekable (HTTP `Range`) source, which `HttpSource` provides
- tidalt's ALSA preference lists, buffer sizes and timing budgets are as quoted under "Context" (read from its source)

Verified on 2026-10-07 against the **live API** by `scripts/tidal-playback-probe.sh`, run by the user (account: `PREMIUM`, `highestSoundQuality: HI_RES`, country `NG`, client "Android Automotive HiRes"), with a hi-res track (33695188) and a CD-quality one (3079103). URLs and tokens redacted:

| Asked | Hi-res track | CD-quality track |
|---|---|---|
| `HI_RES_LOSSLESS` | `200`, granted `HI_RES_LOSSLESS`, `bitDepth` 24, `sampleRate` 96000, DASH, `codecs="flac"` | `200`, granted **`HIGH`**, BTS, `mp4a.40.2` |
| `LOSSLESS` | `200`, granted **`HIGH`**, BTS, `mp4a.40.2` | `200`, granted **`HIGH`**, BTS, `mp4a.40.2` |
| `HIGH` | `HIGH`, `mp4a.40.2` (AAC-LC 44.1 kHz) | same |
| `LOW` | `LOW`, **`mp4a.40.5`** (HE-AAC) | same |

- **The device-flow client never gets 16-bit FLAC.** Asking `LOSSLESS` gives AAC `HIGH`, on every track tried (three runs, incl. track 473593668, checked by the user to be 16/44.1). Only hi-res masters come as FLAC (DASH). Tidal capped that client ID in April 2026; the fix is to refresh under the PKCE client (below, and spec 0002 AC19)
- `playbackinfopostpaywall` response keys: `trackId, assetPresentation, audioMode, audioQuality, manifestMimeType, manifestHash, manifest, albumReplayGain, albumPeakAmplitude, trackReplayGain, trackPeakAmplitude`, plus `bitDepth` and `sampleRate` only on the hi-res grant
- BTS manifest: `{"mimeType":"audio/mp4","codecs":"mp4a.40.2","encryptionType":"NONE","urls":["https://amz-pr-fa.audio.tidal.com/<id>.mp4?token=…"]}`, one URL. `encryptionType` was `NONE` everywhere
- DASH manifest: `type="static"`, `profiles="urn:mpeg:dash:profile:isoff-main:2011"`, one `Period`/`AdaptationSet`/`Representation id="FLAC_HIRES,96000,24" codecs="flac" audioSamplingRate="96000"`, `AudioChannelConfiguration value="2"`, `SegmentTemplate timescale="96000" initialization="…/0.mp4?token=…" media="…/$Number$.mp4?token=…" startNumber="1"`, `SegmentTimeline` `<S d="380928" r="72"/><S d="129030"/>` (3.968 s segments, no `t`). Init segment 619 bytes, media segment ~1.38 MB. init + first segment joined is FLAC 24-bit 96 kHz stereo in MP4 (ffprobe), the shape decoded bit-exact under "Facts" above
- Stream hosts (`sp-ad-fa.audio.tidal.com` for DASH, `amz-pr-fa.audio.tidal.com` for BTS) honour `Range`: `206` with `Content-Range`, `Accept-Ranges: bytes`, and `416` past the end. `Cache-Control: max-age=31536000`
- The first 256 KiB of a `HIGH` MP4 was enough for ffprobe, so `moov` comes first (faststart); `HttpSource` still uses `Range` for seeking
- Stream URLs carry only a `token` query parameter: no readable expiry. Two requests 5 s apart gave different manifests (fresh tokens each time). The token lifetime and the status of an expired one are **not** known (AC24 still assumes `403`/`410`)
- Errors: unknown track ID → `500 {"status":500,"subStatus":999,"userMessage":"Unexpected error occurred."}`; `assetpresentation=PREVIEW` and `urlpostpaywall` at `HI_RES_LOSSLESS` → `401 {"subStatus":4005,"userMessage":"Asset is not ready for playback"}`; bad bearer → `401 {"subStatus":11002,…}`
- `GET /v1/users/{id}/subscription` works and reports `highestSoundQuality` (not needed by this spec)
- **Second run** (2026-10-07T00:22Z, hi-res track 35986245, 24/96): identical shapes. `HI_RES_LOSSLESS` → DASH FLAC 24/96 (`<S d="380928" r="72"/><S d="256282"/>`, `PT4M52.333S`); `LOSSLESS` → `HIGH` AAC again; `LOW` → HE-AAC; `urlpostpaywall` at `HI_RES_LOSSLESS` → `401`/`4005` again. The second track ID given as CD-quality got `500`/`999` from `playbackinfopostpaywall` at **every** tier (and `401`/`4005` from `urlpostpaywall`), the same answer as an unknown ID: so `500`/`999` also means "this track cannot be streamed here" (wrong ID, or not licensed in the country), not only a server fault. That run's `PREVIEW` request (on the same ID) also got `500`. The two manifests fetched 5 s apart were identical this time (different in the first run): tokens are sometimes reused, lifetime still unknown

- **Refreshing under the PKCE client fixes it** (`scripts/tidal-playback-probe2.sh`, 2026-10-07T01:00Z, same account). The device-flow refresh token, refreshed with client `6BDSRdpK9hqEBTgU`: `200`, `clientName` "TIDAL_Android_2.87.0". With that token:
  - CD-quality track 473593668 at `LOSSLESS` **and** at `HI_RES_LOSSLESS`: granted `LOSSLESS`, `bitDepth` 16, `sampleRate` 44100, **DASH** (not BTS): `Representation id="FLAC,44100,16" codecs="flac"`, `timescale="44100"`, `<S d="176128" r="55"/><S d="52930"/>` (3.993 s segments), `PT3M44.854S`. init 623 bytes + first segment = FLAC 16-bit 44.1 kHz in MP4 (ffprobe), the shape decoded bit-exact under "Facts" above
  - Hi-res track 35986245 at `HI_RES_LOSSLESS`: unchanged, DASH FLAC 24/96
  - Stream URLs then come from `sp-ad-cf.audio.tidal.com` with CloudFront signing (`Policy`, `Signature`, `Key-Pair-Id`; the policy presumably carries the expiry, not decoded). Those responses had no `Accept-Ranges` header but still answered `206` to `Range`
  - The device-flow client can still refresh the same refresh token afterwards (`200`)
- With either token, the header `x-tidal-client-version` changes nothing at `LOSSLESS`
- `openapi.tidal.com/v2/trackManifests` lists `formats: [FLAC, AACLC]` for the CD track, but with the PKCE token it carries `drmData` (`WIDEVINE`): not usable. With the device-flow token it had no `drmData`, but that token is the capped one. This spec stays on v1 `playbackinfopostpaywall`

Not verified:

- What Tidal answers for a free account, or a track not licensed in the country (assumed `401`/`4005`, the shape seen above)
- Whether a hi-res track at 44.1 kHz or 192 kHz uses the same DASH shape (assumed; the parser takes `timescale` and rates from the MPD)

Assumed, checked at manual acceptance:

- `alsa` 0.12 exposes everything the backend needs (exact rate, `set_rate_resample`, period/buffer sizes, `delay`, `drain`, `pause` or an equivalent, `recover`)
- PipeWire (WirePlumber) still implements `org.freedesktop.ReserveDevice1` and releases the card on request
- `zbus` 5's blocking API works from the engine thread without a tokio runtime on it

## Decisions (answered by the user, 2026-10-07)

1. **Endpoint**: use `playbackinfopostpaywall` instead of tidalt's `urlpostpaywall`. *Settled by the probe*: `urlpostpaywall` refuses hi-res for this client
2. **Default output device**: `default` (shared), so a first run never takes the sound card away from other apps; bit-perfect is opt-in with `--device hw:C,D` / `TIDAL_PLAYER_DEVICE`, and `devices` shows the names. No DAC auto-detection
3. **`play` / `devices`**: kept as permanent CLI commands, not only for acceptance (a CLI alongside the TUI is welcome). 0005 may group them under `playback …` when it adds the other one-shot commands
4. **`LOW` is HE-AAC** (probe): reject `low` as a setting rather than adding an AAC decoder with SBR (none in pure Rust today; FFmpeg is what 0001 removed). *Proposed: reject it*
5. **Other tidalt bugs**: none beyond "Context" for playback; the ones that hurt most were in daemon-client communication. They are collected from tidalt's history when spec 0005 is drafted
6. **CD-quality as FLAC**: *settled by probe 2*. Refreshing under the PKCE client (spec 0002 AC19, implemented together with this spec) gives 16/44.1 FLAC over DASH and keeps hi-res. `LOSSLESS` therefore always arrives as DASH; the BTS path stays for `HIGH` (AAC) and for any BTS FLAC Tidal may still send

## Out of scope

- Queue, auto-advance policy, next/previous, shuffle, repeat, volume (0004). 0004 decides when to send `Preload`, and clears `bit_perfect` whenever gain ≠ 1
- Releasing the device while paused, answering other applications' `RequestRelease`, daemon and clients (0005)
- Config file (0008), remembering the device (0009 left it out: there is no picker yet), MPRIS (0010)
- ReplayGain / loudness normalisation
- Encrypted streams (incl. the Widevine-protected v2 `trackManifests`), Dolby Atmos / Sony 360, video
- Gapless for lossy tracks (AAC delay trimming)
- Local file playback, caching streams to disk
- Non-ALSA backends (PipeWire native, JACK, PulseAudio): shared mode reaches them through ALSA's `default`
- Resampling in our own code: when the device cannot take the source rate, ALSA's plug layer does it

## Bugs

- **A missing device gives a backend error, and alsa-lib writes its own lines to stderr (found at slice D acceptance, 2026-10-07).** Expected (Edge cases): `--device hw:99,0` or a misspelt PCM name exits 1 with exactly one stderr line, `No such output device <name>: see "tidal-player devices"`. Actual: `snd_pcm_open` on a card that does not exist fails with `EINVAL`, which the ALSA backend mapped to `SinkError::Backend` (`Output error: audio backend error: … Invalid argument (22)`); and for any open failure alsa-lib prints `ALSA lib …: Unknown PCM …` to stderr itself, before our line. Root cause: the backend's open-error mapping only treated `ENOENT`/`ENODEV`/`ENXIO` as "not found", and nothing redirected alsa-lib's error output. New criterion: **AC28** — an open failing with `EINVAL`, `ENOENT`, `ENODEV` or `ENXIO` is `SinkError::NotFound`; alsa-lib's own diagnostics never reach stderr (captured on the engine thread and logged instead), so `play` with `--device hw:99,0` and with `--device tidal_player_no_such_pcm` prints exactly the one line. Tests: `crates/audio/src/output/alsa.rs` :: `ac28_open_error_mapping` (table over errno → `PcmError`; red: `EINVAL` maps to `Other`), and `crates/app/tests/cli.rs` :: `ac26_play_end_to_end_errors` tightened to both device names with exact stderr (red: the `ALSA lib` line and the backend error)
- **Seeking on real hardware fails with `snd_pcm_pause … Unknown errno (77)` (reported by the user, 2026-10-07, after 0004 made seeking reachable from the TUI).** Expected (AC18, AC19, AC23): seeking while playing or paused, and buffering after a seek, keep playing; nothing is reported. Actual: `Output error: audio backend error: ALSA function 'snd_pcm_pause' failed with error 'Unknown errno (77)'` and the track stops. Root cause: errno 77 is `EBADFD`: `snd_pcm_pause(1)` is only valid on a `RUNNING` PCM and `snd_pcm_pause(0)` only on a `PAUSED` one. A seek discards the device buffer (`snd_pcm_drop` + `snd_pcm_prepare`), leaving the PCM `PREPARED`; the refetch after the seek then reports `Buffering`, and the engine pauses the device, which is not running. Likewise a seek while paused, then `Resume`, unpauses a PCM that is no longer `PAUSED`; and `Play` while paused does the same. The fake `PcmBackend` accepted `pause` in any state, so no test saw it. New criterion: **AC29** — `PcmSink` tracks the PCM state (`PREPARED` after open, discard, drain and underrun recovery; `RUNNING` after a write; `PAUSED` after a pause) and calls the backend's `pause(true)` only when `RUNNING` and `pause(false)` only when `PAUSED`; otherwise `set_paused` succeeds without a call (a `PREPARED` PCM plays nothing until written to, and the engine never writes while paused). The fake backend enforces ALSA's state rules (`pause` in the wrong state fails like `EBADFD`). Test: `crates/audio/src/output/pcm_sink.rs` :: `ac29_pause_follows_pcm_state` (table: pause right after open; pause, then resume; seek (discard) then buffering pause; seek while paused, then resume; underrun recovery, then pause; red: the backend's `EBADFD`), and the existing `pause_discard_drain_delay` is updated to write before pausing, since pausing a PCM that never ran is the bug

- **`ac5_one_request` fails under machine load (reported by the user, 2026-10-08: once in a full `cargo test --workspace` beside another big build; 3/3 green alone).** Expected (AC5 test plan): the table proves one `playbackinfopostpaywall` request per row whatever the machine is doing. Actual: `503: playbackinfo requests`, left `0`, right `1`. Root cause: every row ran with a 200 ms request timeout, there only for the `timeout` row. reqwest's timeout covers connecting and sending too, so on a starved CPU a row that is not about timing timed out before the request reached the mock server (0 requests, a transport error instead of the row's answer). Reproduced: the test binary pinned to one core at `nice 19` beside 8 busy loops on that core (`taskset -c 0 nice -n 19 <stream test binary> ac5_one_request`) failed 5 runs out of 5 at the first row. New rule for the test (AC5 is unchanged): each row carries its own timeout; rows not about timing use the production `RESOLVE_TIMEOUT` (10 s), and the `timeout` row uses 5 s against a response delayed 60 s, so the outcome of a row never depends on how fast the machine is. Under the same reproduction the fixed test passed 4 runs out of 4 (a 2 s budget for the `timeout` row still failed once there, so it is 5 s; that row's request must reach the local server within its own timeout, the one timing assumption left). Test: `crates/api/tests/stream.rs` :: `ac5_one_request` (red: the load reproduction above)
