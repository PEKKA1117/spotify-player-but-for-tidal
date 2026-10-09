English | [繁體中文](zh-TW/playback.md)

# Playing tracks

`tidal-player` decodes Tidal's streams itself (FLAC and AAC, in pure Rust) and writes them to an ALSA device: through the system mixer by default, or straight to your DAC, bit-perfect, when you ask for it. Design: [spec 0003](specs/0003-playback-engine.md) (the engine) and [spec 0004](specs/0004-queue-and-controls.md) (the queue and the controls).

Three ways to play: the TUI (`tidal-player [ITEM]...`, see [The TUI](tui.md)), headless from the command line (`tidal-player play <ITEM>...`, below), or the daemon (`tidal-player daemon`, controlled with `tidal-player playback …` and attached TUIs, see [The daemon and clients](daemon.md)). All play a **queue** filled from [items](#items). Every player also answers the desktop's media controls, the media keys and `playerctl`: see [Desktop controls and media keys](mpris.md).

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

Every setting can be set in [`app.toml`](config.md#apptoml) (in the [config directory](config.md#where-the-files-are), `~/.config/tidal-player` by default), by an environment variable, and some by a flag.

| Setting | `app.toml` key | Flag | Environment | Default |
|---|---|---|---|---|
| Highest quality to ask for | `quality` | `--quality` (`hi-res`, `lossless`, `high`) | `TIDAL_PLAYER_QUALITY` | `hi-res` |
| Output device | `output_device` | `--device` (any ALSA PCM name) | `TIDAL_PLAYER_DEVICE` | `default` |
| Volume step of `+`/`-` in the TUI (%) | `volume_step` | | `TIDAL_PLAYER_VOLUME_STEP` (1–25) | `5` |
| Seek step of `>`/`<` in the TUI (s) | `seek_duration_secs` | | `TIDAL_PLAYER_SEEK_STEP` (1–600) | `5` |
| Previous restarts the track after (s); `0`: previous always goes back | `previous_restart_secs` | | `TIDAL_PLAYER_PREVIOUS_RESTART` (0–60) | `3` |
| Autoplay at start | `autoplay` (`true`, `false`) | `--autoplay` (`play` only) | `TIDAL_PLAYER_AUTOPLAY` (`on`, `off`) | `off` |
| Remember the queue, position, modes and volume across runs; see [Resuming the last session](#resuming-the-last-session) | `remember_playback` (`true`, `false`) | | `TIDAL_PLAYER_REMEMBER_PLAYBACK` (`on`, `off`) | `true` |
| Publish the player for desktop controls, media keys and `playerctl`; see [Desktop controls and media keys](mpris.md) | `mpris` (`true`, `false`) | | `TIDAL_PLAYER_MPRIS` (`on`, `off`) | `true` |
| Album covers kept in the cache for the desktop (`0`: none, the desktop fetches them); see [Album covers](mpris.md#album-covers) | `max_cover_arts` (0–1000) | | `TIDAL_PLAYER_MAX_COVER_ARTS` | `20` |
| Release the device after pausing for (s); see [Releasing the device](daemon.md#releasing-the-device-while-paused) | `release_paused_secs` (or `"never"`) | | `TIDAL_PLAYER_RELEASE_PAUSED` (0–3600, or `never`) | `10` |
| Rows fetched at a time for the library's lists in the TUI; see [Lists load as you scroll](tui.md#lists-load-as-you-scroll) | `page_size` | | `TIDAL_PLAYER_PAGE_SIZE` (1–10 000) | `100` |
| Rows fetched at a time for the search results in the TUI; see [Search](tui.md#search) | `search_page_size` | | `TIDAL_PLAYER_SEARCH_PAGE_SIZE` (1–1000) | `20` |
| Words that hide alternate versions in an artist's *All tracks* (comma-separated in the variable, an array in the file; empty hides nothing); see [The artist's All tracks](tui.md#the-artists-all-tracks) | `hide_versions` | | `TIDAL_PLAYER_HIDE_VERSIONS` | `instrumental, inst, off vocal, karaoke, tv version, tv ver, tv size, tv edit, sped up, speed up, nightcore, slowed, slowed + reverb, reverb, 8d, 8d audio` |

A flag beats the environment, which beats `app.toml`, which beats the default, per setting. An empty variable counts as unset (except `TIDAL_PLAYER_HIDE_VERSIONS`, where empty means hide nothing), so the file's value applies. An invalid value exits 2 before anything starts, naming the variable (or the file and key) and what it accepts (`invalid TIDAL_PLAYER_SEEK_STEP: expected an integer from 1 to 600, got "0"`, `…/app.toml: invalid seek_duration_secs: expected an integer from 1 to 600, got 0`); an invalid variable is an error even when the file is valid. An unknown quality exits 2. `low` is refused too: Tidal's `LOW` streams are HE-AAC, which the player cannot decode; `high` (AAC 320 kbit/s) is the lowest setting. See [Configuration](config.md) for the file's syntax and errors.

The quality is the **highest** to ask for. Tidal answers with what the track and your subscription allow: a CD-quality track asked at `hi-res` comes as `LOSSLESS` (FLAC 16-bit 44.1 kHz), and some tracks only exist as `HIGH`.

To make a DAC the default, put `output_device = "hw:1,0"` in `~/.config/tidal-player/app.toml` (or `export TIDAL_PLAYER_DEVICE=hw:1,0` in your shell profile).

## Resuming the last session

The player (the standalone TUI, or the daemon) remembers what it was playing and starts from it the next time, **stopped** at the same position: nothing plays, and nothing is fetched or opened, until you press `Space` (or `tidal-player playback play-pause`), which plays from that position. Design: [spec 0009](specs/0009-persistence.md).

- **Remembered**: the queue (with its `Suggested` tracks and its shuffled order), the current track and the position in it, shuffle, repeat, autoplay, the volume and mute. Not remembered: whether it was playing, the output device (it comes from the settings), and anything a TUI shows on its own (the page, the cursor, the search)
- **Autoplay**: the remembered mode beats `autoplay` in `app.toml` (the file sets it for a fresh queue); `TIDAL_PLAYER_AUTOPLAY` and `--autoplay` beat the remembered mode
- **`tidal-player ITEM…`** starting a player replaces the remembered queue with the items and plays them; shuffle, repeat, autoplay and volume stay as remembered. `tidal-player play ITEM…` neither reads nor writes the remembered state
- **When it is saved**: a change is saved within 2 seconds (many changes in a row are saved once); the position while playing at most every 30 seconds; a pause, a stop, a seek, a track change and quitting (`q`, `tidal-player daemon stop`, `systemctl --user stop`) save at once. A crash or a power loss loses at most the last 30 seconds of position
- **Where**: `playback.json` in the state directory, next to the session file: `$TIDAL_PLAYER_STATE_DIR`, else `$XDG_STATE_HOME/tidal-player`, else `~/.local/state/tidal-player`. It is written to a temporary file and renamed over the old one, so a crash leaves the old file or the new one, never half of one (on a local filesystem). It is the player's data, not a file to edit. `tidal-player logout` deletes it ([Logging in](login.md#tidal-player-logout))
- **Turning it off**: `remember_playback = false` in [`app.toml`](config.md#apptoml), or `TIDAL_PLAYER_REMEMBER_PLAYBACK=off`. The player then neither reads nor writes `playback.json`, and leaves an existing one as it is
- **A file that cannot be read**: the player starts with an empty queue and says why in the playback window, `Could not restore the playback state: <path>: <error>`. A corrupt file (or one from another version) is kept as `playback.json.bad` and the message is `Could not restore the playback state (kept as playback.json.bad): <reason>`. The player always starts; the next save writes a fresh file
- **A save that fails** (disk full, read-only directory): the playback window says `Could not save the playback state: <error>` once; playback goes on, and the next change tries again

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
