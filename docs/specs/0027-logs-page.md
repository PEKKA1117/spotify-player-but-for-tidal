# 0027 — Logs: a log file and the logs page

- **Status**: approved (2026-11-10; "What the program logs" and AC16–AC19 added 2026-11-11 at the user's request; AC1–AC15 are implemented)
- **Owner**: tech-lead (primary session)
- **Issue**: [#64](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/64)
- **Depends on**: 0001 (implemented: the crates, `Command`/`Event`, the "logging" row this spec replaces), 0005 (implemented: the socket, requests answered to one client, the lock, AC14), 0006 (implemented: pages, the history, disconnected rules), 0008 (implemented: `app.toml`, the keymap, the keys help, the skipped spotify-player commands), 0012 (implemented: `/` and its title), 0013 (implemented: key hints), 0017 (implemented: themes)
- **User docs**: [`docs/tui.md`](../tui.md) gains a "Logs" section and the key; [`docs/config.md`](../config.md) gains `log_level`, `OpenLogs` and drops it from the skipped list; [`docs/daemon.md`](../daemon.md) says where the daemon's log goes; and their zh-TW copies (AC13)

## Context

The code has 40-odd `tracing` calls (a rejected refresh token, a card that cannot be reserved, a stream that was reconnected, a corrupt state file, a client that was dropped) and **no subscriber**: every one of them writes nothing (0001 "logging", found by 0026's audit; 0010 and 0025 each rediscovered it). When something goes wrong that the message row does not explain, there is nothing to read.

spotify-player writes a log file per run into its cache directory and, since v0.24, shows the run's lines in a page (`OpenLogs`, `g o`): an in-memory buffer, scrolled by a line offset, `ERROR` lines red and `WARN` lines yellow. This player skips `OpenLogs` in a copied `keymap.toml` (0008). tidalt had no log.

Almost all of this player's log lines come from the **player** side (auth, the audio device, streams, persistence, the socket). With a daemon, that is another process than the TUI, so a page that showed the TUI's own buffer, as spotify-player's does, would be empty exactly when it is needed.

### Decisions (the user's, 2026-11-10)

1. **An attached TUI shows the player's log, through the protocol.** The player keeps its recent lines and answers a request for them, as it does for lyrics and devices; a client opens nothing of the player's on disk (0005 AC14). The TUI's own few lines (cover, keymap) are merged into the same page; a line's target (`tidal_player::tui::…`) says where it came from
2. **Every player writes a log file in the state directory**, standalone, daemon and `play` alike, so a crash leaves something to read. The daemon also writes its lines to stderr, which is the journal under systemd

### What could go wrong here, and the criterion that covers it

1. **A secret in the log**: HTTP crates log headers at `debug`/`trace`, and the `Authorization` header is the session → other crates' lines are taken at `warn` and above only, whatever `log_level` says (AC2)
2. **The log file growing without end** → rotated at a fixed size, at most two files (AC4)
3. **Logging stopping the music**: a full disk, an unwritable directory → the file is given up, the player runs on, and the buffer says why (AC4)
4. **A flood of lines disconnecting clients**: lines pushed as events would fill a slow client's outbox (0005: 1024 messages, then dropped) → lines are never pushed; the page asks for the lines after the last one it has, about once a second, only while it is on top (AC5, AC8)
5. **The view jumping while reading**: a new line arriving while the user reads an older one → the view follows only while it is at the bottom (AC9)
6. **Two processes writing one file** → only the player, which holds `player.lock`, writes the file; a client writes none (AC3)

## Behaviour

### What is logged

- A **log line** is one `tracing` event: its time, level (`ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`), target (the module path, `tidal_player::player::reserve`) and text. The text is the event's message followed by its other fields as ` name=value`, in the order they were given (`could not save the refreshed session error=…`). A line break in the text is written as a space
- **`log_level`** in `app.toml` (default `"info"`; `"off"`, `"error"`, `"warn"`, `"info"`, `"debug"`, `"trace"`, any letter case) is the lowest level kept from this program's crates (targets `tidal_player`, `tidal_player_core`, `tidal_player_api`, `tidal_player_audio` and their modules). `TIDAL_PLAYER_LOG_LEVEL` beats the file, as for every setting; a value that is none of the six is a bad setting (exit 2, 0008's message)
- **Other crates' events** (`reqwest`, `hyper`, `zbus`, `symphonia`, …) are kept at `WARN` and `ERROR` only, at every `log_level` but `"off"`
- `"off"`: nothing is kept, no file is created or touched
- Each process reads its own `log_level`: a daemon the daemon's, an attached TUI its own (for its own lines)
- Nothing a user must see is only in the log (0001): errors still go to the message row or stderr as before. No existing message moves

### What the program logs

Giving the 40-odd existing calls somewhere to go was not enough: they are nearly all warnings about rare failures, so a healthy run logged nothing, and what the TUI shows its user when something fails was in no log at all (found at the hands-on check, 2026-11-11). The program logs these lines, each one `tracing` event with the message and fields shown (fields in this order; AC2 turns them into the text):

**A run's life, at `INFO`** (in every process that keeps a log):

| When | Message and fields |
|---|---|
| The log is installed | `starting version=<x.y.z> mode=<standalone\|daemon\|play\|client> log_level=<level> config=<config dir>` |
| A player's output device is opened (the first time, after a release, after a device change) | `device opened device=<name> format=<sample format> rate=<Hz> channels=<n> bit_perfect=<true\|false>` |
| A player gives the device up while paused (0005), or closes it | `device released device=<name>` |
| A track starts playing (an entry becomes the playing one, gapless advances included) | `playing track=<id> title=<title> quality=<the stream's quality> requested=<the configured quality>` |
| The queue runs out or playback stops | `stopped` |
| The login is lost / restored (0002) | `WARN login required` / `INFO login restored` |
| A player or TUI shuts down in order | `shutting down reason=<quit\|shutdown command\|signal\|player gone>` |

**At `DEBUG`**: `client connected client=<n>` and `client disconnected client=<n>` in a player (a status bar that runs a one-shot command every second connects every second, so these are not `INFO`).

**What the user was told, at `WARN`**, so that a message that was replaced on screen can still be found:

| Where | When | Message and fields |
|---|---|---|
| player | It answers a request with `Err` (`Reply`, `LibraryReply`, `DevicesReply`; not `LogsReply`) | `request failed request=<the command's or request's name> reason=<the answer's text>` |
| player | Its snapshot's `message` changes to a new text (`Could not save the playback state: …`, a failed hook, a device that would not open) | `message text=<the text>` |
| TUI | The connection is lost, a reconnect fails, or the versions differ | `disconnected reason=<the text shown>`; a reconnect is `INFO reconnected` |
| TUI | A start-up notice (`keymap.toml`/`theme.toml` entries skipped, 0008, 0017) | `notice text=<the text>` |
| TUI | An export fails (AC14's failure) | `export failed reason=<reason>` |

- A standalone TUI is a player and a TUI in one process with one log, so a failed request appears once (the player's line); the TUI's own lines are only those in the table
- **Never logged**, in these lines or any other: a token, the `Authorization` header, a stream or cover URL (a stream's host name is allowed), the account's name, e-mail or ID. A request's name is the variant's name (`LoadQueue`, `Page`), never its payload
- **A panic** (0001's hook, in every process that keeps a log): before the terminal is restored and the report printed, one `ERROR` line `panic location=<file:line:column> message=<the panic's message>` is written straight to the process's log file (a player's; a client has none) and its buffer, without going through `tracing` and without waiting for a lock: if the log is busy (the panic happened while it was writing) the line is skipped. The hook then does what it did before, in the same order

### Who keeps what

| Process | Buffer (last 1000 lines) | `tidal-player.log` | stderr |
|---|---|---|---|
| standalone TUI | yes | yes | no (the terminal is the TUI's) |
| `daemon` | yes | yes | yes |
| `play` | yes | yes | no (its output is 0003's) |
| attached TUI | yes, its own lines | no | no |
| every other command (`login`, `playback …`, `playlist …`, `devices`, …) | no | no | no |

- The **buffer** holds the process's last **1000** lines; the oldest is dropped first. A player numbers its lines `seq` 1, 2, 3, … for its lifetime
- The daemon's **stderr** line is `LEVEL target: text`, without the time (the journal has it)

### The log file

- `tidal-player.log` in the state directory (`$TIDAL_PLAYER_STATE_DIR`, else `$XDG_STATE_HOME/tidal-player`, else `~/.local/state/tidal-player`, where 0009's `playback.json` is), created with mode `0600`, appended to: one line per log line, written when it happens,

  ```
  2026-11-10T21:04:05.123+08:00  WARN tidal_player::player::reserve: cannot reserve card 1: busy
  ```

  the time in local time with its UTC offset and milliseconds, the level right-aligned to 5 columns
- **Rotation**: when the file is larger than **1 MiB** at start, or would pass it with the next line, it is renamed to `tidal-player.log.old` (replacing one that is there) and a new file is started. So the log takes at most about 2 MiB, and the previous run's end is always in one of the two
- A file that cannot be created, written or rotated (no directory, read-only, full disk) is **given up for the rest of the run**: one `WARN` line `cannot write <path>: <reason>` goes to the buffer (and a daemon's stderr), and the player runs as if there were no file. It is not an error exit
- `logout` leaves the log files (they hold no session)

### Talking to the player

A request of its own, like `Devices` (0014), answered to the asking client only:

- `ClientMessage::Logs { id, after: Option<u64> }` → `ServerMessage::LogsReply { id, result: Result<LogBatch, String> }`
- `LogBatch { lines: Vec<LogLine>, off: bool }`: the buffer's lines with `seq > after` (all of them for `None`), oldest first; `off` is `true` when the player's `log_level` is `"off"`
- `LogLine { seq: u64, at: SystemTime, level: LogLevel, target: String, text: String }` (`tidal_player_core::log`; `at` in milliseconds since the Unix epoch on the wire; `LogLevel` is the five levels)
- An `after` greater than the newest `seq` (the player restarted) is answered with every line, as `None` is
- Serving a `Logs` request logs nothing itself
- A standalone TUI asks the same way through its in-process client (0001)

### The page

| Opened with | Shows | Title row |
|---|---|---|
| `g o` (`OpenLogs`), from anywhere | the player's log lines and the TUI's own, oldest at the top, newest at the bottom | `Logs` |

- A page of the history (0006): pushed on top; on top already, the key does nothing; going back leaves it
- **Lines**: each is drawn as `HH:MM:SS LEVEL target: text` (local time; the level left-aligned to 5 columns), wrapped at the window's width by display width. An attached TUI merges the player's lines and its own by time (equal times: the player's first); a standalone TUI has one buffer, which is both. The page keeps the last 1000 lines after merging
- **Colours**: an `ERROR` line is drawn in the theme's `error` component, a `WARN` line in the palette's `yellow`, the rest in the screen's style
- **Asking**: while the page is on top and the client is connected, it asks for the lines after the last `seq` it has: once when it comes on top, then **one second** after each answer (never two requests in flight). Not on top, it asks nothing; coming back onto it asks at once. After a new `Welcome` (a reconnect) it forgets the player's lines and asks with `after: None`
- **Following**: the page opens showing the newest lines, the last one on the window's last row, and *follows*: new lines scroll the view. The list movement keys (`j`/`k`/`down`/`up`, `C-f`/`C-b`/`page_down`/`page_up`, `g g`/`G`) scroll by log lines, clamped to the lines. A view that is not at the bottom does not follow: it stays on the rows it shows while lines arrive (and while old lines are dropped it moves only when its own top row is dropped), and the title shows `Logs · scrolled`. Reaching the bottom again (`G`, or scrolling down to it) or **`Esc`** (`ClosePopup`, with no popup or prompt open and no kept filter) follows again
- **Filtering** (0012): `/` filters the lines as it filters a list: only lines whose drawn text contains the filter, ignoring case, stay; the title gains 0012's parts (`Logs · /reserve · 3 matches`); `Enter` keeps the filter, `Esc` clears it (a kept filter first, then following, one press each); a filter matching nothing shows 0012's `No rows match "…"`. New lines that match appear as they arrive
- **Messages in the window**: `Loading…` before the first answer; `No log lines yet` for none; `Logging is off: set log_level in app.toml` when the player's `off` is `true` and the TUI has no lines of its own; failures `Could not load the log: <reason>` (the lines already shown stay, and the page asks again a second later); a lost connection shows 0006's disconnected message as is, and the lines already shown stay
- Keys that act on rows (`Enter`, `Z`, the actions popup on a row, `s …`, `d …`, `r`) do nothing on the logs page; global keys work as everywhere

### Exporting

- **`e`** (`ExportLogs`) on the logs page writes the lines the page holds to a new file: with a kept filter, only the lines that match (while a filter is being typed, `e` is a letter of it); all of them, not only the rows in view; the player's and the TUI's own, in the page's order
- The file is `tidal-player-logs-YYYYMMDD-HHMMSS.log` (local time) in the cache directory (`$TIDAL_PLAYER_CACHE_DIR`, else `$XDG_CACHE_HOME/tidal-player`, else `~/.cache/tidal-player`; created if missing), mode `0600`, one line per log line in the log file's format (full local time with offset). A name already taken (two exports in one second) gets `-2`, `-3`, … before `.log`; an export never overwrites a file
- The TUI writes it (the client process, attached or standalone): it is the page's content, not the player's file, so it works while disconnected with the lines already shown
- The message row says `Exported 42 log lines to <path>`; a failure `Could not export the log: <reason>` (nothing is left half-written under the final name); with no line to write, `No log lines to export`, and no file
- Anywhere but the logs page, `ExportLogs` does nothing

### Keys

| Command | Default keys | Acts in | Help text |
|---|---|---|---|
| `OpenLogs` | `g o` | everywhere | `open the logs page` |
| `ExportLogs` | `e` | the logs page | `export the shown log lines to a file` |

`ExportLogs` is this player's own command. `OpenLogs` leaves 0008's list of skipped spotify-player commands: a copied `keymap.toml` that binds it now binds it. The keys help (0008) and the key hints (0013) list it like any command.

## Acceptance criteria

- **AC1** — Protocol: `ClientMessage::Logs`, `ServerMessage::LogsReply`, `LogBatch`, `LogLine` and `LogLevel` round-trip through serde and the socket's framing (`at` as milliseconds since the epoch); an unknown level fails to decode
- **AC2** — Capture (pure, over a level and a target): a line is kept exactly when `log_level` is not `off` and either the target is one of this program's crates (or a module of one) and the level is at or above `log_level`, or it is another crate's and the level is `WARN` or `ERROR` (table over every `log_level`, both kinds of target, and look-alike targets such as `tidal_player_extra`). The text is the message, then the fields as ` name=value` in order, line breaks as spaces (table)
- **AC3** — Buffer and roles: the buffer keeps the last 1000 lines in order, drops the oldest first, numbers `seq` from 1 without gaps, and `after(n)` returns the kept lines with `seq > n` (everything for `None` and for `n` beyond the newest). Each run mode installs what the "Who keeps what" table says (table over the modes: buffer, file, stderr)
- **AC4** — File: lines are appended in the format above (table over levels and a fixed time and offset); the file is created with mode `0600`; a file over 1 MiB at start, and a line that would pass 1 MiB, rotate to `tidal-player.log.old`, replacing the old one; a path that cannot be written is given up after exactly one `cannot write <path>: <reason>` line in the buffer, later lines still reach the buffer, and nothing panics or exits; `log_level = "off"` creates no file
- **AC5** — Serving: a `Logs` request is answered once, to the asking client only, with the lines after `after` and the player's `off`; other clients receive nothing; no `Event` carries a log line; answering adds no line to the buffer
- **AC6** — Settings: `log_level` is read from `app.toml` and `TIDAL_PLAYER_LOG_LEVEL` with the usual precedence, any letter case; default `info`; another value exits 2 with 0008's message naming `log_level`; `examples/app.toml` carries the default
- **AC7** — Opening: `g o` pushes the logs page and emits one `Logs { after: None }`; on top already, it does nothing; going back pops it; `OpenLogs` is in the default keymap, the keys help and the key hints with its text, is no longer reported as unsupported, and can be rebound or unbound
- **AC8** — Asking: after an answer the next request is due one second later, carries the newest `seq` received, and is not sent while one is in flight, while the page is not on top, or while disconnected; coming back on top asks at once; an answer with another `id` is ignored; a `Welcome` clears the player's lines and the next request has `after: None`; a failed answer keeps the lines and asks again a second later
- **AC9** — Display state (pure): lines merged by time with the player's first on ties, capped at 1000; following at the bottom; each movement key's scroll, clamped; not following away from the bottom, with the view holding its rows as lines arrive and are dropped; following again at the bottom and on `Esc`; the titles (`Logs`, `Logs · scrolled`, with the filter's parts); the window messages for each state; row keys inert
- **AC10** — Filter: `/` on the logs page filters lines by their drawn text, ignoring case, with 0012's typing, `Enter`, `Esc` and title; `Esc` clears a kept filter before it follows again; arriving lines are filtered too
- **AC11** — Rendering: at 80 × 24 and 50 × 20, snapshots of a log with every level (colours: `error` component, palette yellow), wrapped long and CJK lines, the scrolled title, a filter, and each window message; no panic at any size
- **AC12** — End to end: against a daemon started with a state directory, an attached client's `Logs` request is answered with the daemon's lines, `tidal-player.log` exists with mode `0600` and holds them, the daemon's stderr holds them without the time, and the client process has created no file in the state directory
- **AC13** — Docs: `docs/tui.md` (the page, its keys, following, filtering, exporting), `docs/config.md` (`log_level`, `OpenLogs`, the skipped list without it), `docs/daemon.md` (the log file, the journal), `README.md`, their zh-TW copies, `examples/app.toml` and `examples/keymap.toml` match this spec; 0001's "logging" row and 0008's skipped list point here
- **AC14** — Export, the model (pure): on the logs page `e` emits one export of the held lines, filtered by a kept filter, in the page's order, whatever the scroll position and whether connected or not; with no such line it emits none and the message row says `No log lines to export`; on any other page it does nothing; the result sets the message row (`Exported N log lines to <path>`, with `1 log line` for one, or `Could not export the log: <reason>`); `ExportLogs` is in the default keymap, the keys help (in the logs page's section) and can be rebound or unbound
- **AC15** — Export, the file: the export is written to the cache directory under the name above for a given local time, mode `0600`, in the log file's line format; a taken name gets the next `-N`; the directory is created when missing; a directory that cannot be written gives the failure and leaves no file
- **AC16** — A run's life: each row of "A run's life" is logged once per occurrence with its level, message and fields in order, and the two `DEBUG` lines at `DEBUG` (table: for each row, the seam's input → the captured event); `playing` is logged once per entry that starts, a gapless advance included, and not on resume from pause or on a seek; no captured line of these tests contains a token, a URL or an account field (the fixtures carry recognisable ones)
- **AC17** — What the user was told: every `Err` answer of a player logs one `request failed` with the request's name and the reason, and no payload (table over `Reply`, `LibraryReply`, `DevicesReply`; `LogsReply` logs nothing, AC5); a snapshot `message` logs once when its text changes and not again while it stays; the TUI's rows log as the table says; a standalone TUI logs a failed request once
- **AC18** — Panic: the line's text for a location and a message (table: `&str` and `String` payloads, another payload → `message=<non-string panic>`); with a log installed, the hook writes it to the file and the buffer before `restore` and the previous hook run, in that order (0001 AC10 still holds); with the log's lock held by the panicking thread, the hook skips the line and still restores and reports, without blocking
- **AC19** — Docs: `docs/tui.md` "What is logged and where" lists these lines by group (a run's life, what you were told, a panic) and says what is never logged; its zh-TW copy matches; `CLAUDE.md`'s Logs entry says the log records a run's life and the failures shown

## Edge cases & errors

| Case | Behaviour |
|---|---|
| A burst of more than 1000 lines between two requests | The page gets the newest 1000; the rest are in the file only. No marker is drawn |
| The player restarts while the page is open | Disconnected message; on reconnect (`Welcome`) the old player's lines are dropped and the new one's are asked for |
| An old daemon and a new TUI (or the other way) | 0005's version message; no `Logs` is ever sent to a player of another version |
| A line longer than the window | Wrapped. Scrolling is by log lines: the view's top row is always a line's first row. While following, the newest line's last row is on the window's last row, however the lines wrap |
| A line with control characters | Drawn without them (0012's rule for typed text applies to drawn text) |
| A terminal too small for the window | 0006's rule: nothing panics (AC11) |
| The state directory does not exist | Created as 0009 creates it; if that fails, the file is given up (AC4) |
| The clock steps backwards | Lines keep their `seq` order from the player; only the merge with the TUI's own lines uses the time |
| A panic | 0001's panic hook is unchanged; it writes nothing to the log |
| `log_level = "trace"` on a busy player | Other crates stay at `WARN`; the file rotates more often. Nothing else changes |

## Test plan

Each test is named after its criterion (`ac4_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors). File tests write into a `tempfile` directory; time, offset and size limit are parameters of the pure parts, so no test waits or writes a megabyte through the real clock.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | stub `Serialize` writes `null` |
| AC2 | `crates/core/src/log.rs` :: `ac2_kept` (table), `ac2_text` (table) | which lines are kept; the text | stub keeps every line; stub text is the message alone |
| AC3 | `crates/core/src/log.rs` :: `ac3_buffer` (table) + `crates/app/src/logging.rs` :: `ac3_roles` (table) | order, cap, `seq`, `after`; the sinks per mode | stub buffer keeps nothing; stub roles give every mode all three sinks |
| AC4 | `crates/app/src/logging.rs` :: `ac4_file_format` (table), `ac4_rotation`, `ac4_gives_up_once`, `ac4_off_creates_nothing` | line format and mode; rotation; one warning then silence | stub writer writes nothing |
| AC5 | `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (new row) + `crates/app/src/player/runtime.rs` :: `ac5_logs_reply` | one reply, to the sender; lines after `after`; `off`; buffer unchanged | stub answers an empty batch |
| AC6 | `crates/app/src/settings/tests.rs` :: `ac26_settings_precedence` (new rows) + `crates/app/src/config/tests.rs` :: `ac10_app_toml` (new rows) + `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` | precedence, case, default, the bad value; the example | stub ignores the setting |
| AC7 | `crates/core/src/ui/logs/tests.rs` :: `ac7_open_logs` (table) + `crates/core/src/ui/keymap/tests.rs` :: `ac7_logs_keys`, `ac12_unsupported_notice` (rows moved to another skipped command) + `crates/core/src/ui/help/tests.rs` :: `ac7_help_keys_and_texts` (new row) | push, request, history; default key, help, hints, unbinding | stub keeps `OpenLogs` unsupported |
| AC8 | `crates/core/src/ui/logs/tests.rs` :: `ac8_asking` (table) + `crates/core/src/ui/browse/tests.rs` :: `ac16_disconnected_and_login` (new rows) | when a request is due and what it carries | stub asks on every tick |
| AC9 | `crates/core/src/ui/logs/tests.rs` :: `ac9_display_state` (table) | merge, cap, follow, scroll, titles, messages, inert keys | stub always follows |
| AC10 | `crates/core/src/ui/logs/tests.rs` :: `ac10_filter` (table) | filtered lines, title, `Esc` order | stub shows every line |
| AC11 | `crates/app/src/tui/render.rs` :: `ac11_logs_80x24` (one snapshot per row), `ac11_logs_narrow_50x20`, `ac17_no_panic_any_size` (new page) | `contains` + snapshots | stub render draws the queue |
| AC12 | `crates/app/tests/daemon.rs` :: `ac12_daemon_log` | reply, file and mode, stderr, nothing created by the client | stub installs no subscriber, so the reply is empty |
| AC13 | — reviewed at acceptance | docs match this spec, linked, zh-TW in step | — |
| AC14 | `crates/core/src/ui/logs/tests.rs` :: `ac14_export` (table) + `crates/core/src/ui/keymap/tests.rs` :: `ac14_export_key` | the exported lines; messages; inert elsewhere; the key and help | stub export emits every line, unfiltered |
| AC15 | `crates/app/src/tui/logs_export.rs` :: `ac15_export_file` (table) | name, mode, content, `-N`, created directory, failure | stub writes nothing |
| AC16 | `crates/app/src/logging/events.rs` :: `ac16_run_life` (table), `ac16_playing_once_per_entry`, `ac16_no_secrets` | level, message, fields per row; once per entry; nothing secret | stub functions log nothing |
| AC17 | `crates/app/src/logging/events.rs` :: `ac17_request_failed` (table), `ac17_message_once`, `ac17_tui_rows` (table) + `crates/app/src/player/runtime.rs` :: `ac17_err_answers_are_logged` | one line per `Err` answer, name and reason; once per text; the TUI's lines; once in a standalone | stub logs nothing |
| AC18 | `crates/app/src/logging.rs` :: `ac18_panic_line` (table), `ac18_panic_skips_when_busy` + `crates/app/tests/panic_hook.rs` :: `ac18_panic_is_logged_first` | the text; no blocking; file and buffer, then restore, then report | stub line is empty; stub hook writes nothing |
| AC19 | — reviewed at acceptance | docs match this spec, zh-TW in step | — |

Checked by hand at acceptance with a real account (results in the PR's hands-on check): `g o` in a standalone TUI and in a TUI attached to a daemon shows the player's lines; a second attached TUI shows the same; new lines appear within about a second while following and do not move a scrolled view; `/` narrows them and `e` writes exactly the narrowed lines to the file the message row names; a healthy start and a played track show `starting`, `device opened` and `playing` at `info`, and `log_level = "debug"` adds the client lines; `"off"` shows the message; a request that fails (a track ID that does not exist) shows `request failed`; `journalctl --user -u tidal-player` (the maintainer's, by hand) shows the daemon's lines; no token or account name appears in a screenshot or in the file at `debug`.

## Crate placement

- `tidal-player-core`: `log` (new: `LogLevel`, `LogLine`, `LogBatch`, the buffer, what is kept, the line formats), `protocol` (the two messages), `ui` (the logs page: `PageKind::Logs`, asking, following, filter), `ui::keymap` (`OpenLogs`, `ExportLogs` and their defaults). No I/O, no new dependency
- `tidal-player` (binary): `logging.rs` (new, top level: every mode uses it): the `tracing` subscriber that feeds the buffer, the file and stderr, and the panic line; `logging/events.rs`: one function per line of "What the program logs", called at the seams, so each line's level, message and fields are tested in one place; `settings.rs`/`config.rs` (`log_level`); `player/runtime.rs` and `ipc/` (serving `Logs`); `client.rs` (asking); `tui/` (rendering, the poll timer beside `HintTimer`, `logs_export.rs` for the export file). New dependencies, declared in the root manifest: `tracing-subscriber` (the registry and layer, default features off) and one crate for local time
- `tidal-player-api`, `tidal-player-audio`: unchanged (their `tracing` calls now reach a subscriber)

## Out of scope

- A one-shot command that prints the player's log (`tail -f` on the file, or `journalctl`, do it)
- Structured (JSON) logs, per-module levels, `RUST_LOG`, a setting for the buffer or file size
- A cursor, selection or copying to the clipboard on the logs page (the clipboard is #57's); choosing the export's path or name
- New theme components for log levels
- Rewording or removing the log calls that existed before this spec; a log file for an attached TUI (its lines live in its buffer: `e` exports them)
