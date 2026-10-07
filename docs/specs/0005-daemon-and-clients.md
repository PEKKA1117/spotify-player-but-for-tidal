# 0005 — Daemon & client mode

- **Status**: approved (2026-10-07)
- **Owner**: tech-lead (primary session)
- **Depends on**: 0001 (implemented: run modes, the `Command`/`Event` boundary), 0002 (implemented: the daemon's passphrase sources, `LoginRequired`/`LoginRestored`), 0003 (implemented: the engine and the reservation), 0004 (implemented: the player runtime, the TUI)
- **User docs**: a new [`docs/daemon.md`](../daemon.md); [`docs/playback.md`](../playback.md), [`docs/tui.md`](../tui.md) and [`docs/login.md`](../login.md) lose their "until 0005" notes (AC24)

## Context

0004 runs the player in the TUI's process. This spec lets it run on its own (`tidal-player daemon`, a systemd user service) and lets any number of clients reach it: TUI clients, one-shot commands (`tidal-player playback next`), and a second `tidal-player --add-to-queue <link>`. It also makes the engine release the audio device while paused, so other applications can use the DAC, as tidalt's daemon did.

### What tidalt did

Read from `cmd/tidalt/{main,daemon}.go`, `internal/mpris/server.go`, `internal/ui/{model,queue}.go`, `docs/client-server.md`, and its closed issues #3–#8 and #19–#21:

- The first `tidalt` claimed the D-Bus name `org.mpris.MediaPlayer2.tidalt` and became the **parent**. Every later invocation found the name taken and became a client of it, whether the parent was a daemon or a TUI. `tidalt daemon` was the same program, minus the UI
- Clients talked to the parent through a private D-Bus interface, `io.tidalt.App`, with one method per feature, added as bugs were found (`OpenURL`, `PlayTrackID`, `PlayPlaylist`, `Enqueue`, `Dequeue`, `SetShuffle`, `SetDevice`, …)
- The player's logic (queue, shuffle, auto-advance) lived in the BubbleTea UI model. The parent's model ran it; a client ran a second copy of the same model against its own queue copy
- A client learned the parent's state by **polling** `GetState` every second. `GetState` returned the current track and the whole playlist as JSON strings, plus status, position, duration, volume, device and shuffle mode
- The parent turned each D-Bus call into an event on a channel with a non-blocking send (`select { case ch <- ev: default: }`): when the channel was full, the call was **dropped** and the caller still got success
- The daemon opened the device only when playing. Pausing closed the PCM and released the `ReserveDevice1` name; resuming reacquired both
- `tidalt setup --daemon` wrote a systemd user unit (`Restart=on-failure`, `RestartSec=5s`) and enabled it

### What went wrong there, and the criterion that covers it here

1. **Edits made in a client were undone by the next poll** (#6; `0092235`, `3519907`): "Add to queue", "Play next" and "Remove" changed the client's own copy. The parent never heard about them, and within a second the poll restored its list. Added tracks never played; removed ones played anyway → a client has no queue of its own: every edit is a `Command` to the player, and the client shows only what the player sends back (0004 AC22; here AC5, AC12)
2. **Playing a playlist from a client loaded one track** (#5; `a932e57`, `ce05a23`): the client sent `PlayTrackID` unless a flag that only one code path set was true, so the parent's queue shrank to the one track → one way to load: `LoadQueue`/`Open` carry the whole list (AC7)
3. **Shuffle was applied twice, then lost** (#19, #20, #21; `fb4721c`): the client re-shuffled the parent's already shuffled list on every poll; `s` in a client was never forwarded; a playlist sent from a shuffled client arrived in display order and the parent stored it as the original order with shuffle off → shuffle exists only in the player (0004 AC3); a client sends items or tracks in their original order, never a play order (AC7)
4. **Client actions ran locally and did nothing** (#4 follow-up `2c94bbd`: choosing a device in a client called the client's own player, which had no device open) → a client process has no player, no engine and no session (AC14)
5. **Calls were dropped silently** (`select … default:` on every `io.tidalt.App` method): a busy parent lost commands and the client reported success → every request gets exactly one reply, after it was applied; nothing between a client and the player is dropped while the client is connected (AC5, AC6)
6. **Polling** made every client up to a second stale and raced with edits (#6) → the player pushes every change; a client is never behind by more than the time to deliver one message (AC4)
7. **Duplicates were ambiguous**: `Dequeue` removed "the first entry with this track ID", because positions differed between two independently shuffled copies → 0004's entry IDs address entries; the player's order is the only one (0004 AC10)
8. **`tidalt daemon` restarted forever while a TUI was open** (#8; `69f6744`): the daemon exited 1 when the name was taken, the unit restarted it every 5 s without limit, and a journal reached restart counter 109 → "another player is running" exits **3**, and the unit generated here sets `RestartPreventExitStatus=1 2 3` and a start limit (AC11, AC18)
9. **Two processes fought over one database** (`1d3faed`, `a8b5e7e`; the bolt lock timeouts in #8): the parent and each client opened the same store → a client opens nothing on disk but the socket (AC14). Persistence (0009) belongs to the player process alone
10. **Resuming after a release crashed or played the wrong track** (`f93fa38`, `2afa977`): a failed reacquire on resume freed the PCM twice (use-after-free in libasound); a stuck reacquire outlived `stop()`'s 3 s window and a second playback loop started, so two loops fought over the device; a `RequestRelease` to an owner that never answered blocked forever; a reservation error was taken for the end of the track and auto-advanced in a loop → release and reacquire live in the engine thread, which owns the device alone, so there is never a second output; a failed reacquire leaves the player **paused** with a message and never skips; every D-Bus call has a deadline (AC19–AC21, and 0003 AC13)
11. **The parent was whichever process came first**: quitting the TUI that happened to be the parent stopped the music under every client, and a client had no way to tell → clients are told (`ShuttingDown`, or the lost connection), show it, and reconnect when a player appears again (AC12, AC13)

Most of these (1–7, 9, 11) are one bug in different forms: **the client and the player went out of sync**, because the client held state of its own (a queue, a shuffle order, a device, a database), changed it locally, and learned the player's version late (polling) or never (dropped calls). This spec removes the cause rather than each symptom; the rule under "Sync" is what every criterion below enforces.

## Behaviour

### Sync

1. **The player's state is the only state.** A client holds nothing about playback but the last `Welcome`/`Event`s it received: no queue of its own, no order, no mode, no device, no store. Its own state is only what no other process needs: the cursor, the open prompt, the page
2. **A client changes nothing locally.** Every key that affects playback sends a `Command` and changes nothing on screen; the screen changes when the player's event arrives (one message away, no poll)
3. **Every change is pushed, in order, to every client, exactly once**, starting from a `Welcome` that is consistent with it (no gap and no overlap between the snapshot and the events after it)
4. **Nothing is dropped silently.** A command is answered after it was applied; a client that cannot keep up is disconnected (and then resyncs from a fresh `Welcome`), never skipped over
5. **Commands carry intent, not positions.** Entries are addressed by entry ID; lists are sent in their original order; modes are toggled in the player (0004)

Consequence, tested as such (AC4): whatever the interleaving of commands from any number of clients, once the player is idle every connected client's view equals the player's snapshot.

### Roles

A **player** process runs `tidal_player_core::player` and the engine. A **client** sends commands to a player and shows its events. There is **at most one player per user** at a time.

| Invocation | No player running | A player running |
|---|---|---|
| `tidal-player [ITEM]...` | Becomes the player (standalone, as in 0004) and shows the TUI | **Attaches** a TUI client to it; with items, first sends `Open` (below) |
| `tidal-player daemon` | Becomes the player, headless | Exits **3**: `Another player is running (pid N)` |
| `tidal-player play ITEM...` | Becomes the player, headless, in the foreground (0004) | Exits **3**, same message plus `: use "tidal-player playback load"` |
| `tidal-player playback …` | Exits 1: `No player is running: start "tidal-player" or "tidal-player daemon"` | Sends one request, prints its answer, exits |
| `tidal-player daemon stop` | Exits 1, same message | Asks the player to shut down; waits until it is gone (at most 5 s); exit 0 |

Every player (standalone TUI, `daemon`, `play`) serves the socket, so one-shot commands and TUI clients work against any of them. A standalone TUI's own screen is a client too, joined to its player in-process (0001), with the same messages.

Quitting (`q`) a **client** TUI detaches it; the player keeps playing. Quitting the TUI of a **standalone** player stops that player, as in 0004; its other clients see `The player shut down` (below).

### Transport

A Unix domain socket, `player.sock`, in the **runtime directory**:

1. `$TIDAL_PLAYER_RUNTIME_DIR`, if set and not empty (tests, unusual setups)
2. `$XDG_RUNTIME_DIR/tidal-player`
3. `/tmp/tidal-player-<uid>`

The directory is created with mode `0700`. An existing one that is not a directory, not owned by this user, or accessible to group or others is refused (`Runtime directory <path> is not private: …`, exit 1), so no other user can reach the player. The socket is created inside it.

**One player per user** is enforced by `player.lock` in the same directory: a player takes an exclusive lock on it (`File::try_lock`) for its whole life and writes its pid into it. "A player is running" means the lock is held. A player that gets the lock removes a leftover `player.sock` (a crashed player) before it binds. On exit it removes the socket; the lock goes with the process, however it ends.

A client that finds the lock held connects to the socket, retrying for up to 2 s (the player may still be starting). If it still cannot connect: `A player is running (pid N) but not answering on <socket>` (exit 1).

### Messages

Newline-delimited JSON over the socket: one message per line, UTF-8, at most **16 MiB** per line (a `LoadQueue` of a few thousand tracks is under 1 MiB). The types live in `tidal_player_core::protocol`; the JSON encoding lives in the binary (core has no `serde_json`, 0001 AC3).

1. On accept, the player sends a **greeting**, `{"tidal_player":"<version>"}`. Its shape is frozen: every later version sends it unchanged, so an old client and a new player (or the other way round) can tell what they are talking to. A client whose version differs closes the connection: `The running player is tidal-player <theirs>, this is <ours>: restart it ("tidal-player daemon stop", or "systemctl --user restart tidal-player")` (exit 1 for one-shot commands; the TUI shows it and does not retry)
2. The client then sends `ClientMessage`s:
   - `Subscribe`: the player answers `Welcome { snapshot, login_required }` and from then on sends this client every `Event`
   - `Request { id, command }`: the player applies `command` and answers `Reply { id, result: Ok | Err(message) }`
3. The player sends `ServerMessage`s: `Welcome`, `Event(Event)`, `Reply`

Ordering:

- A client that subscribes gets a `Welcome` that reflects every input the player handled before it, then **every** later event, in order, exactly once. A client that applies the `Welcome` and then the events ends up with the player's state (no gap, no stale snapshot)
- A `Reply` is sent after the events its command caused. The events go to every subscriber (the sender included, if subscribed)
- Commands from all clients are applied one at a time, in the order the player receives them. 0004's toggles keep two clients with stale views from undoing each other

A client that sends a line that is not a valid message, or one over the size limit, is disconnected; the player and the other clients carry on. A client that stops reading never blocks the player: each client has an outbox of **1024** messages, and a client whose outbox is full is disconnected (it can reconnect and gets a fresh `Welcome`). A TUI drains its socket at least every 100 ms (0004's frame), so only a stopped or suspended client (Ctrl-Z) gets there, after about four minutes of position events.

### Opening items in the player

`Command::Open { items, at }`, new: `items` are 0004 items (track IDs, track/album/playlist links), already checked by `parse_item` on the client side (a bad item exits 2 or shows the message there, as in 0004, without contacting the player). The **player** expands them (0004 "Filling the queue") and then:

- `at: None`: `LoadQueue { tracks, start: 0 }` (replace the queue, play the first)
- `at: Some(End | Next)`: `AddToQueue { tracks, at }`, then `PlayEntry` of the first added entry when the player is `Stopped` with nothing current, or the queue was empty (0004 AC28's rule, moved from the TUI's runtime into the player's)

The `Reply` comes once the tracks are queued, or with the expansion's error message (`Album 1 was not found`, …); an error changes nothing. `Open`s are applied in the order they were received, even when a later one finishes expanding first. The TUI's open prompt (`o`/`O`) and its startup items go through `Open` as well, so a client needs no API access of its own (decision 3).

### The TUI as a client

- On `Welcome`, the client state takes the snapshot (0004 AC22) and the login status, and the playback window shows the player
- **Disconnected** (the connection closed or failed, or `Event::ShuttingDown`): the last snapshot stays on screen; the message row says `Disconnected from the player: reconnecting…`, or `The player shut down: waiting for it to come back…` after `ShuttingDown`. Keys that would send a command do nothing; cursor keys and `q` work. The client tries to reconnect every second; on success it subscribes again and the `Welcome` replaces everything
- A version mismatch is shown (the message above) and not retried
- A client TUI's `q` detaches: no `Shutdown` is sent

### One-shot commands

`tidal-player playback <command>`: connect, send one `Request` (or `Subscribe`, for `status`), wait for the answer (at most 5 s, else `The player did not answer` and exit 1), print, exit.

| Command | Sends |
|---|---|
| `play-pause` | `TogglePause` |
| `next`, `previous` | `Next`, `Previous` |
| `seek +S`, `seek -S`, `seek S` | `SeekBy(±S s)`, `SeekTo(S s)`; `S` in seconds, fractions allowed |
| `volume +N`, `volume -N`, `volume N` | `ChangeVolume(±N)`, `SetVolume(N)`; `N` 0–100 |
| `mute`, `shuffle`, `repeat`, `autoplay` | `ToggleMute`, `ToggleShuffle`, `CycleRepeat`, `ToggleAutoplay` |
| `load ITEM...` | `Open { at: None }` |
| `add [--next] ITEM...` | `Open { at: Some(End) }`, or `Next` with `--next` |
| `status [--json]` | `Subscribe`; prints the `Welcome` and disconnects |

Output: nothing on success (exit 0), except `status`; the reply's message on stderr and exit 1 on `Err`; exit 2 for bad arguments or a bad item (nothing sent). `status` prints three lines:

```
▶ Hell Above · Pierce The Veil · Collide With The Sky
1:23 / 3:32 · shuffle · repeat: queue · 80%
Queue: 2 of 12
```

(the state symbols and indicators of 0004's TUI; `Nothing playing` alone for an empty queue; a fourth line with the message, if any, or `Session expired: run "tidal-player login"` when the login is required). `--json` prints the `Welcome` as one JSON line, for scripts.

`tidal-player daemon stop` sends `Shutdown` and waits until the lock is free.

### The daemon

`tidal-player daemon`:

- Startup, in order: take the lock (else exit 3), load the session (0002: no session → exit 1; encrypted without a passphrase → exit 1), read the settings from the environment (0003/0004; invalid → exit 2), bind the socket, tell systemd it is ready (`READY=1` on `$NOTIFY_SOCKET`, when set), then wait for clients. Messages go to stderr (the journal under systemd)
- It never opens the audio device before something plays (0003's engine opens it on `Play`)
- `SIGTERM` or `SIGINT`, or a client's `Shutdown`: `Event::ShuttingDown` to every subscriber, the engine stops (device closed, reservation released), the socket is removed, exit 0
- Login: the daemon's `Authenticator` status is broadcast as `Event::LoginRequired` / `LoginRestored` (0002 AC14); `Welcome` carries it. After `tidal-player login` in any terminal, the daemon picks the new session up (0002 AC9)
- The queue starts empty; remembering it across restarts is 0009

**systemd user service.** `tidal-player daemon unit` prints a unit to stdout, with `ExecStart` set to the running binary's absolute path. The user installs it (`docs/daemon.md`):

```
tidal-player daemon unit > ~/.config/systemd/user/tidal-player.service
systemctl --user daemon-reload
systemctl --user enable --now tidal-player
```

```ini
[Unit]
Description=tidal-player daemon
Documentation=https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/main/docs/daemon.md
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=notify
ExecStart=<absolute path> daemon
Restart=on-failure
RestartSec=5
# 1: not logged in or the session store is unusable; 2: bad settings;
# 3: another player is running. Restarting fixes none of them.
RestartPreventExitStatus=1 2 3
# Settings (docs/playback.md#settings), e.g.:
#Environment=TIDAL_PLAYER_DEVICE=hw:1,0
# Passphrase for an encrypted session file (docs/login.md):
#LoadCredential=tidal-player-passphrase:%h/.config/tidal-player/passphrase

[Install]
WantedBy=default.target
```

The program never writes the unit or runs `systemctl` itself (decision 5).

### Releasing the device while paused

The engine releases the output while paused, so other applications can use the card:

- After the **release delay** in `Paused` (a setting, `TIDAL_PLAYER_RELEASE_PAUSED`: `0`–`3600` seconds or `never`; default **10**, decision 4), the engine closes the PCM and, for exclusive output, releases the `ReserveDevice1` name. It keeps the track: source, decoder and position. Event `Released`
- **Another application asks** (`RequestRelease` on the name we hold): while `Paused` (released or not), the engine closes the PCM, answers `true`, then releases the name. While loading, playing or buffering it answers `false`, as 0003 does today. It answers within 400 ms (0003's callers wait 500 ms). This holds even with the delay set to `never`
- **Resume** after a release: reserve (0003's table), reopen and negotiate as on a track start, then continue from the first frame that was not yet heard: the frames still in the device buffer at release time are played again from the decoder, so nothing is lost or repeated. Event `Resumed`
- **Resume fails** (busy, gone): the engine stays paused with nothing open; event `ResumeFailed(error)`. The player stays `Paused` at the same position with 0003's message (`Output hw:1,0 is busy (used by …)`); it never skips (this is not a track failure). Play/pause tries again
- Seek while released moves the position and takes effect on resume. `Play`, `Next`, `Stop` and `SetDevice` while released behave as when not released (a `Stop` closes nothing twice and releases nothing twice)
- Shared output is closed and reopened the same way (no reservation)
- The snapshot's now-playing details gain `released: bool`; the TUI's third row ends with ` · device released` while it is true; `playback status` shows the same

Settings table addition (0004 "Settings"; still from the environment until 0008):

| Setting | Environment | Accepted | Default |
|---|---|---|---|
| Release the device after pausing for (s) | `TIDAL_PLAYER_RELEASE_PAUSED` | integer 0–3600, or `never` | `10` |

## Acceptance criteria

Protocol (`tidal_player_core::protocol`, pure):

- **AC1** — `ClientMessage` (`Subscribe`, `Request { id, command }`), `ServerMessage` (`Welcome { snapshot, login_required }`, `Event(Event)`, `Reply { id, result }`), `Command::Open { items, at: Option<InsertAt> }`, the engine-facing `released` field of `NowPlaying` and every other new variant survive a JSON round trip (0001 AC11's test, extended)
- **AC2** — Framing (`tidal-player::ipc`, pure over byte buffers): `encode` writes one JSON line; the decoder assembles a message split across reads, decodes several from one read, refuses a line over 16 MiB without buffering it whole, and refuses invalid JSON and unknown variants, naming the problem (table)
- **AC3** — Greeting: the player's first line is exactly `{"tidal_player":"<CARGO_PKG_VERSION>"}`; `check_greeting` accepts the same version, gives the mismatch message (with both versions) for another, and `Not a tidal-player socket: <path>` for any other first line (table)

Player server (`tidal-player`, with 0004's fake engine and jobs, clients over socket pairs or in-memory connections):

- **AC4** — Convergence ("Sync"): a client that subscribes while commands are being handled gets a `Welcome` and then every later event once, in order; applying them gives exactly the runtime's final snapshot (table: subscribe before, during and after a run of 50 commands; a second subscriber's sequence equals the first's from its `Welcome` on). And, over 200 seeded random runs: 3 clients send random interleavings of every queue, mode and volume command (edits addressed to entries they saw, possibly already removed), one of them disconnects and resubscribes mid-run; once the player is idle, every client's `ui::State` playback copy equals the player's snapshot (queue order, current entry, modes, volume). The seed of a failing run is printed
- **AC5** — Requests: each `Request` gets exactly one `Reply` with its `id`, after the events it caused; commands from two clients are applied in arrival order; both subscribers see the same events. No command is dropped: 1000 requests from 4 clients give 1000 replies and the player saw 1000 commands
- **AC6** — Isolation: a subscriber that never reads is disconnected once 1024 messages are waiting, the others keep receiving, and the player keeps handling input (it answers another client's request within 1 s while the stalled one is pending); a client that disconnects mid-line, sends invalid JSON or an oversized line is dropped without affecting the player or the others
- **AC7** — `Open`: expansion runs in the player process (fake metadata); `at: None` loads and plays; `End`/`Next` add, and start the first added entry when stopped with nothing current or the queue was empty; an expansion error is the `Reply`'s `Err` and changes nothing; two `Open`s are applied in the order received when the second finishes expanding first. The queue is sent in item order, never a play order (tidalt #21)
- **AC8** — Login: an `Authenticator` status change is broadcast as `Event::LoginRequired`/`LoginRestored` to every subscriber, and `Welcome.login_required` is the current status (fake status source)

One player per user (`tidal-player`):

- **AC9** — `runtime_dir(env, uid)` (pure) picks the directory per "Transport" (table: each variable set, empty or unset); `prepare_runtime_dir` creates it `0700` and refuses a path that is a file, owned by another uid (fake metadata) or group/world-accessible (temp dirs)
- **AC10** — Lock: with the lock held by another process, `daemon` and `play` exit 3 with `Another player is running (pid N)` (and `play` with its hint); a stale `player.sock` with the lock free is removed and the new player listens; two daemons started at once leave exactly one running and the other exits 3 (integration, `crates/app/tests/daemon.rs`, temp `TIDAL_PLAYER_RUNTIME_DIR`, wiremock API, no audio device)
- **AC11** — Auto-attach: `tidal-player` with a player running attaches as a client instead of starting a second player (the lock is not taken, no engine is created); with items it sends `Open` first (`--add-to-queue` → `End`, `--play-next` → `Next`, plain → `None`); while the player is starting (lock held, socket not yet bound) the client retries for 2 s, then fails with `A player is running (pid N) but not answering`

Clients:

- **AC12** — TUI client model (`tidal_player_core::ui`, pure): `Welcome` replaces the snapshot and the login status; `Disconnected { shut_down }` keeps the last snapshot and sets the matching message; while disconnected no key emits `Send` (table over every command key), cursor keys and `q` still work; `q` in client mode emits `Quit` and never `Send(Shutdown)`; a version mismatch sets its message and stops reconnecting
- **AC13** — Client connection (fake connector): after a lost connection the client tries again every second, subscribes on success, and the next `Welcome` replaces the state; a client never shows a queue edit the player did not send (an `o` add appears only after the player's snapshot has it: tidalt #6)
- **AC14** — A client process opens no session store, keyring, passphrase source or audio device: `tidal-player playback status` and a TUI client attach succeed with `TIDAL_PLAYER_STATE_DIR` pointing at an empty directory and `TIDAL_PLAYER_NO_KEYRING=1` (they would otherwise say `Not logged in`); the client-mode startup function takes no store and no engine (integration + by construction)
- **AC15** — `playback` (pure parse + integration against a test player): every row of the "One-shot commands" table maps to its message (table, incl. `seek 90`, `seek +5`, `seek -2.5`, `volume 80`, `volume +5`, `volume -100`); `volume 101`, `seek x`, a bad item → exit 2 and nothing sent; no player → exit 1 with the message; `Err` reply → its message on stderr, exit 1; no reply within 5 s → exit 1; `status` prints the three lines (pure formatter, table: playing, paused + released, nothing playing, session expired) and `--json` one JSON line
- **AC16** — `daemon stop` sends `Shutdown`, waits until the lock is free and exits 0; no player → exit 1

Daemon (`tidal-player`):

- **AC17** — `daemon` with a session: takes the lock, binds the socket, sends `READY=1` to `$NOTIFY_SOCKET` when set (a test datagram socket) only after the socket accepts connections; on `SIGTERM` (and on a client's `Shutdown`) every subscriber gets `ShuttingDown`, the engine is stopped, the socket file is removed and it exits 0. 0002 AC12's exit codes still hold (no session, no passphrase → 1)
- **AC18** — `daemon unit` prints the unit under "systemd user service", with `ExecStart` the given executable path (pure function of the path, `insta` snapshot reviewed by eye, plus `contains` checks for `Type=notify`, `RestartPreventExitStatus=1 2 3`, `StartLimitBurst=5` and the absolute path)

Engine and player (`tidal-player-audio` with 0003's fakes; `tidal_player_core::player`):

- **AC19** — Release while paused: after the release delay (fake clock; `0` releases at once, `never` never) the engine closes the sink once and releases the reservation once, and emits `Released`; `Resume` reserves and opens again and the sink receives the frames from exactly the first unheard one (fake sink reporting a delay: no frame lost or repeated, 16- and 24-bit fixtures, compared byte for byte with an unpaused run); a seek while released resumes at the target; `Play`, `Stop`, `SetDevice` while released open/close nothing twice
- **AC20** — `RequestRelease`: answered `true` within 400 ms while paused (released or not, any delay setting, the PCM closed before the answer and the name released after it), `false` while loading, playing or buffering (table over engine states, fake reservation)
- **AC21** — Resume failure: a busy or missing device on resume gives `ResumeFailed(error)`, nothing stays open, and a later `Resume` tries again; in the player, `ResumeFailed` keeps `Paused` at the same position with 0003's message, emits no `Resolve` and no skip, and `TogglePause` emits `EngineResume` again (table over `Busy`, `NotFound`, `Lost`)
- **AC22** — `Released`/`Resumed` set and clear `NowPlaying.released` in the next snapshot; the TUI's third row ends with ` · device released` while set (rendering snapshot, 80×24)
- **AC23** — Settings: `TIDAL_PLAYER_RELEASE_PAUSED` in `resolve_player_config` (0004 AC25's table, extended: unset → 10 s, `0`, `3600`, `never`, `3601`, `-1`, `x`, empty = unset)

Docs:

- **AC24** — `docs/daemon.md` documents the roles table, auto-attach, the systemd setup (and why exit 1/2/3 are not restarted), `playback` and `daemon stop`, releasing the device while paused and its setting, and the troubleshooting messages (another player running, version mismatch, not answering, runtime directory not private); `docs/playback.md`, `docs/tui.md` and `docs/login.md` drop their "until 0005" notes and link to it; `CLAUDE.md` "Status" names this spec; this spec links to all of them

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| A TUI is open (standalone) and `systemctl --user start tidal-player` runs | The daemon exits 3; the unit does not restart it (tidalt #8). The daemon can start once the TUI quits |
| The daemon runs, the user starts `tidal-player` | TUI client; quitting it leaves the music playing |
| The standalone TUI quits while other TUI clients are attached | They show `The player shut down: waiting for it to come back…` and attach to the next player that starts |
| `systemctl --user restart tidal-player` with clients attached | `ShuttingDown`, then the clients reconnect to the new daemon (empty queue until 0009) |
| The player crashes (panic, `SIGKILL`) | Clients see the connection drop and reconnect; the lock is gone with the process; the next player removes the stale socket |
| A binary upgrade while the old daemon runs | New clients refuse it with the version message; nothing crashes on an unknown message |
| Two `tidal-player --add-to-queue …` from a browser handler in quick succession | Both `Open`s are applied in the order received (AC7) |
| A client suspended with Ctrl-Z for a long time | Disconnected after 1024 messages; reconnects on resume with a fresh `Welcome` (AC6, AC13) |
| `$XDG_RUNTIME_DIR` unset (no logind, e.g. some SSH sessions) | `/tmp/tidal-player-<uid>`, private; a daemon under systemd and a shell without the variable then use different directories, so `docs/daemon.md` says to set `XDG_RUNTIME_DIR` (or `TIDAL_PLAYER_RUNTIME_DIR`) in both |
| The runtime directory belongs to someone else or is world-readable | Refused, exit 1 (AC9); never chmod'ed silently |
| Paused for longer than the release delay, then another app plays to the DAC | It gets the card; resume says `Output hw:1,0 is busy (used by …)` and stays paused until the card is free (AC21) |
| Paused, WirePlumber asks for the card | Released at once (AC20) |
| Playing, another app asks for the card | Refused, as in 0003 |
| Stream URL expired during a long pause | 0003's re-resolve on `403`/`410` on resume |
| Session expires while the daemon runs | `LoginRequired` to every client (`playback status` says so); after `tidal-player login`, `LoginRestored` (AC8, 0002 AC9) |
| `playback …` while the player is starting | Retries the connection for 2 s (AC11) |
| `play` (foreground) running, then `playback next` | Works: `play` serves the socket too |

## Test plan

Each automated test is named after its criterion (`ac5_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0004. Server tests run the real server code over `UnixStream::pair()` or a socket in a temp dir, with 0004's fake engine and jobs; no test opens an audio device or touches the network (API calls go to `wiremock` through `TIDAL_PLAYER_API_BASE`, as 0004's CLI tests do). Time (release delay, reconnect interval, request timeout) is injected; no test sleeps longer than the 2 s connect retry, and only AC10/AC11's integration tests do.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (extended) | round trip of every new variant | new types' hand-written stub `Serialize` writes `null` |
| AC2 | `crates/app/src/ipc/codec.rs` :: `ac2_framing` (table) | messages or errors per row; memory bound on the oversized line | stub decoder returns nothing for every input |
| AC3 | `crates/app/src/ipc/codec.rs` :: `ac3_greeting` (table) | exact greeting text; accept / mismatch / not ours | stub accepts every greeting |
| AC4 | `crates/app/src/ipc/server.rs` :: `ac4_subscribe_ordering` (table), `ac4_clients_converge` (200 seeded runs) | client state after `Welcome` + events equals the runtime's snapshot; identical sequences; every client equals the player when idle | stub sends `Welcome` from a snapshot taken outside the runtime thread, so events in between are lost or doubled |
| AC5 | `crates/app/src/ipc/server.rs` :: `ac5_requests_replied_in_order`, `ac5_nothing_dropped` | one reply per id, after its events; counts | stub replies before applying and drops when the input channel is busy |
| AC6 | `crates/app/src/ipc/server.rs` :: `ac6_stalled_client_disconnected`, `ac6_bad_clients_dropped` (table) | disconnection at 1024; others unaffected; reply latency | stub writes to each client synchronously, so the stalled one blocks the player |
| AC7 | `crates/app/src/player_runtime.rs` :: `ac7_open` (table) | commands applied, start rule, error reply, order | stub applies each `Open` when its expansion finishes |
| AC8 | `crates/app/src/ipc/server.rs` :: `ac8_login_status` | events and `Welcome.login_required` | stub always sends `login_required: false` and no events |
| AC9 | `crates/app/src/ipc/paths.rs` :: `ac9_runtime_dir` (table), `ac9_private_dir` | chosen path; created mode; refusals | stub returns `/tmp/tidal-player` for every input and never checks |
| AC10 | `crates/app/tests/daemon.rs` :: `ac10_second_player_exits_3`, `ac10_stale_socket_removed`, `ac10_race_one_wins` | exit codes, messages, a client can connect afterwards | stub daemon never takes the lock, so the second one also starts |
| AC11 | `crates/app/tests/daemon.rs` :: `ac11_tui_attaches` + `crates/app/src/client.rs` :: `ac11_startup_open`, `ac11_connect_retry` | attach without lock or engine; the `Open` sent; retry then message | stub starts a standalone player regardless |
| AC12 | `crates/core/src/ui.rs` :: `ac12_client_connection_states` (table) | state, messages, no `Send` while disconnected, `q` | stub ignores `Welcome`/`Disconnected` |
| AC13 | `crates/app/src/client.rs` :: `ac13_reconnect`, `ac13_no_local_edits` | reconnect cadence, re-subscribe, state replaced; queue unchanged until the snapshot | stub gives up after the first failure |
| AC14 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` | exit 0 and output with an empty state dir | stub client loads the session first and says `Not logged in` |
| AC15 | `crates/app/src/oneshot.rs` :: `ac15_parse` (table), `ac15_status_lines` (table) + `crates/app/tests/daemon.rs` :: `ac15_exit_codes` | messages per row; exit codes; printed lines | stub sends `TogglePause` for every command |
| AC16 | `crates/app/tests/daemon.rs` :: `ac16_daemon_stop` | exit 0 and the lock free afterwards; 1 without a player | stub exits 0 without sending |
| AC17 | `crates/app/tests/daemon.rs` :: `ac17_ready_and_sigterm` | `READY=1` received after the socket accepts; `ShuttingDown` seen by a client; socket gone; exit 0 | stub never notifies and leaves the socket file |
| AC18 | `crates/app/src/daemon.rs` :: `ac18_unit` | snapshot + `contains` checks | stub prints an empty unit |
| AC19 | `crates/audio/tests/engine.rs` :: `ac19_release_while_paused` (table: 16/24-bit, delay 0/10 s/never, seek/play/stop/device while released) | close and release counts; frames byte for byte; positions | stub never releases (counts 0) |
| AC20 | `crates/audio/tests/engine.rs` :: `ac20_request_release` (table over states) | answers, order of close / answer / release, latency | stub refuses always (0003's behaviour) |
| AC21 | `crates/audio/tests/engine.rs` :: `ac21_resume_failure` + `crates/core/src/player/tests.rs` :: `ac21_resume_failed_stays_paused` (table) | event, nothing open, retry; player state, message, no skip | stub player handles `ResumeFailed` as a track failure and skips |
| AC22 | `crates/core/src/player/tests.rs` :: `ac22_released_flag` + `crates/app/src/ui.rs` :: `ac22_released_row` | snapshot flag; ` · device released` in the buffer, snapshot reviewed by eye | stub never sets the flag |
| AC23 | `crates/app/src/play.rs` :: `ac25_player_config` (extended rows) | values and errors | stub ignores the variable |
| AC24 | — reviewed at acceptance | docs exist, match this spec, are linked | — |

Not covered by automated tests, on purpose, and checked by hand at acceptance on the user's machine (results in the PR description):

- The generated unit under systemd: `enable --now` starts it, `systemctl --user status` shows it active (ready notified), `stop` exits cleanly; with a standalone TUI open, `start` fails once with status 3 and is not restarted
- Pause on a `hw:` DAC, wait past the release delay: another application (`speaker-test -D hw:1,0`, or a browser through PipeWire) can use the card; quit it, resume: the track continues where it was, with no audible skip or repeat
- Paused, then a PipeWire client starts on that card: does WirePlumber send `RequestRelease` (and does it get the card at once)? Record what happens; the assumption below depends on it
- `playerctl` is not expected to work yet (MPRIS is 0010)

## Crate placement

- `tidal-player-core::protocol`: `ClientMessage`, `ServerMessage`, `Command::Open`, `NowPlaying.released`. `player`: `ResumeFailed`, `Released`/`Resumed` handling. `ui`: client connection states. No transport, no `serde_json` (0001 AC3)
- `tidal-player-audio`: release delay, release/reacquire, `RequestRelease` answers (through the existing `Reserver` seam), the new events; still no tokio
- `tidal-player`: `ipc::{codec, paths, server}` (std `UnixListener`/`UnixStream`, one reader and one writer thread per client feeding the runtime's input channel, the outbox), `client.rs` (connect, retry, reconnect, the TUI client loop), `oneshot.rs` (`playback`), `daemon.rs` (startup, signals, `sd_notify`, the unit text), `reserve.rs` (the exported object asks the engine instead of refusing). No new third-party dependency: `File::try_lock` is in std since 1.89, `UnixDatagram` covers `sd_notify`, tokio's `signal` feature is already enabled
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-07):

- tidalt's design and bugs under "Context", from its source, history (`git log`, the commits named) and closed issues #3–#8 and #19–#21
- 0004's runtime takes commands on one `mpsc` channel and has one event receiver, and the TUI expands items itself through `Expander` (`crates/app/src/player_runtime.rs`, `crates/app/src/main.rs`): hence the fan-out and `Open`
- `std::fs::File::try_lock` and `SocketAddr::from_abstract_name` (for an abstract `$NOTIFY_SOCKET`) compile and behave as expected on the pinned toolchain (rustc 1.97.1; a second `try_lock` on the same file fails)
- `tidal_player_core::Item` already derives `Serialize`/`Deserialize`
- `reserve.rs`'s exported object refuses every `RequestRelease` today

Assumptions to check during implementation or at acceptance:

- 0003's fake sink can report a device delay, so AC19's "first unheard frame" is testable; if not, the slice adds it to the fake
- WirePlumber sends `RequestRelease` to the holder when a PipeWire client wants a reserved card (manual check above). If it does not, other applications only get the card after the release delay, which is why the delay exists
- The 0003 reservation dance plus reopen makes resume after a release take well under a second on the user's DAC (manual check)
- A browser `tidal://` handler is not part of this spec; `docs/daemon.md` may show `tidal-player playback add` as one

## Decisions (answered by the user, 2026-10-07: all as proposed)

1. **Transport**: a Unix socket with JSON lines (*accepted*) rather than a private D-Bus interface as tidalt had. The `Command`/`Event` types already serialise to JSON; a socket needs no session bus (a headless box over SSH may have none), carries push events and a connection lifetime (attach/detach) for free, and is testable in-process. MPRIS (0010) is D-Bus and stays separate: a public adapter on top of the player, not the client channel
2. **Who is the player when no daemon runs**: the first `tidal-player` (standalone, as in 0004), which also serves the socket (*accepted*). Alternative: `tidal-player` always starts a background daemon and is only ever a client, so quitting the TUI never stops the music. Proposed keeps 0001's three modes and spotify-player's behaviour (quitting stops)
3. **Item expansion in the player** (`Command::Open`, *accepted*): clients need no session, no passphrase and no API access, and one-shot commands are a single message. Alternative: clients expand and send tracks (needs a session in every client, which 0006's browsing will need anyway). Proposed for now; 0006 decides how its browsing pages fetch
4. **Release delay**: 10 s by default, `0` for tidalt's release-at-once, `never` (*accepted*). `RequestRelease` is honoured while paused whatever the setting
5. **systemd unit**: printed by `tidal-player daemon unit`, installed by the user (*accepted*), rather than tidalt's `setup --daemon` writing it and running `systemctl` itself
6. **One-shot commands**: the table under "One-shot commands" (*accepted*): toggles only, as 0004's protocol has; explicit `play`/`pause`/`set` commands come with MPRIS's setters (0010)

## Out of scope

- MPRIS2, media keys, `playerctl` (0010)
- Remembering the queue, position and volume across daemon restarts (0009)
- Socket activation, a system-wide (multi-user) daemon, remote clients over TCP
- A `tidal://` URL handler and `.desktop` file (tidalt's `setup`)
- Browsing (library, search) in a client: 0006/0007 decide whether it fetches through the player or with its own session
- Explicit play/pause/set one-shot commands (decision 6)
- Choosing the output device from a client (0004 "Out of scope"; `SetDevice` is not in the protocol yet)
- A client taking over as the player when the player goes away
