# The TUI

Plain `tidal-player` (optionally with [items](playback.md#items)) opens the terminal interface. With no player running, the player and the screen run in one process (standalone): quitting stops playback and releases the audio device. With a player already running (a [daemon](daemon.md), or another TUI), the TUI attaches to it instead: the screen and the keys are the same, and quitting leaves the music playing (see [Attaching a TUI](daemon.md#attaching-a-tui)). Design: [spec 0004](specs/0004-queue-and-controls.md) and [spec 0005](specs/0005-daemon-and-clients.md).

## The screen

One frame titled `tidal-player`, with the **playback window** (4 rows) at the top and the **queue** below it:

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                     shuffle  repeat: queue  80%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: volume below 100% │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────  1:23 / 3:32│
│┌Queue (12)──────────────────────────────────────────────────────────────────┐│
││  1   May These Noises Start…  Pierce The Veil    Collide With The S…   4:01││
││▶ 2   Hell Above               Pierce The Veil    Collide With The S…   3:32││
││  3   A Match Into Water       Pierce The Veil    Collide With The S…   4:22││
│…                                                                             │
││  9   I'm Low On Gas And You…  Pierce The Veil    Collide With The S…   3:43││
││  ── Suggested ─────────────────────────────────────────────────────────────││
││  10  If I'm James Dean, You…  Sleeping With Si…  With Ears To See A…   3:39││
```

### Playback window

1. The state, the title and the artists, and on the right the modes:
   - `▶` playing, `⏸` paused, `…` loading or buffering, `■` stopped; `Nothing playing` when the queue is empty
   - `shuffle` when shuffle is on, `repeat: queue` or `repeat: track` when repeat is on, `autoplay` when autoplay is on, then the volume (`80%`), or `muted`
2. The album
3. How it plays: the quality Tidal granted, the format, the output device, and `bit-perfect` or why it is not (as the "Track" and "Output" lines of [`play`](playback.md#output-kinds-and-bit-perfect)), then ` · device released` while paused with the device [released](daemon.md#releasing-the-device-while-paused). When something went wrong, the message takes this row instead (see [Failures](#failures)); so does `Disconnected from the player: reconnecting…` while an attached TUI has lost its player
4. The progress bar, the position and the length. When Tidal does not give the length, there is no bar: `1:23 / ?:??`

### Queue

The queue in the order it plays (shuffled when shuffle is on). The playing track is marked `▶` and kept in view when it changes. The highlighted row is the **cursor**, which you move with the keys below; it stays on its track when the queue changes, and moves to the neighbouring row when its track is removed. Tracks added by [autoplay](#shuffle-repeat-and-autoplay) come after a `Suggested` row and are drawn dimmed.

In a narrow terminal the columns are cut with `…`; the album column goes first, then the artist. In a terminal of fewer than 8 rows (9 while the session-expired line shows) only the playback window is shown. When the session has expired, the last row says `Session expired — run "tidal-player login" in another terminal` (see [Logging in](login.md)).

## Keys

| Key | Does |
|---|---|
| `Space` | play/pause |
| `n` / `p` | next / previous track |
| `>` / `<` | seek forward / backward by the seek step (5 s) |
| `^` | back to the start of the track |
| `Ctrl-s` | shuffle on/off |
| `Ctrl-r` | repeat: off → queue → track → off |
| `A` | autoplay on/off |
| `+` / `-` | volume up / down by the volume step (5 %) |
| `_` | mute / unmute |
| `o` | add a link or track ID to the end of the queue |
| `O` | add a link or track ID to play next |
| `j` or `↓`, `k` or `↑` | move the cursor down, up |
| `g g`, `G` | move the cursor to the top, to the bottom |
| `Enter` | play the track under the cursor |
| `q`, `Esc` | quit (an attached TUI detaches; the player keeps playing) |

`g g` is two presses of `g`; a `g` followed by any other key does what that key does. With an empty queue only the volume, mute and mode keys (and `o`/`O`, `q`) do something; the modes and the volume then apply to what you add next.

The steps come from the environment: `TIDAL_PLAYER_VOLUME_STEP` (1–25 %, default 5) and `TIDAL_PLAYER_SEEK_STEP` (1–600 s, default 5); see [Settings](playback.md#settings). Keys cannot be changed yet (spec 0008).

## Adding tracks

`o` opens a prompt over the top of the queue, `Add to queue: `; `O` opens `Play next: `. Type or paste (the terminal's paste, `Ctrl-Shift-v` in most terminals) a track ID or a Tidal track, album or playlist link, as on the [command line](playback.md#items), then:

- `Enter` sends the item to the player, which fetches the tracks and adds them at the end of the queue (`o`) or right after the playing track (`O`). If nothing is playing (an empty queue, or a stopped player with no current track), the first added track starts. The queue shows them once the player has added them
- `Backspace` deletes the last character; `Esc` closes the prompt without adding anything

While the prompt is open every key types into it: `Space`, `q` and the other keys do not reach the player. An invalid item (`Not a Tidal track, album or playlist: …`) or a failed fetch (`Album 123 was not found`) closes the prompt and shows the message in the playback window; the queue is unchanged.

## Playback

- **Previous** (`p`) goes back to the start of the track when more than 3 seconds have played, otherwise to the previous track (`TIDAL_PLAYER_PREVIOUS_RESTART`, 0–60 s; `0` makes previous always go back). On the first track it goes back to the start
- **Next** (`n`) at the end of the queue (and repeat off, autoplay off) stops on the last track at 0:00; `Space` then plays it again
- **Seeking** past the end ends the track, which then moves on as it would at its natural end; before 0:00 it stops at 0:00. Seeking while a track loads sets where it starts
- **Pause while loading**: the track keeps loading but starts only when you press `Space` again
- Tracks follow each other without a gap when their formats match (the next track is prepared 30 seconds before the end)

### Shuffle, repeat and autoplay

- **Shuffle** (`Ctrl-s`) puts the playing track first and shuffles the rest; the queue shows the new order. Turning it off restores the original order. The playing track keeps playing either way. Tracks added while shuffle is on go where you add them; they are not shuffled in
- **Repeat** (`Ctrl-r`): `queue` starts over at the first track after the last one; `track` plays the same track again at its end (`n` still moves on)
- **Autoplay** (`A`, off by default; `TIDAL_PLAYER_AUTOPLAY=on` turns it on at start) keeps the music going when the queue runs out: near the end of the last track, Tidal is asked for tracks related to it, and they are added to the queue (under `Suggested`, without tracks already queued) and play on without a gap. Repeat `queue` or `track` takes precedence. When Tidal has nothing to suggest, the queue stops at its end with `Autoplay: no suggestions (…)`

### Volume

From 0 to 100 %, **100 % by default**, in steps of the volume step. At 100 % the samples are not touched, so a bit-perfect output stays bit-perfect; below 100 % (or muted) the third row says `not bit-perfect: volume below 100%` (or `muted`). The volume follows a curve that matches how loudness is heard (50 % is about −18 dB). Mute (`_`) keeps the volume, so unmuting restores it; changing the volume unmutes. The volume is not remembered across runs yet (spec 0009).

## Failures

The message shows in the playback window, in place of the format row, until the next track starts or another message replaces it.

| What failed | Examples | What happens |
|---|---|---|
| The track only | not available in your country, preview only, not found, not decodable | The message shows and the next track plays. After 5 such tracks in a row (or the whole queue, if shorter) playback stops: `Stopped: 5 tracks in a row could not be played` |
| The network or Tidal | the connection dropped after retries, `429`, `5xx` | Stops on that track, where it was. Never skips. `Space` tries again |
| The output | `Output hw:1,0 is busy …`, `No such output device …`, `Output hw:1,0 was lost` | Stops on that track; the queue is kept. `Space` tries again on the same device |
| The session | Tidal no longer accepts the login | Stops; the last row shows the session-expired status. After `tidal-player login` in another terminal, `Space` resumes |

The messages are the same as for `play`: see [Errors](playback.md#errors).
