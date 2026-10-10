# 0014 — Choosing the output device while the player runs

- **Status**: draft
- **Owner**: tech-lead (primary session)
- **Depends on**: 0003 (implemented: the engine's `SetDevice`, `tidal-player devices`), 0004 (implemented: the player and its effects), 0005 (implemented: the protocol, clients, `playback`, releasing while paused), 0008 (implemented: `output_device`, the keymap, the keys help), 0009 (implemented: `playback.json`)
- **User docs**: [`docs/tui.md`](../tui.md) gains "Output device"; [`docs/daemon.md`](../daemon.md) gains `playback device`; [`docs/playback.md`](../playback.md) and [`docs/config.md`](../config.md) explain which device is used (AC12)

## Context

Today the output device is chosen once, when the player starts: `--device`, `TIDAL_PLAYER_DEVICE` or `output_device` in `app.toml` (0008), `default` otherwise. Switching from the DAC to the onboard card means editing `app.toml` (or the systemd unit) and restarting the player, which drops what is playing. The engine has supported moving a playing track to another device since 0003 (`SetDevice`, AC21: no frame lost or repeated), but nothing reaches it: 0004, 0005 and 0009 each deferred "a device picker" to a later spec, and `SwitchDevice` is one of the spotify-player commands the keymap skips (0008).

spotify-player has `SwitchDevice` (default key `D`), which opens a popup listing the Spotify Connect devices; `Enter` moves playback there. Its CLI has `spotify_player connect --name/--id` and `spotify_player get key devices`.

tidalt had a device picker in its TUI. Choosing a device in a client called the client's own player, which had no device open, so the choice did nothing (0005 Context, bug 4). Here the player alone lists devices and switches them (0005 "Sync" rule 1).

## Behaviour

### The player's device

The player has a **selected device**: an ALSA PCM name, as `--device` accepts. At start it is, from highest to lowest precedence:

1. `--device` or `TIDAL_PLAYER_DEVICE` (explicit for this run)
2. the device last chosen at runtime, from `playback.json` (decision 2), when `remember_playback` is on
3. `output_device` in `app.toml`
4. `default`

The snapshot carries it (`PlayerSnapshot.device`), so every client shows the same device.

`SetDevice(name)` (new `Command`) makes `name` the selected device, from any client:

- **Playing or buffering**: the engine moves the track to the new device at its position (0003 AC21). The now-playing details' output and bit-perfect verdict are updated from the new device's `OutputInfo` (new engine event `OutputChanged`)
- **Paused**: the old device is closed and the new one opened paused. Nothing plays until `space`
- **Released while paused** (0005), **stopped** or **loading**: nothing is opened. The next track start or resume opens the new device
- **The same device as selected**: nothing happens (no reopen)
- An empty name is refused (`Reply` error `Device name is empty`). Any other name is accepted as given. A name that does not exist fails when it is opened, like a wrong `--device`

Opening the new device can fail (busy, missing, refused reservation). The player then handles it as any output failure (0004 "Failures": stop, never skip, the queue is kept, the message names the device: `Output hw:1,0 is busy (used by …)`). The new device **stays selected**, so the user can pick another or retry with `space`. There is no automatic return to the old device (decision 3).

### Listing devices

`DeviceList` (new client request, answered to the asking client only, like `Library`) returns the devices the **player's** machine has right now: 0003's list (`default` first, then every `hw:C,D` with a playback stream and the card's name), read fresh on every request so a DAC plugged in after start shows up. The selected device is part of the snapshot. When it is not in the list (`plughw:1,0`, a custom PCM, an unplugged DAC), clients show it first, marked as selected, with the description `not found`.

### TUI: the devices popup

`D` (`SwitchDevice`, spotify-player's command name and default key) opens the **Devices** popup from any page. It is no longer one of the skipped spotify-player commands.

```
┌Devices─────────────────────────────────────────┐
│● default   shared, through the system mixer    │
│  hw:0,0    HDA Intel PCH: ALC892 Analog        │
│  hw:1,0    E30 II: USB Audio                   │
└────────────────────────────────────────────────┘
```

- `Loading devices…` until the list arrives; `Cannot list devices: <reason>` if the request fails
- `●` marks the selected device. The cursor starts on it
- `j`/`k` (and the arrow keys) move; `Enter` sends `SetDevice` for the row and closes the popup; `esc` (or `q`) closes it without changing anything; `r` reads the list again
- Choosing the selected device just closes the popup
- The keys help lists the popup's keys under its own section, as for the other popups. `D` is dim while disconnected
- The now-playing bar's output line shows the new output as soon as the engine has opened it. While nothing is open, the selected device is the one shown in the popup and in `playback status`

### CLI

| Command | What it does |
|---|---|
| `tidal-player playback device` | Asks the running player for its devices and prints them like `tidal-player devices`, with `*` on the **player's** selected device. Exit 1 with 0005's message when no player runs |
| `tidal-player playback device NAME` | Sends `SetDevice(NAME)` and exits 0 once the player has applied it, or 1 with the player's error. It does not wait for the device to open: a failure to open shows in `playback status` and the TUI, like any output failure |
| `tidal-player playback status` | The second line gains ` · hw:1,0` (the selected device) after the volume. `--json` carries it in the snapshot |
| `tidal-player devices` | Unchanged: the local list with `*` on the configured device (`--device`/environment/`app.toml`). It reads no remembered choice and contacts no player. Its help line says so and points to `playback device` |
| `tidal-player play --device PCM` | Unchanged; `--device` beats a remembered choice for that run (precedence above) |

### Remembering the choice

The selected device is saved in `playback.json` (0009) as `device`, only when it was chosen at runtime (`SetDevice`). It is not saved when it came from the flag, the environment or `app.toml`, so editing `app.toml` still takes effect unless the user has since picked a device in the player. `logout` and `remember_playback = false` forget it (0009). A file without `device` (written by an older version) reads as "nothing chosen". `version` stays 1: the field is optional.

## Acceptance criteria

- **AC1** — Protocol: `Command::SetDevice(String)`, `ClientMessage::Devices { id }`, `ServerMessage::DevicesReply { id, result: Result<Vec<DeviceEntry>, String> }` (`DeviceEntry { name, description }`) and `PlayerSnapshot.device: String` round-trip through the codec. No version field changes: 0005's greeting already refuses a client of another build
- **AC2** — Player: `SetDevice` emits `EngineSetDevice(name)` and a snapshot with the new `device` in every phase (stopped, loading, playing, buffering, paused, released). The same name as selected emits nothing. An empty name is refused and changes nothing (table over phase × name)
- **AC3** — Player: an output failure after `SetDevice` stops without skipping, keeps the queue and the new selected device, and shows 0003's message. `TogglePause` then plays the current entry on the new device. An `OutputChanged` updates `now_playing.output` and the bit-perfect verdict (volume and mute rules from 0004 still apply)
- **AC4** — Engine: `SetDevice` while paused opens the new device paused: the sink receives no frame until `Resume`, and then the frame from the paused position, nothing lost or repeated. `OutputChanged(OutputInfo)` is emitted after every reopen caused by `SetDevice` (playing or paused) and never on a track start (which has `Started`). `SetDevice` to the device already open does not reopen it
- **AC5** — Start precedence: flag/environment > remembered > `app.toml` > `default`, for the standalone TUI, `daemon` and `play` (table). `PlayerSnapshot.device` and the engine's first device agree
- **AC6** — Persistence: `saved()` holds `device` only after a `SetDevice`; a restored player starts with it (below a flag or the environment); a v1 file without `device` restores as before; `logout` and `remember_playback = false` leave no device behind (0009's AC rows extended)
- **AC7** — Listing: the player answers `Devices` with the list parsed from `/proc/asound` (`TIDAL_PLAYER_ASOUND_DIR` in tests) at request time. Changing the fixture between two requests changes the answer. A read error is the `Err` of the reply. The reply goes to the asking client only
- **AC8** — TUI model: `D` opens the Devices popup and sends `Devices`; the reply fills it, the cursor on the selected device; the selected device missing from the list is shown first as `not found`; `Enter` emits `SetDevice` for the row and closes (nothing when it is already selected); `esc`/`q` close without effect; `r` asks again; a failed reply shows its message. `SwitchDevice` is a supported keymap command (no longer in the skipped list) with default `D`, and the keys help lists the popup's keys
- **AC9** — TUI rendering: snapshots of the popup at 80 × 24 (loading, list with the selected device, a missing selected device, failure) and at 40 × 12; drawing at any size from 1 × 1 to 200 × 60 does not panic
- **AC10** — CLI: `playback device` prints the player's list with `*` on its selected device; `playback device NAME` sends `SetDevice(NAME)` and exits 0, 1 with the player's error (empty name: exit 2 before sending), 1 with 0005's message when no player runs; `playback status` shows the selected device on its second line (integration against a real daemon with fixture `asound` files and the fake engine)
- **AC11** — End to end: in a daemon with the fake engine, a TUI-side `SetDevice` reaches the engine as `SetDevice`, every subscribed client receives the snapshot with the new `device`, and an `OutputChanged` from the engine reaches them as an updated `now_playing.output`
- **AC12** — `docs/tui.md` (Output device: `D`, the popup, its keys), `docs/daemon.md` (`playback device`), `docs/playback.md` (which device is used and the precedence, `devices` vs `playback device`), `docs/config.md` (`SwitchDevice` supported; `output_device` and the remembered choice), their zh-TW copies, `examples/keymap.toml` (`D`), `README.md` and its zh-TW copy are updated; `CLAUDE.md` "Features" gains the entry

## Edge cases & errors

| Case | Behaviour |
|---|---|
| The new device is busy (another app holds the `hw:` card) | Stopped, `Output hw:1,0 is busy (used by <app>): …`; the queue and the new selection are kept; `space` retries; pick another device with `D` |
| The new device does not exist (typo through `playback device`) | Accepted; opening it fails with 0003's `No such output device hw:5,0: …`. Nothing is opened while stopped, so the error shows at the next play |
| The selected DAC is unplugged | Unchanged from 0004 (`Output hw:1,0 was lost`, stopped). It stays selected and is shown `not found` in the popup |
| `SetDevice` while released (paused, device given back) | Nothing is opened now; `space` opens the new device at the position (0005 AC19's rule: nothing opened or released twice) |
| `SetDevice` during a load | The track opens on the new device when it starts |
| Gapless preload pending | Kept: the preload plays on the new device (the engine's preload is not tied to a device) |
| Two clients pick different devices at once | Handled in order; the last one wins, and both see it in the next snapshot |
| No sound card (container, no `/proc/asound`) | The list is `default` alone, as `tidal-player devices` |
| `alsa` feature off | `SetDevice` is accepted and stored; playing fails as today (`this build has no ALSA output`) |
| A tiny terminal | The popup is clipped like the other popups; under 8 rows no page area, no popup (0004) |
| Volume below 100 % or muted | The verdict after the switch keeps 0004's reasons (`volume below 100%`) over the engine's |
| MPRIS | No change: MPRIS2 has no device property (0010) |

## Test plan

Each automated test is named after its criterion. Red is a failing assertion against stub types and functions with stub bodies (`SetDevice` ignored, `Devices` answered with an empty list, the popup draws nothing), no `todo!()` and no compile errors, as in 0001–0013. No test opens an audio device or touches the network.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac1_device_messages_round_trip` | the new messages and snapshot field round-trip | stub serialises `device` as empty |
| AC2 | `crates/core/src/player/tests.rs` :: `ac2_set_device` (table: phase × name) | effects and snapshot `device` per row | stub ignores `SetDevice` (no effect, device unchanged) |
| AC3 | `crates/core/src/player/tests.rs` :: `ac3_set_device_failure`, `ac3_output_changed` | stopped, queue and device kept, message, retry plays; output and verdict updated | stub keeps the old output line |
| AC4 | `crates/audio/tests/engine.rs` :: `ac4_set_device_while_paused`, `ac4_output_changed`, `ac4_set_same_device` | no frame before `Resume`, continuity byte for byte; event emitted once per reopen; no reopen for the same device | stub engine starts writing on the new device while paused / emits no `OutputChanged` |
| AC5 | `crates/app/src/play.rs` :: `ac5_device_precedence` (table) | the chosen device per source combination | stub ignores the remembered device |
| AC6 | `crates/core/src/player/tests/persistence.rs` :: `ac6_device_saved_only_when_chosen`, `ac6_restore_device`, `crates/app/src/persist.rs` :: `ac6_v1_file_without_device` | saved field; restore; old file | stub never saves `device` |
| AC7 | `crates/app/src/ipc/server/tests.rs` :: `ac7_devices_reply` (fixture dir changed between requests; unreadable dir) | fresh list per request; error; only the asker gets it | stub answers `default` only |
| AC8 | `crates/core/src/ui/browse/tests.rs` :: `ac8_devices_popup` (table: key → effects, state) + `crates/core/src/ui/keymap/tests.rs` :: `ac8_switch_device_supported` + `crates/core/src/ui/help/tests.rs` :: `ac8_devices_popup_help` | popup state, cursor, `SetDevice`, close; keymap; help section | stub `D` does nothing; `SwitchDevice` still skipped |
| AC9 | `crates/app/src/ui.rs` :: `ac9_devices_popup` (snapshots), `ac17_no_panic_any_size` (new rows) | layout; no panic | stub draws no popup |
| AC10 | `crates/app/src/oneshot.rs` :: `ac10_parse_device`, `ac10_status_device_line` + `crates/app/tests/daemon.rs` :: `ac10_playback_device` | parse and exit codes; status line; listing and switching against a daemon | stub prints nothing and sends nothing |
| AC11 | `crates/app/src/player_runtime.rs` :: `ac11_set_device_reaches_engine` (fake engine, two subscribers) | engine call; both snapshots; output update | stub runtime drops `EngineSetDevice` |
| AC12 | `crates/app/tests/docs.rs` :: `ac12_device_selection_documented` + zh-TW copies, README and `CLAUDE.md` reviewed at acceptance; `crates/app/tests/examples.rs` (example keymap is the default keymap, existing test) | docs mention `D`, `SwitchDevice`, `playback device`; example keymap matches | docs lack the terms; example lacks `D` |

Checked by hand at acceptance on the user's machine (results in the PR description): playing on `default`, `D` → the DAC (`hw:`) moves the track with no audible gap beyond the reopen and the output line says `bit-perfect`; `playback device hw:0,0` from another terminal moves it again; a DAC plugged in while the popup is closed shows up on the next `D`; a busy `hw:` reports busy and `space` after closing the other app plays; restarting the daemon keeps the chosen device; a `TIDAL_PLAYER_DEVICE` in the unit beats it.

## Crate placement

- `tidal-player-core`: `Command::SetDevice`, `ClientMessage::Devices`, `ServerMessage::DevicesReply`, `DeviceEntry`, `PlayerSnapshot.device`; `PlayerState` gains the selected device, `PlayerEffect::EngineSetDevice`, `EngineEvent::OutputChanged(TrackDetails)`; `SavedPlayback.device: Option<String>`; the Devices popup and `SwitchDevice` in `ui`. No I/O
- `tidal-player-audio`: `Event::OutputChanged(OutputInfo)`; `SetDevice` while paused opens paused; same-device no-op
- `tidal-player`: the runtime maps `EngineSetDevice` and `OutputChanged`; the server answers `Devices` through a device-lister seam (reads `/proc/asound` or `TIDAL_PLAYER_ASOUND_DIR`); start precedence; `playback device`; the popup's drawing
- `xtask layering`: no change (core still knows nothing of the audio crate: `DeviceEntry` is its own type)

## Facts vs. assumptions

Verified (2026-10-10, from code): the engine's `SetDevice` moves a playing track and replays the unheard frames (`crates/audio/src/engine.rs` `set_device`, 0003 AC21) but emits no event with the new `OutputInfo`; with nothing open it only remembers the name; it calls `sync_pause` after opening, so a paused engine should stay paused (to be proven by AC4); the player has no notion of the device (`PlayerState`, `PlayerSnapshot`); the device list is `parse_devices` over `/proc/asound/{cards,pcm}` with `TIDAL_PLAYER_ASOUND_DIR` for tests (`crates/audio/src/devices.rs`, `crates/app/src/main.rs` `devices`); `SwitchDevice` is in 0008's skipped list and `D` is unbound by default; `playback.json` is `SavedPlayback` version 1 (`crates/core/src/player/saved.rs`).

Assumptions (checked at acceptance): reading `/proc/asound` reflects a USB DAC plugged in after start without anything else (it is kernel state); switching between two `hw:` cards releases the first card's reservation before claiming the second (0003 AC13 covers release on device switch).

## Decisions (to be answered by the user)

1. **Scope of the CLI**: proposed `playback device [NAME]` (list from the running player, or switch), `devices` unchanged. Alternative: `devices` asks the running player when there is one
2. **Remembering**: proposed to remember a runtime choice in `playback.json`, above `app.toml` but below `--device`/the environment (0009 decision 2 promised this). Alternative: never remember (the choice lasts for the run); or write the choice into `app.toml`
3. **A failed switch**: proposed to stop with the message and keep the new device selected. Alternative: go back to the previous device and keep playing there, with the message
4. **Key**: proposed `D` / `SwitchDevice` (spotify-player's). Alternative: also an *Output device…* entry in an actions popup

## Out of scope

- Switching automatically when a device appears or disappears (hot-plug following, udev/PipeWire watching): the list is read when asked
- PipeWire/PulseAudio sinks by name (`default` stays the way to reach the system mixer)
- Per-device settings (quality, release delay, buffer size), and the `hw:` buffer issue ([#6](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/6))
- Windows devices ([#21](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/21))
- A device property over MPRIS
