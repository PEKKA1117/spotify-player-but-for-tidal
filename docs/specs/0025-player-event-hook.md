# 0025 — Player event hook

- **Status**: approved (2026-11-05)
- **Owner**: tech-lead (primary session)
- **Issue**: [#56](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/56)
- **Depends on**: 0004 (implemented: the player's states and events), 0005 (implemented: the daemon), 0008 (implemented: `app.toml`), 0014 (implemented: the output device)
- **User docs**: [`docs/config.md`](../config.md) (a new "Running a command on player events" section) and its zh-TW copy, `examples/app.toml`, `README.md` and its zh-TW copy (AC15)

## Context

Status bars (waybar, i3blocks), scrobbling scripts and logs want to know what the player does. MPRIS (0010) covers part of this, but a script has to stay connected to the bus and has no way to tell a track that played to its end from a skipped one. spotify-player has `player_event_hook_command`: a command it runs on each player event. This spec adds the same setting with the same arguments, and a few events and environment variables on top.

tidalt had none of this.

## Behaviour

### The setting

```toml
player_event_hook_command = { command = "/home/me/bin/on-player-event", args = ["--quiet"] }
```

`command` is a path or a name found on `PATH`, run directly, without a shell: `~` is not expanded. `args` is optional and comes before the event's arguments. The setting exists only in `app.toml`: a command with arguments has no sensible environment form. Without it nothing runs, and no thread is started for it.

The player runs the hook, whichever kind of player it is: standalone, daemon or `play`. A TUI attached to a daemon runs nothing, so one event runs the command once however many clients are attached.

### Events

The event's arguments follow `args`, so with the example above a track change runs `on-player-event --quiet Changed 12345`.

| Event | Arguments | When |
|---|---|---|
| `Changed` | `TRACK_ID` | A track starts: its first frame was written, or playback moved into it gaplessly. This includes a repeat of the same track, playing a track again after a stop and starting a restored track at its saved position |
| `Playing` | `TRACK_ID POSITION_MS` | Playback starts or resumes: right after `Changed`, or on leaving a pause |
| `Paused` | `TRACK_ID POSITION_MS` | A playing track is paused |
| `Stopped` | `TRACK_ID POSITION_MS` | The player stops with a track started since the last `Stopped`: `Stop`, the queue ran out or was cleared, a skip that found nothing to play, or the player is shutting down. `TRACK_ID` is the track that last started |
| `EndOfTrack` | `TRACK_ID` | The track played to its end, gaplessly or not. A skip or a stop is not an end |
| `Seeked` | `TRACK_ID POSITION_MS` | The position of a playing or paused track was changed by hand |
| `VolumeChanged` | `PERCENT` | The volume heard changed: `0` to `100`, and `0` while muted |
| `DeviceChanged` | `NAME` | The selected output device changed (0014), by a client or by the fallback |

`POSITION_MS` is the position after the event, so `Playing` after `Changed` is not always `0`. `Buffering` is not a state change here: a stall and its recovery run nothing. Nothing runs while a track only loads, so skipping quickly through a queue runs `Changed` only for the track that really starts.

One input can cause several events, which run in this order: `EndOfTrack` of the old track, `Changed`, `Playing`, `Paused`, `Stopped`, `Seeked`, `VolumeChanged`, `DeviceChanged`. A track that plays to its end and is followed by another gives `EndOfTrack OLD`, then `Changed NEW` and `Playing NEW 0`; the last one of the queue gives `EndOfTrack LAST`, then `Stopped LAST POSITION_MS`.

Starting the player is not an event: a restored session (0009) runs nothing until something plays.

### The environment

The command inherits the player's environment. Events about a track (all but `VolumeChanged` and `DeviceChanged`) add:

| Variable | Value |
|---|---|
| `TIDAL_PLAYER_TRACK_TITLE` | The title, with its version in parentheses when it has one (`Song (Live)`) |
| `TIDAL_PLAYER_TRACK_ARTISTS` | The artists, joined with `, ` |
| `TIDAL_PLAYER_TRACK_ALBUM` | The album's title; not set when the track has no album |
| `TIDAL_PLAYER_TRACK_DURATION_MS` | The duration; not set when the metadata has none |

So a script needs no Tidal session to show or scrobble what plays.

### How it runs

The commands of one player run one at a time, in the order of their events, on a thread of their own: playback never waits for one.

- **Waiting events**: at most 64. When it is full the oldest is dropped. A `Seeked` or `VolumeChanged` that is last in line is replaced by a newer event of its kind, so holding a key costs one run, not hundreds, and the order of events never changes
- **Time limit**: a command still running after 30 s is killed, and the next one runs
- **Input and output**: stdin and stdout are `/dev/null`. stderr is the player's stderr under the daemon and `play`, which is the journal under systemd, and `/dev/null` in standalone, where it would draw over the TUI

### Failures

A command that cannot start, exits with a non-zero status or is killed for its time limit does not affect playback. The player says so once, as the player's message that every client shows, and on stderr under the daemon and `play`:

```
Player event hook failed: on-player-event: No such file or directory (os error 2)
Player event hook failed: on-player-event exited with status 3
Player event hook failed: on-player-event was killed after 30 s
Player event hook is too slow: events were dropped
```

Further failures say nothing until a command succeeds, so a broken hook does not overwrite the message row at every track.

### Shutting down

When the player shuts down with a track playing or paused, it sends `Stopped`, so a status bar does not keep showing the track. The hook gets up to 2 s to finish what is waiting; what is left is dropped, and a command still running is left to finish.

## Acceptance criteria

1. **AC1 Setting**: `player_event_hook_command` in `app.toml` takes a table with `command` (a non-empty string) and optional `args` (an array of strings). Anything else (a string, an empty `command`, a missing `command`, an unknown field, `args` that is not an array of strings) is an `app.toml` error with exit 2, in 0008's wording. Without it no hook runs and no thread is started
2. **AC2 Track events**: from the state before and after an input, `Changed` and `Playing` run for a started track (a repeat and a play after a stop included), `Playing` for leaving a pause, `Paused` for pausing a playing track, and `EndOfTrack` only for a track that played to its end, in the order of "Events"
3. **AC3 Stopped**: `Stopped` runs once when the player stops after a track started, by `Stop`, an empty queue, a cleared queue or a skip that found nothing to play, and names the track that last started even when it left the queue; a player that stops without a track having started runs nothing
4. **AC4 Seeked**: `Seeked` runs with the new position for a seek on a playing or paused track and for `Previous` restarting it, and not for a seek while stopped or loading, a seek to the same position or a `Previous` that moves to another track
5. **AC5 Volume**: `VolumeChanged` runs when the volume heard changes, `0` while muted, and not for a change that leaves it the same
6. **AC6 Device**: `DeviceChanged` runs when `device` changes, and moving a playing track to another device runs no `Changed`, `Playing` or `Seeked`
7. **AC7 Silence**: loading, buffering, recovering from buffering, releasing the output while paused, pausing a track still loading, queue edits and a restored session run nothing; quickly skipping past tracks that never start runs nothing for them. A track that starts under such a pause runs `Changed` alone, and `Playing` when it resumes
8. **AC8 Arguments and environment**: the command gets `args`, then the event's name and arguments; events about a track set the four variables as "The environment" says, an absent album or duration leaving its variable unset, and `VolumeChanged` and `DeviceChanged` set none of them
9. **AC9 Queue**: events run one at a time in order; the 65th waiting event drops the oldest; a `Seeked` or `VolumeChanged` last in line is replaced by a newer one of its kind, and one with another event behind it is not
10. **AC10 Real sequences**: driven through `PlayerRuntime` with a fake engine, a gapless transition, a non-gapless end, the end of the queue and a skip give the sequences "Events" says, and a skip gives no `EndOfTrack`
11. **AC11 Failures**: a command that cannot start, a non-zero exit and a timeout each give their message once as the player's message; a later failure says nothing until a success; a success re-arms it; playback is unchanged throughout
12. **AC12 Never blocks**: `PlayerRuntime::handle` returns while a hook command is still running
13. **AC13 Every player**: standalone, the daemon and `play` run the hook, and an attached TUI does not
14. **AC14 Shutting down**: shutting down with a track playing or paused sends `Stopped`, the hook gets up to 2 s for what is waiting, and the player exits within that time with a command stuck
15. **AC15 Docs**: `docs/config.md`, `README.md` and their zh-TW copies describe the setting, the events, the variables and the failures; `examples/app.toml` shows the setting, commented out
16. **AC16 A real process**: a real script receives the arguments and the environment, reads nothing from stdin, and a script that runs past the limit is killed

## Edge cases & errors

| Case | Behaviour |
|---|---|
| `command` missing or not executable | The first event gives the failure message; playback goes on |
| A command that hangs | Killed at 30 s; events wait meanwhile, and the oldest are dropped past 64 |
| A script that starts a background process | Its own business: the player only waits for the command itself |
| The same track twice in a row in the queue | `EndOfTrack`, then `Changed` for the second: they are two entries |
| A track that fails to start | No `Changed`: it never played. The skip it causes runs nothing either |
| Removing the playing track from the queue | What the player does (0004) decides: stopping gives `Stopped` with the removed track |
| A skip whose track fails, with nothing left to play | `Stopped` with the track that last started: a status bar must not keep showing it |
| The output is released while paused, then resumes | `Paused` and `Playing` only: `released` is not an event |
| A seek past the end | The player's behaviour (0004): a seek that ends the track gives `EndOfTrack` if the engine reports an end |
| Mute, then unmute | `VolumeChanged 0`, then `VolumeChanged PERCENT` |
| `Previous` restarting the track | A seek to the start: `Seeked TRACK_ID 0` |
| `app.toml` changed while running | Read once at start, like every setting |

## Test plan

| AC | Test | Asserts | Expected red failure |
|---|---|---|---|
| AC1 | `crates/app/src/config.rs` :: `ac1_player_event_hook_command` (table) | parsed values and each error's wording | unknown key `player_event_hook_command` |
| AC2 | `crates/core/src/hook/tests.rs` :: `ac2_track_events` (table) | before/after/started/ended → events, in order | no events derived |
| AC3 | `crates/core/src/hook/tests.rs` :: `ac3_stopped` (table) | each way of stopping; a track gone from the queue | no events derived |
| AC4 | `crates/core/src/hook/tests.rs` :: `ac4_seeked` (table) | playing, paused, stopped, loading, same position, `Previous` both ways | no events derived |
| AC5 | `crates/core/src/hook/tests.rs` :: `ac5_volume` (table) | volume, mute, no change | no events derived |
| AC6 | `crates/core/src/hook/tests.rs` :: `ac6_device` + `crates/app/src/player_runtime.rs` :: `ac6_device_move_runs_only_device_changed` | the event; a move of a playing track through the runtime | no events derived; nothing recorded |
| AC7 | `crates/core/src/hook/tests.rs` :: `ac7_silence` (table) | no events for each case | a stub deriving events for every change fails it |
| AC8 | `crates/core/src/hook/tests.rs` :: `ac8_arguments_and_environment` (table) | argv and variables per event; absent album and duration | empty argv |
| AC9 | `crates/core/src/hook/tests.rs` :: `ac9_queue` (table) + `crates/app/src/hook.rs` :: `ac9_one_at_a_time` | order, the limit, replacement of the last; never two commands at once | queue keeps nothing; nothing runs |
| AC10 | `crates/app/src/player_runtime.rs` :: `ac10_real_sequences` (table) | recorded events for the four sequences | nothing recorded |
| AC11 | `crates/app/src/hook.rs` :: `ac11_failures` (table, fake runner) | messages and their re-arming | no message reported |
| AC12 | `crates/app/src/hook.rs` :: `ac12_never_blocks` | a runner held on a barrier; `send` returns at once | the send waits |
| AC13 | `crates/app/src/config.rs` :: `ac13_hook_reaches_the_settings` + hands-on check | `app.toml`'s command is in the settings all three players start from; the check above runs it with a TUI attached, since `main` starts the players and has no seam | settings carry no hook |
| AC14 | `crates/app/src/hook.rs` :: `ac14_shutting_down` + `crates/app/src/player_runtime.rs` :: `ac14_shutdown_sends_stopped` | the 2 s limit with a stuck runner; `Stopped` for playing and paused, none when stopped | no drain; nothing recorded |
| AC15 | `crates/app/tests/examples.rs` (the example parses) + reviewed at acceptance | docs read by hand: prose has no automated check | the commented example is not found |
| AC16 | `crates/app/tests/hook.rs` :: `ac16_real_process` | a `/bin/sh` script writes what it got to a file; a sleeping one is killed at a short limit | no process runs |

Checked by hand at acceptance, against the live API: the release build as a daemon on its own runtime and state directories, with a script appending to a file. Play, pause, seek, skip, change the volume and the device, let a track end gaplessly, stop the daemon; the file holds the expected lines, once each, with a TUI attached. Then a command that does not exist: the TUI's message row shows the failure once. The message row is the only thing the TUI gains, so the PR's hands-on section is this one scenario.

## Crate placement

- `tidal-player-core`, new `hook` module: `HookEvent`, `HookTracker` (events from the snapshots around an input, remembering the track that last started), argv and environment of an event, the waiting queue. All pure
- `tidal-player` (`crates/app`), new `hook` module: the worker thread, the failure reporting and the process behind a `Runner` seam
- `crates/app/src/player_runtime.rs`: `handle` takes the snapshot before an input when a hook is set too, and hands the events to the hook. `Command`, `Event` and the socket protocol do not change
- A failure reaches the player as a new `RuntimeInput::HookFailed(String)`, turned into `PlayerInput::Notice` as a failed save is

## Facts vs. assumptions

Verified in the code at `463e3e5`:

- `Handled.ended` is set for a track that played to its end and also for a gapless `Transitioned`, never for a skip; `Handled.started` is set whenever a track starts, the one a skip led to included
- `PlayerRuntime::handle` already computes whether an input is a seek, and takes the snapshot before an input when persistence is on
- `PlayerSnapshot.position` follows the engine's `Position` events, and `device` is the selected device
- No `tracing` subscriber is installed anywhere in the workspace, so `tracing::warn!` writes nothing: the issue's "logged" cannot mean `tracing`
- At `Shutdown` the runtime skips `track_change`, so the snapshot does not show a stop: `Stopped` needs its own case
- `Command::Previous` is not in `handle`'s seek check, though it can restart the track (a `seek_to(0)`): the hook's check adds it, and `Seeked` needs the same `current` and a changed position, so a `Previous` that moves to another track or a seek to the same position runs no `Seeked`

Assumptions, to confirm while implementing; a wrong one comes back to this spec:

- A device move goes through `OutputChanged` and gives no `Started` (AC6 tests it)

## Decisions

1. **`Changed` follows a started track, not `current`.** `current` moves as soon as a track is asked for, so fast skipping would run the hook for tracks nobody heard
2. **The runtime, not a hub peer.** MPRIS sees only snapshots, which cannot tell a natural end from a skip. The runtime knows, and using it leaves the protocol alone
3. **One at a time.** Spawning every event is simpler, but events could run out of order and a stuck script would pile up processes. A queue with a limit keeps the order; the time limit stops one stuck command from silencing all the later ones
4. **Failures use the player's message.** There is no log to write to, and a hook that fails silently is the worst case for someone setting it up
5. **Standalone drops stderr.** Writing to it would draw over the TUI

## Out of scope

- A log file or `tracing` subscriber (the logs page, #64, has to settle that)
- A shell form of the command, `~` expansion
- Events for shuffle, repeat, autoplay, the queue or login
- Setting the hook from a flag or the environment (`-o`, #65, would cover it)
- Making the 30 s limit or the 64 events configurable
- Killing what a command left in the background
