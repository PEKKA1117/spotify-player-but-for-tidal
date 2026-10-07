# Playing tracks

`tidal-player` decodes Tidal's streams itself (FLAC and AAC, in pure Rust) and writes them to an ALSA device: through the system mixer by default, or straight to your DAC, bit-perfect, when you ask for it. Design: [spec 0003](specs/0003-playback-engine.md) (the engine) and [spec 0004](specs/0004-queue-and-controls.md) (the queue and the controls).

Three ways to play: the TUI (`tidal-player [ITEM]...`, see [The TUI](tui.md)), headless from the command line (`tidal-player play <ITEM>...`, below), or the daemon (`tidal-player daemon`, controlled with `tidal-player playback …` and attached TUIs, see [The daemon and clients](daemon.md)). All play a **queue** filled from [items](#items).

## Items

An **item** is what you give the player to queue: a track ID or a Tidal link, as the Tidal apps' share menu copies it.

| Item | Queues |
|---|---|
| `77640617` | track 77640617 |
| `https://tidal.com/browse/track/77640617`, `https://tidal.com/track/77640617`, `https://listen.tidal.com/track/77640617`, `tidal://track/77640617` (with or without `www.`, the share menu's `/u` suffix, a trailing `/` or a query string) | track 77640617 |
| `https://tidal.com/browse/album/123`, `https://tidal.com/album/123/u` | every track of album 123, in album order |
| `https://tidal.com/browse/playlist/<uuid>`, `…/playlist/<uuid>/u` | every track of the playlist, in playlist order; music videos are left out |

Anything else (an artist, mix or video link, another site, a typo) is refused before anything plays, with `Not a Tidal track, album or playlist: <item>` (exit 2). Several items are queued one after the other, in the order given. Albums and playlists are fetched whole, however long they are, before playback starts.

A track Tidal does not offer for streaming is still queued and shown, but skipped (`Track 123 is not available in NO`).

## Commands

### `tidal-player [--add-to-queue | --play-next] [ITEM]...`

Starts the TUI. With items, the queue is loaded with them and the first one plays; without, the queue starts empty (add to it with `o`, see [The TUI](tui.md#adding-tracks)). If an item cannot be fetched (`Album 123 was not found`, a network error) or has no tracks, the queue is left as it was and the message shows in the playback window.

`--add-to-queue` adds the items at the end of the queue and `--play-next` right after the current track, instead of replacing the queue; when nothing was playing, the first added track starts. With a player already running (a daemon, or another TUI), the TUI attaches to it and the items go to that player's queue: see [Attaching a TUI](daemon.md#attaching-a-tui). They cannot be combined, and both need at least one item (exit 2).

### `tidal-player play <ITEM>... [--shuffle] [--repeat off|queue|track] [--autoplay] [--quality Q] [--device PCM] [--start SECONDS]`

Plays the items as one queue in the foreground, without the TUI, and exits when the queue ends. While it plays, [`tidal-player playback …`](daemon.md#one-shot-commands) controls it. With another player running it exits 3 (`Another player is running (pid N): use "tidal-player playback load"`). It needs a stored session (see [Logging in](login.md)); without one it exits 1 with `Not logged in: run "tidal-player login"`.

```
$ tidal-player play 77640617 --device hw:1,0
Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo
Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect
  1:23 / 4:56
```

- **Track** line: the quality Tidal **granted** (it may be lower than you asked: Tidal decides per track and per account), and the format the decoder found
- **Output** line: the device actually opened, its kind (below), the sample format and rate it runs at, then `bit-perfect` or why it is not
- The progress line redraws in place, only when stdout is a terminal (nothing is printed when you pipe the output). It shows `?:??` as the duration when the stream does not say
- The Track and Output lines are printed again each time a track starts; the progress line follows the current track
- `--start 90` starts the first track 90 seconds in
- `--shuffle` shuffles the queue (the first item still plays first); `--repeat queue` starts over at the end, `--repeat track` repeats each track. See [shuffle and repeat](tui.md#shuffle-repeat-and-autoplay)
- `--autoplay` (or `TIDAL_PLAYER_AUTOPLAY=on`) keeps going with tracks Tidal suggests when the queue runs out, until Ctrl-C

A track that cannot be played (not available in your country, not decodable) is skipped with its message on stderr; after 5 in a row (or the whole queue, if shorter) playback stops. A network, `429`/`5xx`, output or session error stops at once, never skips (see [Failures](tui.md#failures)).

Exit codes: `0` the queue ran out and at least one track played to its end; `1` it stopped on an error, or no track could be played (one line on stderr, see [Errors](#errors)); `2` a bad argument, setting or item; `130` Ctrl-C (the device is closed and released first). With one track ID and none of the new flags, `play` behaves exactly as it did before the queue existed.

### `tidal-player devices`

Lists the devices `--device` accepts: `default` first, then every ALSA hardware device (`hw:CARD,DEVICE`) that can play, with the card's name. The device `play` would use is marked `*`:

```
$ tidal-player devices
* default  shared, through the system mixer
  hw:0,0   HDA Intel PCH: ALC892 Analog
  hw:0,1   HDA Intel PCH: ALC892 Digital
  hw:1,0   E30 II: USB Audio
```

It reads `/proc/asound/cards` and `/proc/asound/pcm`; capture-only devices (microphones) are not listed.

## Settings

| Setting | Flag | Environment | Default |
|---|---|---|---|
| Highest quality to ask for | `--quality` (`hi-res`, `lossless`, `high`) | `TIDAL_PLAYER_QUALITY` | `hi-res` |
| Output device | `--device` (any ALSA PCM name) | `TIDAL_PLAYER_DEVICE` | `default` |
| Volume step of `+`/`-` in the TUI (%) | | `TIDAL_PLAYER_VOLUME_STEP` (1–25) | `5` |
| Seek step of `>`/`<` in the TUI (s) | | `TIDAL_PLAYER_SEEK_STEP` (1–600) | `5` |
| Previous restarts the track after (s); `0`: previous always goes back | | `TIDAL_PLAYER_PREVIOUS_RESTART` (0–60) | `3` |
| Autoplay at start | `--autoplay` (`play` only) | `TIDAL_PLAYER_AUTOPLAY` (`on`, `off`) | `off` |
| Release the device after pausing for (s); see [Releasing the device](daemon.md#releasing-the-device-while-paused) | | `TIDAL_PLAYER_RELEASE_PAUSED` (0–3600, or `never`) | `10` |

A flag beats the environment, which beats the default. An empty variable counts as unset. An invalid value exits 2 before anything starts, naming the variable and what it accepts (`invalid TIDAL_PLAYER_SEEK_STEP: expected an integer from 1 to 600, got "0"`). An unknown quality exits 2. `low` is refused too: Tidal's `LOW` streams are HE-AAC, which the player cannot decode; `high` (AAC 320 kbit/s) is the lowest setting.

The quality is the **highest** to ask for. Tidal answers with what the track and your subscription allow: a CD-quality track asked at `hi-res` comes as `LOSSLESS` (FLAC 16-bit 44.1 kHz), and some tracks only exist as `HIGH`.

To make a DAC the default, put `export TIDAL_PLAYER_DEVICE=hw:1,0` in your shell profile (a config file comes with spec 0008).

## Output kinds and bit-perfect

| `--device` | Kind | What happens |
|---|---|---|
| `hw:C,D` (or `hw:C`) | **exclusive** | The card is opened directly. The player asks PipeWire/PulseAudio to release it first (`org.freedesktop.ReserveDevice1`) and holds it while the track plays; other applications cannot use the card meanwhile. The device runs at the track's exact rate, with no resampling |
| `hw:C,D` that refuses the track's format | **fallback** | Reopened as `plughw:C,D`: ALSA converts the rate or format. The Output line says why, e.g. `resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)` |
| `plughw:…` | **fallback** | As above, chosen by you |
| `default` or any other name | **shared** | Through the system mixer (PipeWire, PulseAudio, dmix), alongside other applications. Never bit-perfect |

**Bit-perfect** means the samples in the file reach the DAC unchanged. The Output line says `bit-perfect` only when all of these hold:

- the kind is exclusive (`hw:`),
- the track is lossless (FLAC; AAC is `lossy source`),
- the device runs at the track's sample rate and in stereo (mono tracks are sent as identical left and right channels, which stays bit-perfect),
- the sample format holds at least the track's bits (a 16-bit track in `S32_LE` is padded with zeros, which is still bit-perfect; a 24-bit track in `S16_LE` is not).

Otherwise it gives the reason: `shared (system mixer)`, `lossy source`, `plughw (ALSA may convert the samples)`, `resampled (plughw fallback: …)`, `device runs at 48 kHz, source is 44.1 kHz`, `24-bit source truncated to S16_LE`, ….

To check it while a track plays, read the card's hardware parameters; format and rate must match the Output line:

```sh
cat /proc/asound/card1/pcm0p/sub0/hw_params
```

### Finding your DAC's device

1. Plug the DAC in and run `tidal-player devices`. It shows up as a new card, usually named after the DAC (`hw:1,0   E30 II: USB Audio`). Most USB DACs have one device, `,0`
2. Try it: `tidal-player play <track-id> --device hw:1,0`. The Output line should say `(exclusive)` and `bit-perfect` for a FLAC track
3. Keep it: `export TIDAL_PLAYER_DEVICE=hw:1,0`

Card numbers can change when you plug devices in a different order. ALSA also accepts the card's ID instead of its number (`hw:CARD=DAC,DEV=0`, the ID is the name in brackets in `/proc/asound/cards`); the `devices` list shows numbers.

## Errors

| Message | What to do |
|---|---|
| `Not logged in: run "tidal-player login"` | Log in first ([Logging in](login.md)) |
| `Session expired: run "tidal-player login"` | Tidal no longer accepts the stored login: log in again |
| `Track 123 is only available as a preview for this account` | Your subscription or country only gets a 30-second preview of it |
| `Track 123 is not available in NO` | Tidal does not offer this track to your account or country |
| `Track 123 was not found, or cannot be streamed in NO` | Check the track ID; Tidal answers the same for a track that is not licensed in your country |
| `Track 123 is not playable: …` | The stream is in a form the player cannot decode (encrypted, Dolby Atmos / Sony 360, HE-AAC). Nothing is played rather than noise |
| `Output hw:1,0 is busy (used by …): close it, or use --device default` | Another application holds the card, or refused to release it. Close it, or play through the mixer with `--device default`. The player never silently switches to another device |
| `No such output device hw:5,0: see "tidal-player devices"` | Typo or unplugged device: pick a name from `tidal-player devices` |
| `Output hw:1,0 was lost` | The DAC was unplugged (or failed) while playing. The device and its reservation are released |
| `Network error while streaming track 123` | The connection dropped and did not come back after retries (0.5 s, 1 s and 2 s). Short drops are covered by the read-ahead buffer and the retries without you noticing |
| `Not a Tidal track, album or playlist: …` | The item is not a track ID or a track, album or playlist link (see [Items](#items)) |
| `Track 1 was not found`, `Album 1 was not found`, `Playlist <uuid> was not found` | Tidal does not know that ID: check the link |
| `Album 1 has no tracks`, `Playlist <uuid> has no tracks` | The album or playlist is empty, or holds only videos (`play` exits 2) |
| `Stopped: 5 tracks in a row could not be played` | Several tracks in a row were unplayable (an album not offered in your country, for example); playback stops rather than run through the whole queue |
| `Autoplay: no suggestions (…)` | Tidal had no suggestions for the last track, or the request failed; the queue stops at its end |
| `this build has no ALSA output` | This binary was built without the `alsa` feature: rebuild with the default features |

Stream links expire after a while (a long pause, a seek much later). The player then asks Tidal for a fresh link once and continues where it was; you see nothing.
