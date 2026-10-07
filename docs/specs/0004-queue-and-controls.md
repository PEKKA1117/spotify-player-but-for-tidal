# 0004 — Queue & playback controls

- **Status**: implemented (2026-10-07)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0001 (implemented), 0002 (implemented), 0003 (approved; its engine is extended here, see "Engine additions")
- **User docs**: [`docs/playback.md`](../playback.md) (extended) and a new [`docs/tui.md`](../tui.md) (written by this spec's implementation, AC24)

## Context

0003 plays one track. This spec adds what turns that into a player: a queue, play/pause/seek/next/previous, shuffle, repeat, auto-advance (gapless, through 0003's `Preload`), volume, and the first real TUI screen (a now-playing bar and the queue). It creates the player state machine that spec 0001 reserved, `tidal_player_core::player` (`PlayerState`, `PlayerInput`, `PlayerEffect`, `update`), and fills `protocol::Command`/`Event` with the playback messages. The player runs in-process with the TUI (0001's *standalone* mode) and behind `tidal-player play`; the daemon and its clients are 0005, and nothing here may assume the client and the player share memory.

Browsing (library, search) arrives with 0006/0007. Until then the queue is filled from Tidal links or IDs, given on the command line or pasted into the TUI (decision 1). When the queue runs out, **autoplay** can continue with tracks Tidal suggests, as the Tidal apps do (decision 4).

### What tidalt did

Read from `internal/ui/model.go`, `internal/ui/queue.go`, `internal/player/mpv.go` and the history of their `fix` commits:

- The queue, the current index, shuffle and the "advancing" flags lived in the BubbleTea UI model; the player goroutine only played one URL at a time and closed a "done" channel at the end
- Shuffle had two modes: *Random* (pick an unplayed index on each advance, with a played-index stack for *previous*) and *Fisher–Yates* (pre-shuffled copy, original order kept aside). No repeat
- Previous was "cursor − 1", or a pop from the shuffle history stack
- Volume was a float in the player, applied by scaling every sample; the UI defaulted it to 50 % (`e925071`)
- Keys (spotify preset): `Space` resume/pause, `n`/`p` next/previous, `>`/`<` seek ±10 s, `+`/`-` volume ±5 %, `C-s` shuffle, `z` queue

### What went wrong there, and the criterion that covers it here

1. **Skips cascaded through the playlist** (`5e0e06b`): the old track's "done" message, processed after the new track started, triggered another auto-advance. Fixed there with a generation counter in the UI → here every track-scoped engine event carries the tag of the command that started the track, and the player ignores stale ones (AC6, AC14)
2. **Errors looked like the end of a track**, so the UI auto-advanced into the same broken state, track after track (`2afa977`, and 0003 "Context" 2) → 0003 AC16 keeps `Error` and `TrackEnded` apart; here the player decides per error kind whether to skip or stop (AC7)
3. **Tight skip loops** when every track failed, hammering the API into `429`s (`f6b90be` added skipping past unplayable tracks, `fac01ca` a 2 s delay against the loop) → a `429` or any transient error never skips, and skipping stops after a bounded run of failures (AC7)
4. **Skip did not interrupt the current track** (`efbf527`): `PlayNext` queued the URL and the stream loop never looked at it → `Next` sends `Play`, which 0003's engine handles at once (AC1)
5. **A client re-shuffled the daemon's already-shuffled queue** and showed a different order from the one playing (`fb4721c`, issues #19–#21) → shuffle exists only in the player; snapshots carry the queue in play order and clients show it as sent (AC3, AC22)
6. **A 100 % progress bar at the start of every track** (`78f841f`): the duration was read as 0 before it was set → the duration comes with the queue entry's metadata, and an unknown duration draws no bar rather than a full one (AC23)
7. **Volume broke bit-perfect silently**: samples were scaled while the UI kept saying "bit-perfect", and the default volume was 50 % → the default is 100 %, gain 1 leaves samples untouched (bit-exact, AC15), and any other volume clears `bit_perfect` with a reason (AC9; 0003 "Out of scope")
8. **Videos in a playlist were queued as tracks**: tidalt loaded playlists from `/v1/playlists/{uuid}/tracks`, which returns music videos alongside tracks with no type field ("Facts"), so a video entry was queued as a track and sent to the track stream request (inferred from the code, not reported). tidalt's `limit=1000` there may also have been refused, as `/items` refuses it (not verified) → `/items`, `type == "track"` only, pages of 100 (AC17)
9. **Playing from a list played only that one track** (`59c610d`): nothing was loaded into the queue, so auto-advance had nothing to follow → loading a list always loads the whole list and starts at the chosen entry (AC1)

## Behaviour

### The queue

The queue is an ordered list of **entries**. Each entry holds a track (ID, title, artists, album, duration if known, streamable), whether autoplay **suggested** it (below), and an **entry ID** that is unique for the life of the player, so the same track can be queued twice and still be addressed unambiguously. One entry is **current** (or none, when the queue is empty).

The queue has two orders: the **original** order (as loaded and edited) and the **play order** (equal to the original when shuffle is off). Next, previous, auto-advance and the TUI all use the play order. Snapshots send the play order only.

| Operation | Effect |
|---|---|
| Load *tracks*, starting at *i* | Replaces the queue. Entry *i* becomes current and plays from the start. With shuffle on, entry *i* comes first in the play order and the rest is shuffled |
| Add *tracks* next | Inserted right after the current entry, in both orders, in the given order (empty queue: becomes the queue, the first one current, nothing starts) |
| Add *tracks* at the end | Appended to both orders |
| Remove *entry* | Removed from both orders. Removing the current entry while playing or paused moves on as **Next** does (stopping if there is no next); while stopped, the next entry (or none) becomes current |
| Clear | Removes every entry except the current one, which keeps playing |
| Play *entry* | Makes it current and plays it from the start |

### Playback state

`Stopped` → (`Loading` → `Playing` ⇄ `Paused`), with `Buffering` as a sub-state of `Playing` while the engine reports it (0003 AC23).

- **Loading**: the stream is being resolved and opened; nothing plays yet. The position is the start position
- **Play/pause** (`TogglePause`): Playing → Paused; Paused → Playing; Stopped with a current entry → play it from the start; Loading → the stream keeps loading but is held: it starts only when toggled again, so a pause during loading never lets a burst of audio through
- **Next**: the entry after the current one in play order, per the repeat table below, starts playing (also from Paused). If there is none, playback stops on the current entry at 0:00
- **Previous**: if the position is **more than the restart threshold** (3 s by default, a setting; `0` means previous always goes back), seek to 0:00; otherwise the entry before the current one in play order starts playing (repeat *queue* wraps from the first to the last); on the first entry without wrap, seek to 0:00
- **Seek** by ± the seek step (5 s by default, a setting; `SeekBy`) or to a position (`SeekTo`): clamped below at 0:00; a target at or past the end ends the track, which then auto-advances as a natural end does. Keeps Playing or Paused. While Loading, the target becomes the start position. While Stopped, ignored
- **Auto-advance**: when a track ends naturally (engine `TrackEnded`, or a gapless `Transitioned`), the next entry per the repeat table plays

### Shuffle and repeat

- **Shuffle** (`ToggleShuffle`): *on* puts the current entry first in the play order and shuffles all the other entries (Fisher–Yates, from a seed the runtime gives the player; tests fix it). *Off* restores the original order; the current entry keeps playing in both cases. Entries added while shuffled go where the queue table says, in both orders; they are not shuffled in. There is a single shuffle mode (tidalt's *Random* mode is dropped: with a fixed play order, previous needs no history stack)
- **Repeat** (`CycleRepeat`): `off` → `queue` → `track` → `off`

| Repeat | Natural end of the current track | `Next` | End of the last entry (play order) |
|---|---|---|---|
| `off` | next entry | next entry | autoplay on: the suggested tracks (below); otherwise, or with no suggestions, stop on the last entry at 0:00 |
| `queue` | next entry | next entry | first entry (same play order; not reshuffled) |
| `track` | the same entry again | next entry (as `queue`) | — (the track repeats) |

### Autoplay

When **autoplay** is on (`ToggleAutoplay`; off by default, a setting) and repeat is `off`, the queue does not end: when the last entry (in play order) reaches the preload point (30 s left, or `Started` for an unknown duration), the player asks for suggestions seeded by that entry's track (effect `FetchSuggestions { seed, tag }`). The result is appended to the queue as **suggested** entries, minus tracks already in the queue, and the first of them is preloaded as usual, so the transition is gapless. Suggested entries are ordinary entries afterwards (they can be played, removed, shuffled), and the TUI marks them. Repeat `queue` or `track` takes precedence: autoplay only acts where the queue would otherwise stop.

- At most one request per seed entry; turning autoplay off drops a pending request's result, and on again at the preload point asks again
- An error, or no new tracks: nothing is appended, the queue stops at its end as with autoplay off, and the message says `Autoplay: no suggestions (<reason>)`. Never retried in a loop
- Suggestions come from `GET {api_base}/tracks/{seed}/radio?countryCode=…&limit=100`: one request, no paging (Tidal caps it at 100 tracks, the seed first). The track's mix (`mixes.TRACK_MIX`) returns the same 100 tracks in the same order ("Facts"), so the radio, which needs only the track ID, is used. `404` (`Track radio cannot be generated…`) counts as "no suggestions"

### Gapless and preloading

When the current track has **30 s or less** left (or as soon as it starts, if its duration is unknown) and a next entry exists per the repeat table, the player resolves that entry's stream and sends the engine `Preload`. The engine then moves into it gaplessly when formats match (0003 AC17). If the next entry changes after that (queue edit, shuffle, repeat), the player resolves and preloads the new one, or sends `CancelPreload` when there is none any more. A failed preload resolution is not reported until the track would have started: the player then handles it as a failure of that entry (below).

### Failures

| What failed | Examples | What the player does |
|---|---|---|
| This track only | `NotFound`, `NotAvailable`, `PreviewOnly`, `Unsupported` (resolve); `Decode`, `Unsupported` (engine) | Shows the 0003 message, then moves on as auto-advance would (repeat `track` moves on too). After **5** consecutive failures, or as many as there are entries if fewer, it stops on the failing entry with `Stopped: N tracks in a row could not be played` (when N is 1, a queue of one, the failure's own message stays instead) |
| Something transient | network error after the retries (`SourceError::Network`), `429`, `5xx` | Stops (state `Stopped`) on that entry, at the position reached, with the 0003 message. **Never skips.** Play/pause retries it |
| The output | `Busy`, `NotFound`, `Lost` | Stops on that entry with the 0003 message. Never skips (the next track would fail the same way) |
| The session | `LoginRequired` | Stops; the TUI shows the "session expired" status (0002 AC14) |

The message stays in the snapshot until the next track starts or the next failure replaces it. A successful `Started` resets the failure count.

### Volume

- `0`–`100` %, in steps of the **volume step** (5 by default, a setting, 1–25; `ChangeVolume(±step)`), clamped; **default 100 %**. Mute (`ToggleMute`) sets the gain to 0 and keeps the volume, so unmuting restores it; changing the volume while muted unmutes. Not remembered across runs (0009)
- Software gain in the engine: `gain = (volume / 100)³` (a perceptual curve: 50 % ≈ −18 dB; decision 2), 0 when muted. At gain 1 the engine does not touch the samples
- `bit_perfect` (0003 "Output kinds") gains a last condition, *and the gain is 1*: below 100 % the snapshot reports `bit_perfect: false` with the reason `volume below 100%`, and `muted` when muted

### Engine additions (`tidal-player-audio`)

| Change | Why |
|---|---|
| `Play` and `Preload` take a `tag: u64`; `Started`, `Transitioned`, `TrackEnded` and `Error` carry the tag of the track they are about | The player can tell an event of the track it just replaced from one of the track it started (tidalt bug 1) |
| `CancelPreload` | Forgets the preload (closing its source); at the end of the current track the engine drains and sends `TrackEnded` as if none had been sent |
| `SetGain(f32)` (0.0–1.0) | Applied to every sample before packing, from the next write (within one 50 ms write slice); kept across tracks, transitions and `SetDevice`. Gain exactly 1.0 passes samples through untouched. Otherwise `round(s × gain)` in `f64`, no dither |

### Player and protocol (`tidal-player-core`)

`tidal_player_core::player::update(&mut PlayerState, PlayerInput) -> Vec<PlayerEffect>`, pure, as 0001 laid out:

- `PlayerInput`: a client `Command`; an engine event, mapped by the runtime into core types (tag, source format, output info, error kind and message); a stream resolution result (entry, tag, granted quality, or error kind and message); a suggestions result (tag, tracks or error message)
- `PlayerEffect`: `Resolve { entry, tag, purpose: Play | Preload }`, `EnginePlay { tag, start_at }`, `EnginePreload { tag }`, `EngineCancelPreload`, `EnginePause`, `EngineResume`, `EngineSeek`, `EngineStop`, `EngineSetGain(f32)`, `FetchSuggestions { seed, tag }`, `Broadcast(Event)`. The runtime keeps the resolved stream for a tag and hands it to the engine with `EnginePlay`/`EnginePreload`; core never sees a URL
- Each `Resolve` gets a fresh tag. A resolution result or an engine event with any tag other than the current track's (or the pending preload's) changes nothing

New `protocol::Command` variants (client → player): `LoadQueue { tracks, start }`, `AddToQueue { tracks, at: Next | End }`, `RemoveFromQueue(EntryId)`, `ClearQueue`, `PlayEntry(EntryId)`, `TogglePause`, `Next`, `Previous`, `SeekBy(milliseconds, signed)`, `SeekTo(Duration)`, `ToggleShuffle`, `CycleRepeat`, `ToggleAutoplay`, `ChangeVolume(i8)`, `SetVolume(u8)`, `ToggleMute`. Toggles rather than "set" variants, so two clients acting on a stale view (0005) cannot fight; MPRIS (0010) may add setters.

New `protocol::Event` variants (player → clients):

- `Player(PlayerSnapshot)`: the whole player state, sent once after every input that changed anything but the position: queue (play order, with entry IDs and the suggested mark), current entry ID, state, position, shuffle, repeat, autoplay, volume, muted, the now-playing details (granted quality, the 0003 "Track" and "Output" descriptions, `bit_perfect` and its reason as above) and the message, if any. Clients replace their copy with it and never reorder it
- `Position { entry, position }`: forwarded from the engine's `Position` events (every ≤ 250 ms while playing), without a full snapshot

### Filling the queue (until 0006/0007)

From the command line (`tidal-player [ITEM]...`, `play <ITEM>...`) or from the TUI's **open** prompt (`o` / `O`, below). An **item** is a Tidal track ID (a bare number, as in 0003) or a Tidal link to a track, album or playlist:

| Item | Becomes |
|---|---|
| `77640617` | track 77640617 |
| `https://tidal.com/browse/track/77640617`, `https://tidal.com/track/77640617`, `https://listen.tidal.com/track/77640617`, `tidal://track/77640617`, any of these with the share menu's `/u` suffix (`https://tidal.com/track/77640617/u`), a trailing slash or a query string | track 77640617 |
| `https://tidal.com/browse/album/123`, `…/album/123/`, `https://tidal.com/album/123/u` | every track of album 123, in album order |
| `https://tidal.com/browse/playlist/<uuid>`, `…/playlist/<uuid>/u` | every track of the playlist, in playlist order; videos are left out |
| anything else (an artist or mix link, a video, a typo) | refused before anything plays: `Not a Tidal track, album or playlist: <item>` (exit 2) |

Items are expanded in the order given into one list; the metadata (title, artists, album, duration) comes from the API at the same time:

- `GET {api_base}/tracks/{id}?countryCode=…`
- `GET {api_base}/albums/{id}/tracks?countryCode=…&limit=100&offset=…`, every page
- `GET {api_base}/playlists/{uuid}/items?countryCode=…&limit=100&offset=…`, every page, keeping only `type == "track"` items (`item` holds the track)

Pages are requested with `limit=100` (the largest the playlist endpoint accepts, "Facts") and `offset` advanced by the number of items received, until `offset ≥ totalNumberOfItems` or a page comes back empty. Each track maps to: `id`, `title`, `artists[].name` (in order), `album.title`, `duration` (whole seconds), and **streamable** = `allowStreaming && streamReady`. A track that is not streamable is still queued and shown, but the player handles it as a track-only failure (`Track 123 is not available in <country>`) without resolving it.

### Settings (until spec 0008 adds `app.toml`)

Like 0003's, from the environment until 0008 moves them into `app.toml` (keeping the variables working). An empty variable counts as unset; an invalid value exits 2 naming the variable and the accepted range, before anything starts.

| Setting | Environment | Accepted | Default |
|---|---|---|---|
| Volume step (%) | `TIDAL_PLAYER_VOLUME_STEP` | integer 1–25 | `5` |
| Seek step (s) | `TIDAL_PLAYER_SEEK_STEP` | integer 1–600 | `5` |
| Previous restarts the track after (s) | `TIDAL_PLAYER_PREVIOUS_RESTART` | integer 0–60; `0` = previous always goes back | `3` |
| Autoplay at start | `TIDAL_PLAYER_AUTOPLAY` | `on`, `off` | `off` |

The player takes them as a `PlayerConfig` at start; `ToggleAutoplay` changes autoplay for the running player only (remembering it is 0009).

### Commands

| Command | What it does |
|---|---|
| `tidal-player [--add-to-queue \| --play-next] [ITEM]...` | Starts the TUI (standalone: player and TUI in one process). With items, it loads them into the queue and starts playing the first; without, it starts with an empty queue. `--add-to-queue` adds the items at the end of the queue and `--play-next` right after the current entry (`AddToQueue { at: End \| Next }`) instead of replacing it; the two are mutually exclusive (exit 2) and need at least one item (exit 2). Until 0005 lets a second invocation reach a running player, the queue is always empty at start, so both behave like the plain form (the first added track starts); 0005 sends the same command to the running daemon. Quality and device come from the environment, as for `play` (0003 "Settings") |
| `tidal-player play <ITEM>... [--shuffle] [--repeat off\|queue\|track] [--autoplay] [--quality Q] [--device PCM] [--start SECONDS]` | 0003's headless `play`, now taking several items and playing them as one queue through the same player code. `--start` applies to the first track. The "Track" and "Output" lines are printed again at each track start (and the progress line redraws for the current track). With `--autoplay` (or `TIDAL_PLAYER_AUTOPLAY=on`) it keeps going on suggestions until Ctrl-C. Exit `0` when the queue ran out and at least one track played to its end; `1` when it stopped on a failure (message on stderr, as in 0003) or no track could be played; `2` bad arguments or items; `130` Ctrl-C. With one track ID and no new flag, its behaviour and output are exactly 0003's |

### TUI

The screen is one bordered frame titled `tidal-player`, as now. The **playback window** (spotify-player's, at the top) takes 4 rows inside the frame; the **queue** fills the rest. The two never share rows: anything drawn next to the now-playing text (cover art, when a later spec adds it) stays inside the playback window's rows, and the queue list always spans the full width of its own rows (decision 6: tidalt's cover panel beside the queue list glitched as the list redrew)

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Hell Above · Pierce The Veil                     shuffle  repeat: queue  80%│
│  Collide With The Sky                                                        │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: volume below 100% │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━──────────────────────  1:23 / 3:32│
│┌Queue (12)──────────────────────────────────────────────────────────────────┐│
││  1  King For A Day         Pierce The Veil            3:52                  ││
││▶ 2  Hell Above             Pierce The Veil            3:32                  ││
│…                                                                             │
```

- State symbol: `▶` playing, `⏸` paused, `…` loading or buffering, `■` stopped; `Nothing playing` when the queue is empty
- Indicators: `shuffle` only when on, `repeat: queue`/`repeat: track` only when not off, `autoplay` only when on, the volume, `muted` instead of it when muted
- The third row is the now-playing details; the message (failures) replaces it while there is one
- The progress row draws no bar when the duration is unknown (`1:23 / ?:??`)
- The queue lists the play order, the current entry marked `▶` and kept in view when it changes; the cursor moves independently. Suggested entries follow a `Suggested` divider row and are drawn dimmed
- **Open prompt** (`o`: add to the end of the queue; `O`: play next, i.e. right after the current entry): a one-row input over the queue's top row, `Add to queue: ` or `Play next: `, taking a track ID or a Tidal track/album/playlist link, typed or pasted (bracketed paste). `Enter` expands it (as on the command line) and adds the tracks at the end of the queue (`o`) or after the current entry (`O`); if nothing is playing, the first added track starts. `Esc` closes it. While it is open, keys type into it; an invalid item or a fetch error closes it and shows the message in the playback window
- Narrow or short terminals: columns are truncated with `…`, the album column goes first, then the artist; below 6 inner rows only the playback window is drawn; nothing panics at any size (0 × 0 included)

Keys (spotify-player's defaults; hardcoded until 0008 makes them configurable):

| Key | Does |
|---|---|
| `Space` | play/pause |
| `n` / `p` | next / previous |
| `>` / `<` | seek forward / backward by the seek step |
| `^` | seek to start |
| `C-s` | toggle shuffle |
| `C-r` | cycle repeat |
| `A` | toggle autoplay |
| `o` | add a link or ID to the end of the queue (prompt) |
| `O` | add a link or ID to play next (prompt) |
| `+` / `-` | volume up / down by the volume step |
| `_` | mute / unmute |
| `j`/`↓`, `k`/`↑`, `g g`, `G` | move the queue cursor (down, up, top, bottom) |
| `Enter` | play the entry under the cursor |
| `q`, `Esc` | quit (unchanged); the player stops and the device is released first |

## Acceptance criteria

Player state machine (`tidal_player_core::player`, pure; tests drive `update` with inputs and assert the state and the effects):

- **AC1** — `LoadQueue { tracks, start }` replaces the queue, makes entry `start` current, emits `Resolve { purpose: Play }` for it with a fresh tag and enters `Loading`; a successful resolution for that tag emits `EnginePlay { tag, start_at: 0 }`; engine `Started` with that tag enters `Playing`. `Next`, `Previous` (at or below the restart threshold), `PlayEntry` and auto-advance follow the same path from any state, so a skip never waits for the current track to end (tidalt bugs 4 and 8)
- **AC2** — Next, previous and the natural end follow the tables under "Playback state" and "Shuffle and repeat" (table over repeat `off`/`queue`/`track` × current entry first/middle/last × shuffle off/on × position at or below / above the restart threshold for previous, with thresholds 3 s and 0). Stopping at the end leaves the last entry current at 0:00 in `Stopped`, and `TogglePause` then plays it from the start
- **AC3** — `ToggleShuffle` on: the current entry is first in the play order, every entry appears exactly once, the order is a function of the seed (two seeds give different orders for a 10-entry queue; the same seed the same order), and no effect touches the engine (the current track keeps playing). Off: the original order, current unchanged. Snapshots carry the play order. Load with shuffle on starts at the chosen entry
- **AC4** — `CycleRepeat` goes `off → queue → track → off`; each change is in the next snapshot
- **AC5** — Preload: a `Position` that leaves 30 s or less (or `Started` for an unknown duration) emits `Resolve { purpose: Preload }` for the next entry per the repeat table, once; its result emits `EnginePreload` with its tag; `Transitioned` with that tag makes it current without any `EnginePlay`. A queue edit, shuffle or repeat change that changes the next entry after that emits a new `Resolve`/`EnginePreload`, or `EngineCancelPreload` when there is no next entry; one that does not change it emits nothing. No preload is requested when there is no next entry
- **AC6** — Stale inputs change nothing and emit nothing: a resolution result, `Started`, `Transitioned`, `TrackEnded` or `Error` whose tag is neither the current track's nor the pending preload's (table; the case of tidalt bug 1: `Next`, then the replaced track's `TrackEnded`, gives exactly one advance)
- **AC7** — Failures follow the table under "Failures" (table over every error kind × resolve/engine): a track-only failure advances and shows the message; a transient, output or session failure stops on that entry, shows the message and emits no `Resolve` for another entry; 5 consecutive track-only failures (or the queue length, if smaller; repeat `queue` and `track` included) stop with `N tracks in a row could not be played`; a `Started` resets the count; an entry whose track is not streamable is a track-only failure with no `Resolve` sent for it. An `Error` is never handled as `TrackEnded`, nor the other way round
- **AC8** — `TogglePause` and seeking follow "Playback state": Playing ⇄ Paused emits `EnginePause`/`EngineResume`; toggled while `Loading`, the successful resolution emits no `EnginePlay` until toggled again; `SeekBy(-5000)` at 0:03 seeks to 0:00 (the client sends `SeekBy(±seek step)`; AC20 checks it uses the configured step); `SeekBy`/`SeekTo` while `Loading` change the `start_at` of the coming `EnginePlay`; while `Stopped` they emit nothing
- **AC9** — Volume: default 100, not muted; `ChangeVolume(±n)` clamps to 0–100 (with n = 1, 5 and 25); `SetVolume` sets it; each change emits `EngineSetGain((v/100)³)` (0 when muted); `ToggleMute` twice restores the previous gain; a volume change unmutes. The snapshot's `bit_perfect` is the engine's at volume 100 and not muted, else `false` with `volume below 100%` / `muted`
- **AC10** — Queue edits follow the table under "The queue" (table over each operation × current entry before/at/after the edit × shuffle off/on): entry IDs are never reused, the same track can be queued twice, removing the current entry while playing advances (and stops with nothing next), `ClearQueue` keeps only the current entry and does not interrupt it, editing an empty queue starts nothing
- **AC11** — Every input that changes anything but the position emits exactly one `Broadcast(Event::Player(snapshot))`, as the last effect, with the new state; an engine `Position` emits only `Broadcast(Event::Position { entry, position })`; an input that changes nothing emits nothing
- **AC12** — Every new `Command` and `Event` variant survives a JSON round-trip (the 0001 AC11 test, extended)

Engine (`tidal-player-audio`, with 0003's fakes):

- **AC13** — `Started`, `Transitioned`, `TrackEnded` and `Error` carry the tag given with the `Play` or `Preload` of their track, also when a `Play` replaces a playing track and when a preloaded track fails after the transition
- **AC14** — `CancelPreload`: the cancelled source is closed; at the end of the current track the engine drains and sends `TrackEnded` (its tag), and the sink never receives a frame of the cancelled track. `CancelPreload` with nothing preloaded does nothing
- **AC15** — `SetGain`: at 1.0 the sink receives exactly the bytes it receives without any `SetGain` (bit-exact, 16- and 24-bit fixtures); at 0.5 each sample is `round(s × 0.5)`; at 0 silence; the new gain applies to frames written after the command, at most one write slice (50 ms of audio) later; the gain stays across a gapless transition, a new `Play` and `SetDevice`

Items and metadata:

- **AC16** — `parse_item(&str) -> Result<Item, ItemError>` (in `tidal-player-core`) maps every form in the table under "Filling the queue" (table, including trailing slashes, the share menu's `/u` suffix, query strings, `www.`, upper-case hosts, a bare number) and refuses artist, mix and video links, other hosts and junk
- **AC17** — `tidal-player-api` gets `get_track`, `get_album_tracks` and `get_playlist_tracks` returning `tidal_player_core::Track` (ID, title, artists, album title, duration if present, streamable), through the `Authenticator` with `countryCode` from the session (wiremock fixtures from the probe): album and playlist fetches walk every page until `totalNumberOfItems` (`limit=100`; one request for one page, three for a list of 250 items, and an empty page ends the walk early); playlist items other than `track` are dropped; `allowStreaming: false` or `streamReady: false` gives `streamable: false`; `404` (`subStatus` 2001) maps to `MetadataError::NotFound`; `LoginRequired` and transient errors are returned unchanged

CLI and runtime (`tidal-player`):

- **AC18** — `play` with several items: expands them in order (fake metadata source), loads them as one queue and plays it through `tidal_player_core::player` (fake engine): the "Track"/"Output" lines are printed at each track start; `--shuffle` and `--repeat` set the player before loading; exit codes as under "Commands" (table: all play → 0; one track-only failure among three → 0 and the message on stderr; output busy → 1; every track fails → 1; bad item → 2). 0003's `play` tests pass unchanged
- **AC19** — The player runtime executes effects in order and maps engine events and resolutions back into inputs with their tags (fake engine and fake resolver: a `Next` mash of 5 presses while loading resolves all 5 but plays only the last; the device is stopped when the TUI quits). `tidal-player [ITEM]...` starts the TUI with the queue loaded; without items, empty

TUI (`tidal_player_core::ui` and `tidal-player`'s rendering):

- **AC20** — Key → action → command table: each key under "Keys" maps to its `Command` (or cursor move); `Enter` sends `PlayEntry` with the entry ID under the cursor (not its index); no key sends anything on an empty queue except the volume and mode keys
- **AC21** — `g g` is a two-key sequence: `g` then `g` within the same sequence moves to the top; `g` followed by any other key does that key's action
- **AC22** — The client state takes each `Event::Player` snapshot as is (the queue order displayed is exactly the snapshot's; the client never shuffles: tidalt bug 5); `Event::Position` for another entry than the snapshot's current one is ignored; the cursor stays on the same entry ID across snapshots (clamped when it was removed)
- **AC23** — Rendering snapshots (`insta`, 80×24): nothing playing; playing with shuffle, repeat `queue`, 80 % and the `volume below 100%` reason; paused; loading; a failure message; unknown duration (no bar, `?:??`: tidalt bug 6); a queue longer than the window with the current entry kept in view. 40×12: truncation order. No panic from 0×0 to 120×40 (table over sizes)
- **AC24** — `docs/playback.md` documents items, the new `play` flags and the settings; `docs/tui.md` documents the screen, the keys, the open prompt, shuffle/repeat/autoplay/volume semantics and the failure behaviour; this spec links to both

Added with the decisions (2026-10-07):

- **AC25** — Settings: `resolve_player_config(env)` (pure) gives the defaults with nothing set, each value within its range, and exit-2 errors naming the variable for out-of-range, non-numeric and unknown values (table, including empty = unset). `play --autoplay` beats the environment. The TUI's `+`/`-` and `>`/`<` send the configured steps; the player's previous uses the configured threshold
- **AC26** — Autoplay (player): with autoplay on and repeat `off`, the last entry reaching the preload point emits `FetchSuggestions { seed: its track, tag }` once; a result for that tag appends the tracks not already queued as suggested entries and preloads the first; an error or an empty (or all-duplicate) result appends nothing, sets the `Autoplay: no suggestions (…)` message and the queue stops at its end; a stale tag, or a result arriving after `ToggleAutoplay` off, changes nothing; with repeat `queue`/`track` or autoplay off, no `FetchSuggestions` is ever emitted (table)
- **AC27** — `tidal-player-api` gets `get_suggestions(track)`: exactly one `GET /tracks/{id}/radio` with `countryCode` and `limit=100` (wiremock fixture from the probe); the tracks in order, mapped as in AC17 (the seed included: the player's de-duplication drops it); `404`/`2001` → an empty list; `LoginRequired` and transient errors returned unchanged
- **AC28** — Open prompt (client state, pure): `o` opens it in add-to-queue mode and `O` in play-next mode; typed characters and a paste edit it; `Backspace` deletes; `Esc` closes it with no effect; `Enter` with a valid item closes it and emits `Effect::Expand { item, at: End | Next }` per mode; with an invalid one, closes it and sets the `Not a Tidal track, album or playlist` message. The client runtime turns the expanded tracks into `AddToQueue { at }`, followed by `PlayEntry` of the first added entry when the player is `Stopped` with nothing current or the queue was empty (fake metadata). While the prompt is open no key reaches the player (`Space` types a space). CLI: `--add-to-queue` and `--play-next` parse into the matching `at`, are mutually exclusive and need an item (exit 2, `crates/app/tests/cli.rs`)

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| Empty queue | Every playback key does nothing; volume and the shuffle/repeat/autoplay modes still change (and apply to the next load); `o`/`O` open the prompt |
| Queue of one, repeat `queue` | The track repeats (as `track`), gaplessly |
| Every track of an album unavailable in the country | 5 failures (or fewer, the album's length), then `Stopped: N tracks in a row could not be played`; no API storm (tidalt bug 3) |
| `429` or network down during resolution | Stops on that entry with the message; play/pause retries. Never skips |
| DAC unplugged mid-queue | `Output hw:1,0 was lost`; stops; the queue is kept; play/pause retries on the same device |
| Session expires mid-queue | Stops; the TUI shows the session-expired status; after `tidal-player login` in another terminal (0002 AC8), play/pause resumes |
| Next mashed while loading | Each press supersedes the last; only the last entry plays (stale resolutions are dropped, AC6, AC19) |
| Preloaded next entry removed from the queue | `CancelPreload` or a new preload (AC5); the removed track never plays |
| Seek past the end, repeat `track` | The track starts again |
| Stream URL of a preload expired by the time it plays | 0003's re-resolve on `403`/`410` covers it (preloads happen ≤ 30 s before use) |
| Track with no duration in its metadata | Progress shows `?:??` and no bar; preload starts at `Started` |
| Pause on a device without hardware pause | As 0003 "Edge cases" (about 0.1 s keeps playing) |
| Album or playlist with more than one page of tracks | All pages are fetched before playing (AC17) |
| An item that does not exist (`404`/`2001`) | Nothing plays: `Track 1 was not found` / `Album 1 was not found` / `Playlist <uuid> was not found` (`play` exits 1; the TUI starts with an empty queue and shows the message) |
| A track the metadata marks not streamable | Queued and shown; skipped as a track-only failure without a stream request (AC7) |
| A playlist with videos | Videos are left out; an all-video playlist is refused: `Playlist <uuid> has no tracks` (exit 2 / message in the TUI's startup) |
| Autoplay with a seed Tidal has no suggestions for | `Autoplay: no suggestions (…)`, stops at the end (AC26) |
| Open prompt with a link to an artist or a video | `Not a Tidal track, album or playlist: …`; the queue is unchanged (AC28) |
| Terminal smaller than the layout | Truncated, then only the playback window; never a panic (AC23) |

## Test plan

Each automated test is named after its criterion (`ac5_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0003. The player tests build inputs by hand and never run a thread; time is only what `Position` inputs say. API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/metadata/`, written from the probe's recorded shapes (IDs and names replaced by fakes). Engine tests reuse 0003's fixtures and fakes.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/player.rs` :: `ac1_load_and_skip_paths` (table: load, next, previous, play entry, auto-advance; from each state) | effects `Resolve` → `EnginePlay` → state `Playing`; fresh tag each time | stub `update` returns no effects |
| AC2 | `crates/core/src/player.rs` :: `ac2_next_previous_end` (table, as in AC2) | the entry that plays next, or the stop state | stub always plays index + 1 and never stops or wraps |
| AC3 | `crates/core/src/player.rs` :: `ac3_shuffle` | first entry, permutation, seed dependence, no engine effect, original restored | stub leaves the order unchanged |
| AC4 | `crates/core/src/player.rs` :: `ac4_repeat_cycle` | the cycle and the snapshot field | stub keeps `off` |
| AC5 | `crates/core/src/player.rs` :: `ac5_preload` (table: 31 s → 30 s left, unknown duration, no next, next changed by each edit/mode, next unchanged) | `Resolve{Preload}` once, `EnginePreload`, `Transitioned` handling, `EngineCancelPreload` | stub never preloads, so `Transitioned` is unknown and the track ends with a gap |
| AC6 | `crates/core/src/player.rs` :: `ac6_stale_inputs_ignored` (table) | no effects, state unchanged; exactly one advance in tidalt's sequence | stub matches events without looking at tags, so the stale `TrackEnded` advances twice |
| AC7 | `crates/core/src/player.rs` :: `ac7_failure_policy` (table over error kinds × source) + `ac7_failure_run_stops` | skip or stop per row; message; the run limit with queue lengths 3 and 20 | stub skips on every error (tidalt's loop) |
| AC8 | `crates/core/src/player.rs` :: `ac8_pause_and_seek` (table) | effects per state; held play; clamping; `start_at` | stub ignores the state and always sends `EnginePause` |
| AC9 | `crates/core/src/player.rs` :: `ac9_volume` (table) | volume, gain values, mute round trip, `bit_perfect` and reason | stub never emits `EngineSetGain` and keeps the engine's `bit_perfect` |
| AC10 | `crates/core/src/player.rs` :: `ac10_queue_edits` (table) | orders, current entry, IDs, effects | stub appends everything at the end |
| AC11 | `crates/core/src/player.rs` :: `ac11_one_snapshot_per_change` (over every row of AC1–AC10's tables) | exactly one `Broadcast(Player)`, last; `Position` events alone for positions; nothing for no-ops | stub broadcasts after every input |
| AC12 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (0001's, extended; `all_commands`/`all_events` list the new variants) | round trip | new variants' hand-written stub `Serialize` writes `null` |
| AC13 | `crates/audio/tests/engine.rs` :: `ac13_events_carry_tags` | tags on each event, incl. replace and failed preload | stub engine sends tag 0 |
| AC14 | `crates/audio/tests/engine.rs` :: `ac14_cancel_preload` | drain, `TrackEnded`, no frame of the cancelled source, source closed | stub ignores `CancelPreload`, so the sink gets the second track |
| AC15 | `crates/audio/tests/engine.rs` :: `ac15_gain` (table: 1.0 on 16/24-bit, 0.5, 0, change mid-track, across transition/play/device) | bytes per row; latency in frames | stub ignores `SetGain` |
| AC16 | `crates/core/src/item.rs` :: `ac16_parse_item` (table) | item or error per row | stub parses bare numbers only |
| AC17 | `crates/api/tests/metadata.rs` :: `ac17_get_track`, `ac17_album_pages`, `ac17_playlist_pages_and_videos`, `ac17_errors` | request paths and params (`expect(n)`), mapped tracks, errors | stub reads the first page only and keeps videos |
| AC18 | `crates/app/src/play.rs` :: `ac18_queue_exit_codes` (table, fake metadata + fake engine) + `crates/app/tests/cli.rs` :: `ac18_bad_item` | lines printed, exit codes; `Not a Tidal…` with exit 2 | stub plays the first item only |
| AC19 | `crates/app/src/player_runtime.rs` :: `ac19_effects_and_mash`, `ac19_quit_stops_engine` | effect execution order; one `Play` after the mash; `Stop` before exit | stub runtime plays every resolution it receives |
| AC20 | `crates/core/src/ui.rs` :: `ac20_keys_to_commands` (table) + `crates/app/src/input.rs` :: `ac20_key_events` | commands per key; entry ID for `Enter` | stub maps only `q`/`Esc` |
| AC21 | `crates/core/src/ui.rs` :: `ac21_key_sequence` | `g g` → top; `g` `j` → down | stub treats `g` alone as top |
| AC22 | `crates/core/src/ui.rs` :: `ac22_snapshot_applied_as_is` | displayed order equals the snapshot's; stale positions ignored; cursor follows entry ID | stub sorts the queue by original position |
| AC23 | `crates/app/src/ui.rs` :: `ac23_playback_window_*`, `ac23_queue_scrolls`, `ac23_truncation_40x12`, `ac23_no_panic_any_size` | `contains` assertions per state (symbol, `?:??`, reason text) plus snapshots reviewed by eye at acceptance | stub `render` draws only the frame |
| AC24 | — reviewed at acceptance | docs exist, match this spec, are linked | — |
| AC25 | `crates/app/src/play.rs` :: `ac25_player_config` (table) + `crates/core/src/ui.rs` :: `ac25_steps_from_config` | values, errors and precedence; commands carry the configured steps | stub ignores the environment and returns the defaults |
| AC26 | `crates/core/src/player.rs` :: `ac26_autoplay` (table: on/off × repeat × result ok/empty/duplicates/error/stale/after toggle-off) | `FetchSuggestions` once, appended entries and their mark, preload, message, stop | stub never fetches, so the queue stops at its end with autoplay on |
| AC27 | `crates/api/tests/metadata.rs` :: `ac27_suggestions` | path and params, mapped tracks, errors | stub returns an empty list without a request, so the `expect(1)` fails |
| AC28 | `crates/core/src/ui.rs` :: `ac28_open_prompt` (table) + `crates/app/src/player_runtime.rs` :: `ac28_open_adds_and_starts` | prompt state and mode, effects; commands sent after expansion (`End` and `Next`) | stub `o`/`O` do nothing |

Not covered by automated tests, on purpose, and checked by hand at acceptance with a real account (results in the PR description):

- Gapless by ear across two tracks of a live or classical album through `play <album link>` on a DAC (0003's open manual check), and `/proc/asound/cardN/pcm0p/sub0/hw_params` unchanged across the transition
- Volume audibly changes in steps; at 100 % the "Output" line still says `bit-perfect` on `hw:`
- Next/previous/seek respond immediately in the TUI while a hi-res track streams
- Album and playlist links from the Tidal app's share menu, pasted as is (on the command line and into the open prompt)
- Autoplay: an album played to its end continues with suggestions, gaplessly, and they look related

## Crate placement

- `tidal-player-core`: `player` (state machine), `item` (`parse_item`), `Track`/`EntryId` types, `protocol` variants, `ui` (client state, key sequences, queue cursor). Shuffling uses a small seeded PRNG written in `core` (a few lines of xorshift/PCG); no new dependency
- `tidal-player-api::metadata`: the three fetches, `get_suggestions`, and their DTO → `core::Track` mapping
- `tidal-player-audio`: tags, `CancelPreload`, `SetGain`
- `tidal-player`: the player runtime (`player_runtime.rs`: runs `update`, executes effects against the engine, the resolver and the broadcast channel), `play` on top of it, the settings, the TUI wiring and rendering (with bracketed paste on), `input.rs` key mapping
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-07, from code):

- 0003's engine handles `Play` while playing by stopping the old track first, keeps one preload that `Play`, `Stop` and a failed track forget, and has no way to cancel a preload or tag a track (`crates/audio/src/engine.rs`): hence the engine additions
- tidalt's behaviours and bugs listed under "Context" (read from its source and history)

Verified on 2026-10-07 against the **live API** by `scripts/tidal-metadata-probe.sh`, run by the user (same account as 0003's probes, device-flow client "Android Automotive HiRes"), with track 33695188, a 17-track album and a user playlist of 39 tracks and 2 videos. Fixtures under `crates/api/tests/fixtures/metadata/` are written from these shapes with IDs and names replaced:

- `GET /v1/tracks/{id}` → `200` with `id`, `title`, `duration` (integer seconds), `version` (null here), `trackNumber`, `volumeNumber`, `allowStreaming`, `streamReady`, `audioQuality`, `mediaMetadata.tags`, `artist {id, name, type}`, `artists[] {id, name, type: MAIN…}`, `album {id, title, cover}`, plus fields this spec does not use (`replayGain`, `peak`, `isrc`, `bpm`, `mixes`, …). Unknown ID → `404 {"status":404,"subStatus":2001,"userMessage":"Track [1] not found"}`
- `GET /v1/albums/{id}/tracks` → `{limit, offset, totalNumberOfItems, items: [track…]}`, bare tracks in album order (`trackNumber` 1, 2, …). With no `limit` it returned all 17 (`limit: 17`); `limit=3&offset=2` returned tracks 3–5 with `totalNumberOfItems: 17`; `limit=1000` was accepted on this endpoint. Unknown album → `404`/`2001` (`Album [1] not found`). `GET /v1/albums/{id}/items` exists too, with `{item, type: "track"}` entries; not used
- `GET /v1/playlists/{uuid}/items` → `{limit, offset, totalNumberOfItems, items: [{item, type, cut}]}`; `item` is a track with extra `dateAdded`, `index`, `itemUuid`, `description`, and `album.releaseDate`. **Default page size 10** (so a client that does not page sees 10 items); `totalNumberOfItems` counts videos too (41 = 39 tracks + 2 videos, per the playlist's `numberOfTracks`/`numberOfVideos`); `limit=1000` → `400 {"subStatus":1001,"userMessage":"Too big page, max page size is [100]"}`. Unknown UUID → `404`/`2001` (`Playlist not found`)
- Video entries in `/items` (second run, `limit=50`, all 41 entries): `{"item": {…}, "type": "video", "cut": null}`, where `item` has `id`, `title`, `duration`, `artists`, `streamReady`/`allowStreaming` (both `true`), `quality: "MP4_1080P"`, `type: "Music Video"`, `trackNumber: 0`, `volumeNumber: 0` and **`album: null`**. Page counts: 39 `track`, 2 `video`, matching `numberOfTracks`/`numberOfVideos`
- `GET /v1/playlists/{uuid}/tracks` (tidalt's endpoint) has the same envelope with bare items and **returns the videos too** (41 items for 39 tracks + 2 videos), with nothing marking them but their shape (`album: null`, `quality`). So this spec uses `/items` and keeps only `type == "track"`. Whether `/tracks` also refuses `limit=1000` (tidalt's value) is not verified

Verified on 2026-10-07 against the **live API** by `scripts/tidal-autoplay-probe.sh`, run by the user (same account), seed track 33695188:

- `GET /v1/tracks/{id}/radio` → the album-tracks envelope with bare tracks (`{limit, offset, totalNumberOfItems, items: [track…]}`). Default page size 10; `totalNumberOfItems: 100`; `limit=100` returns all 100. The **seed track is the first item**
- `GET /v1/mixes/{TRACK_MIX}/items` (the ID from the track's `mixes.TRACK_MIX`) → `{item, type: "track"}` entries, default page size 100, `totalNumberOfItems: 100`, and **the same 100 track IDs in the same order** as the radio. So the track radio *is* the track mix; the radio is used because it needs no extra lookup
- Unknown track → `404 {"subStatus":2001,"userMessage":"Track radio cannot be generated for trackId [1]"}`; unknown mix → `404`/`2001` (`Mix items for [...] not found`)
- Which endpoint the Tidal apps' own autoplay calls is not known

Not verified (the probe did not reach it):

- A track with `allowStreaming: false` or `streamReady: false` (every track seen had both `true`)
- Albums longer than 100 tracks (paging is exercised against fixtures)

## Decisions (answered by the user, 2026-10-07)

1. **Filling the queue before 0006/0007**: *both* — items on the command line (`tidal-player [ITEM]...`, `play <ITEM>...`) and pasted into the TUI's prompt: `o` adds to the end of the queue, `O` plays next; on the command line `--add-to-queue` / `--play-next` (AC28)
2. **Volume**: software gain in the engine, cubic curve, default 100 % (bit-perfect by default). *Fair, but configurable*: the step is a setting (default 5 %, AC25). (The ALSA hardware mixer was the alternative; not taken)
3. **Previous** restarts the track when past a threshold, else goes back. *Fair, but configurable*: 3 s by default, `0` disables the restart (AC25)
4. **End of queue**: stops, unless **autoplay** is on, which continues with Tidal's suggestions as the Tidal apps do; *togglable* (`A`, `--autoplay`, a setting; AC26). Default off (proposed; the Tidal apps default to on)
5. **Failure policy**: as in "Failures". *Fair*
6. **Layout**: *either way*, but cover art (when it comes) must never share rows with the track list, as tidalt's did, which made the image glitch. Playback window at the top; the rule is in "TUI"
7. **Seek step**: 5 s. *Fair, but configurable* (AC25)

## Out of scope

- Daemon, clients over a transport, one-shot `playback` commands, multiple clients (0005)
- Library and search pages, "add to queue" from them (0006, 0007); they will use `AddToQueue` from this spec
- Configurable keys and settings in `app.toml`/`keymap.toml` (0008)
- Remembering the queue, position and volume across runs (0009)
- MPRIS and media keys (0010); setter commands for it
- Mixes and radio as pages to browse (0011); autoplay at the end of the queue is in this spec
- Cover art (its placement rule is fixed here, under "TUI")
- Switching the output device from the TUI (the engine supports it; a picker comes with 0008/0009)
- Reordering the queue by keys, saving the queue as a playlist (tidalt had both), removing entries by key (the command exists; the key comes with 0006)
- Crossfade, ReplayGain, volume ramps, dither
- Following the playing track with the cursor after idle (tidalt `4b1199d`): the current entry is kept in view instead

## Bugs

- **A queue of one failing track ends with `Stopped: 1 tracks in a row could not be played` (found at slice D acceptance, 2026-10-07).** Expected: `play <id>` on an unplayable track prints exactly 0003's message (`Track 404 is not available in NO`), as "Commands" requires for the single-track form; the TUI likewise shows why the track failed. Actual: the run limit is `min(5, queue length)` = 1, so the first failure already hits it and the player replaces the failure's message with the run summary, which names no cause (and reads "1 tracks"). Root cause: "Failures" did not say what the summary is for a run of one. Fix: the summary replaces the message only when N > 1; with N = 1 the failure's own message stays (the table row is corrected). AC7 gains the row: a 1-entry queue (repeat `off` and `queue`) stops after 1 failure with that failure's message. Test: `crates/core/src/player/tests.rs` :: `ac7_failure_run_stops` (rows `(1, 0, 1)` and `(1, 1, 1)`; red: the message is the run summary). `play`'s reporter workaround for this case is removed
- **Share-menu links are refused (reported by the user, 2026-10-07).** Expected: the link the Tidal apps' share menu copies plays, e.g. `https://tidal.com/track/145060431/u` and `https://tidal.com/album/145060429/u`. Actual: `Not a Tidal track, album or playlist: …`. Root cause: the spec's table said share links carry a `?u` query string; they carry a `/u` path segment instead (the user's report; a link copied from the address bar has no suffix), and `parse_item` accepts exactly two path segments. The table and AC16 are corrected: a trailing `/u` segment (then optionally `/`, a query or a fragment) is accepted after a track, album or playlist ID; any other extra segment is still refused. Test: `crates/core/src/item.rs` :: `ac16_parse_item`, new rows for `/u` on a track, an album and a playlist, `/u/`, `/u?x`, `/browse/…/u`, and refusals for `/x` and `/u/u` (red: the `/u` rows are refused)

