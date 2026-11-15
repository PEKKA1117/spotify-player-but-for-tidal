# 0029 — Desktop notification on track change

English | [正體中文](zh-TW/0029-notify.md)

- **Status**: approved (2026-11-16)
- **Owner**: tech-lead (primary session)
- **Issue**: [#55](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/55)
- **Depends on**: 0004 (implemented: the player's states and started tracks), 0005 (implemented: the daemon), 0008 (implemented: `app.toml`), 0010 and 0016 (implemented: the cover cache), 0025 (implemented: what the runtime knows about a started track), 0027 (implemented: the log)
- **User docs**: [`docs/config.md`](../config.md) (the four settings and a new "Desktop notifications" section) and its zh-TW copy, `examples/app.toml`, `README.md` and its zh-TW copy (AC16)

## Context

With the player in a daemon, or the TUI on another workspace, nothing on screen says which track just started. spotify-player has a `notify` feature for this: a desktop notification on each new track, set with `enable_notify`, `notify_format`, `notify_timeout_in_secs` and `notify_transient`. This spec adds the same settings with the same meaning, and three things spotify-player's does not do: a new notification replaces the previous one instead of stacking, a body is safe on servers that read it as markup, and a missing notification server is never an error.

tidalt had none of this.

## Behaviour

### What is shown

When a track starts, the desktop shows a notification with the track's album cover, a summary and a body:

```
[cover]  Song (Live) • First Artist, Second Artist
         The Album
```

The player sends it, whichever kind of player it is: standalone, daemon or `play`. A TUI attached to a daemon sends nothing, so a track gives one notification however many clients are attached, and the daemon notifies with none.

A notification replaces the player's previous one, so skipping through a queue leaves one notification on screen, not a stack.

### The settings

```toml
enable_notify = true
notify_format = { summary = "{track} • {artists}", body = "{album}" }
notify_timeout_in_secs = 0
notify_transient = false
```

| Setting | Value | Default | Environment |
|---|---|---|---|
| `enable_notify` | boolean | `true` | `TIDAL_PLAYER_ENABLE_NOTIFY` |
| `notify_format` | a table: `summary` and `body`, both optional strings | spotify-player's, above | |
| `notify_timeout_in_secs` | integer 0–3600 (`0`: the notification server's own time) | `0` | `TIDAL_PLAYER_NOTIFY_TIMEOUT_IN_SECS` |
| `notify_transient` | boolean (`true`: the server does not keep it in its history) | `false` | `TIDAL_PLAYER_NOTIFY_TRANSIENT` |

The names, defaults and meaning are spotify-player's. The precedence is 0008's (the environment over the file). `notify_format` exists only in `app.toml`, like every table setting.

### The format

`summary` and `body` are texts with placeholders:

| Placeholder | Value |
|---|---|
| `{track}` | The title, with its version in parentheses when it has one (`Song (Live)`), as the event hook's `TIDAL_PLAYER_TRACK_TITLE` |
| `{artists}` | The artists, joined with `, ` |
| `{album}` | The album's title; empty when the track has no album |

Everything else is kept as written, `\n` included. A `{name}` that is not one of the three is an `app.toml` error, so a typo is found at start and not as a hole in a notification. `summary` must not be empty; `body` may be (`body = ""` gives a notification with a summary only). A future `playback_format` (the playback window's text) will use the same placeholders.

### Which tracks notify

A notification follows a track that **starts**: its first frame was written, or playback moved into it gaplessly. This is the event hook's `Changed` (0025), with one difference: a track with the same ID as the track that started before it is not notified again. So:

- Skipping quickly through a queue notifies only the track that really starts
- Repeat-one, and the same track twice in a row in the queue, notify once
- Playing a track again after a stop, with no other track between, does not notify again
- `A`, `B`, then `A` again notifies three times
- A restored session (0009) notifies nothing until a track starts
- Pause, resume, seek, volume and device changes notify nothing

### The cover

The cover is the file the player keeps in its cover cache (0010, 0016). The cache starts downloading a cover when its track becomes current, which is before the track starts, so the file is usually there already.

- **Cached**: the notification carries it
- **Not cached yet**: the notification waits for it up to 2 s, then goes out without a cover. It is sent once, never sent again when the cover arrives later
- **No cover** (the track has no album cover, or `max_cover_arts = 0`): the notification goes out at once without one

A track that starts while another's notification waits takes its place: the waiting one is never sent.

### What the server receives

One call to `org.freedesktop.Notifications.Notify` on the session bus:

| Argument | Value |
|---|---|
| `app_name` | `tidal-player` |
| `replaces_id` | The ID the server gave the player's previous notification, `0` for the first |
| `app_icon` | empty |
| `summary` | The rendered `summary` |
| `body` | The rendered `body` |
| `actions` | none |
| `hints` | `image-path`: the cover's absolute path, when there is one. `transient`: `true`, when `notify_transient` is set |
| `expire_timeout` | `-1` (the server decides) when `notify_timeout_in_secs` is `0`, else the seconds in milliseconds |

A server that announces the `body-markup` capability reads the body as markup, where a title like `Rock & Roll <Live>` would be cut or dropped. For such a server `&`, `<` and `>` in the body become `&amp;`, `&lt;` and `&gt;`. The summary is never markup and is sent as it is. The capabilities are asked with the first notification, and again after a call failed (the server may have been replaced).

### When it cannot notify

A notification is never worth an error: nothing reaches the message row, and playback never waits for the bus.

- **No session bus** (SSH, a bare console): one `info` line in the log at start, and nothing is tried again for that run
- **No notification server**, or a call that fails or takes longer than 5 s: one `warn` line in the log. Further failures say nothing until a notification succeeds. The next track tries again, so a server that starts after the daemon (a login where the player's service comes up first) works from then on
- **`enable_notify = false`**: no thread is started and no connection is opened for it

The notifications use their own connection to the session bus, so they work with `mpris = false`.

## Acceptance criteria

1. **AC1 Settings**: `enable_notify`, `notify_timeout_in_secs` and `notify_transient` are read from `app.toml` and their environment variables with 0008's precedence and the defaults above; `notify_format` takes a table with optional `summary` and `body` strings, a missing one keeping its default. A value of the wrong type, `notify_timeout_in_secs` outside 0–3600, an unknown field in `notify_format` and an empty `summary` are `app.toml` errors with exit 2, in 0008's wording
2. **AC2 Rendering**: a format renders `{track}`, `{artists}` and `{album}` as "The format" says: the version in parentheses, several artists joined with `, `, an absent album as an empty text, a placeholder used twice, text and `\n` around placeholders kept
3. **AC3 Unknown placeholders**: a `{name}` other than the three, in `summary` or `body`, is an `app.toml` error with exit 2 that names the placeholder; an unclosed `{` is kept as text
4. **AC4 The request**: the `Notify` arguments are those of "What the server receives" for each combination of a cover or none, `notify_transient` on and off, and `notify_timeout_in_secs` `0` and `7` (`-1` and `7000`)
5. **AC5 Which tracks notify**: from the tracks that start, in order: the first notifies; a different track notifies; the same track ID again does not, after a stop either; `A`, `B`, `A` gives three
6. **AC6 Markup**: with `body-markup` among the server's capabilities, `&`, `<` and `>` in the body are escaped and the summary is not; without it the body is sent as it is
7. **AC7 Replacing**: the second notification carries the ID the server returned for the first as `replaces_id`, and the third the second's; the first carries `0`
8. **AC8 The cover**: a cached cover is sent at once; one that appears within 2 s is sent when it appears; after 2 s the notification goes out without `image-path` and is not sent again; a track without a cover, or a player without a cover cache, does not wait
9. **AC9 The latest wins**: a track that starts while a notification waits for its cover, or while a call is in flight, is the next one sent, and tracks that started between are never sent
10. **AC10 Real sequences**: driven through `PlayerRuntime` with a fake engine, a started track, a gapless transition and a skip each hand the notifier the track that started, with its metadata; skipping past tracks that never start, pause, resume, seek and a device move hand it nothing
11. **AC11 Failures**: a call that fails, a server that is absent and a call past 5 s each log one `warn` line and send nothing to the player's message; a later failure logs nothing until a success; a success re-arms it; the next track is tried after a failure; the capabilities are asked again after a failure
12. **AC12 Never blocks**: `PlayerRuntime::handle` returns while a notification waits for a cover or for the server
13. **AC13 Every player**: the settings reach standalone, the daemon and `play`, which start the notifier, and an attached TUI does not. With `enable_notify = false` no notifier is started. `play` of one track ID asks for the track's metadata when notifications are on, as it does for a hook (0025 AC13)
14. **AC14 No bus**: with no session bus the player starts, plays and logs one `info` line; nothing is retried
15. **AC15 A real bus**: on a private `dbus-daemon` with a recording notification server, the daemon playing two tracks sends two `Notify` calls with the expected arguments, the second replacing the first; with no server on that bus the daemon plays on and logs one `warn` line
16. **AC16 Docs**: `docs/config.md`, `README.md` and their zh-TW copies describe the four settings, the placeholders, the cover and what happens without a server; `examples/app.toml` shows the four settings with their defaults; `CLAUDE.md` has the feature's entry and `TIDAL_PLAYER_ENABLE_NOTIFY=off` among the hands-on check's isolation settings

## Edge cases & errors

| Case | Behaviour |
|---|---|
| A track with no album | `{album}` is empty; the default body is empty and the notification has a summary only |
| A track whose metadata is unknown (`play` of an ID whose metadata request failed) | No notification: there is nothing to show |
| Fast skipping | Only a track that starts is a candidate (0025 decision 1), and the latest one replaces any that waits |
| The cover arrives after 2 s | It is in the cache for the next time the track plays; this notification stays as sent |
| The server forgot the replaced ID (the notification expired or was dismissed) | The server's business: the specification makes it a new notification |
| The notification server restarts | The next call fails or succeeds on the new one; the capabilities are asked again after a failure |
| The server hangs | The call is given up after 5 s; tracks that start meanwhile keep only the latest |
| The player shuts down | The last notification is left to expire; a waiting one is dropped |
| A title with a newline, or longer than the server shows | Sent as it is; the server truncates |
| `app.toml` changed while running | Read once at start, like every setting |
| The integration tests | They already run players on a private or absent bus (`DBUS_SESSION_BUS_ADDRESS`), so no test notifies the developer's desktop |

## Test plan

| AC | Test | Asserts | Expected red failure |
|---|---|---|---|
| AC1 | `crates/app/src/config.rs` :: `ac1_notify_settings` (table) + `crates/app/src/config.rs` :: `ac1_notify_precedence` (table) | parsed values, defaults, each error's wording; the variable over the file | unknown key `enable_notify` |
| AC2 | `crates/core/src/notify.rs` :: `ac2_render` (table) | format and track → text | the stub returns the format unrendered |
| AC3 | `crates/core/src/notify.rs` :: `ac3_unknown_placeholders` (table) + `crates/app/src/config.rs` :: `ac3_notify_format_errors` | the parser's error names the placeholder; `app.toml`'s wording | the stub accepts every format |
| AC4 | `crates/core/src/notify.rs` :: `ac4_request` (table) | the arguments per combination | the stub sends no hints and timeout `0` |
| AC5 | `crates/core/src/notify.rs` :: `ac5_which_tracks_notify` (table) | started track IDs → which notify | the stub notifies every start |
| AC6 | `crates/core/src/notify.rs` :: `ac6_markup` (table) | escaped body with the capability, raw without, summary raw | the stub never escapes |
| AC7 | `crates/app/src/player/notify.rs` :: `ac7_replaces_the_previous` (fake server) | `replaces_id` of three notifications | every call carries `0` |
| AC8 | `crates/app/src/player/notify.rs` :: `ac8_cover` (table, temp dir, short wait) | cached, late within the wait, too late, no cover, no cache | the stub never waits and never sets `image-path` |
| AC9 | `crates/app/src/player/notify.rs` :: `ac9_latest_wins` (fake server held on a barrier) | the calls made for a burst of starts | every start is sent |
| AC10 | `crates/app/src/player/runtime.rs` :: `ac10_notifier_gets_started_tracks` (table) | recorded tracks for each sequence | nothing recorded |
| AC11 | `crates/app/src/player/notify.rs` :: `ac11_failures` (table, fake server) | log lines and their re-arming; no `RuntimeInput`; capabilities asked again | no line logged |
| AC12 | `crates/app/src/player/notify.rs` :: `ac12_never_blocks` | a server held on a barrier; `send` returns at once | the send waits |
| AC13 | `crates/app/src/config.rs` :: `ac13_notify_reaches_the_settings` + `crates/app/src/player/notify.rs` :: `ac13_off_starts_nothing` + `crates/app/src/cli/play.rs` :: `ac13_play_asks_for_the_notifications_metadata` + hands-on check | the settings all three players start from; `start` gives no notifier when off; one track ID is fetched when on; the check below runs it with a TUI attached, since `main` starts the players and has no seam | settings carry no notify fields; a notifier is started; the track stays nameless |
| AC14 | `crates/app/tests/notify.rs` :: `ac14_no_bus` | a daemon on an absent bus serves a client, and its log has the one line | the line is not found |
| AC15 | `crates/app/tests/notify.rs` :: `ac15_real_bus` + `crates/app/tests/notify.rs` :: `ac15_no_server` | the recorded calls; the `warn` line | no call recorded |
| AC16 | `crates/app/tests/docs.rs` :: `ac16_notify_documented` + `crates/app/tests/examples.rs` (the example parses) + reviewed at acceptance | the four settings appear in `docs/config.md`; prose is read by hand | the settings are not found |

Checked by hand at acceptance, against the live API and the maintainer's notification server (mako): the release build as a daemon on its own runtime and state directories, notifications on. Play a track, skip to the next, skip quickly through five, let a track end gaplessly, set repeat-one for a loop; `makoctl list` shows one notification at a time, with the cover, the summary and the body, and `makoctl history` the replaced ones only when `notify_transient` is off. A title with `&` shows whole. With a TUI attached there is still one notification per track; a standalone TUI and `play` of one track ID notify too. Then with mako stopped: the player plays on and the log has one `warn` line. How the notification looks (the cover's size, where it sits) is the server's and is for the maintainer to judge. The TUI shows nothing new, so the PR has no screenshots section.

## Crate placement

- `tidal-player-core`, new `notify` module: `NotifyFormat` (the parsed `summary` and `body`, with the placeholder check), rendering against a `Track`, `NotifyTracker` (which started tracks notify), the markup escape, and `Request`: the `Notify` arguments built from the rendered texts, the cover's path, the settings and the previous ID. All pure. It is in `core` because the future `playback_format` renders the same placeholders in the TUI
- `tidal-player` (`crates/app`), new `player/notify.rs`: the worker thread (one slot holding the latest track, the wait for the cover, the previous ID, the failure log), behind two seams: `Server` (capabilities and `Notify`; the zbus implementation, and a fake) and the cover's path (the cache directory, or none)
- `crates/app/src/player/runtime.rs`: `with_notifier`, as `with_hook`: `handle` hands the notifier the current track when `Handled.started` is set. `Command`, `Event` and the socket protocol do not change
- `crates/app/src/config.rs` and `settings.rs`: the four settings; `main.rs` starts the notifier for the three players; `cli/play.rs`: the metadata request

## Facts vs. assumptions

Verified on 2026-11-16:

- spotify-player (`master`, `spotify_player/src/config/mod.rs` and `client/mod.rs`): the four settings and their defaults (`enable_notify = true`, `notify_timeout_in_secs = 0`, `notify_transient = false`, the format above); the placeholders are `{track}`, `{artists}` and `{album}`, and any other `{…}` is silently dropped; a timeout is set only when the setting is above `0`; the cover is the cached file's path, given as the icon when the file exists; it notifies after the cover was fetched; it never replaces a notification and does not escape the body
- In the code at `b0093e9`: `Handled.started` is set whenever a track starts, the one a skip led to included, and never for a track that only loads; the cover worker runs whenever `max_cover_arts > 0`, MPRIS on or off, and wants a cover when its entry becomes current; a cached cover is `<cache dir>/covers/<cover>.jpg` (`cover_cache::cached_file`); `zbus` is a dependency with its blocking API; the integration tests run players with `DBUS_SESSION_BUS_ADDRESS` set to an absent or private bus
- On the maintainer's machine: the notification server is mako (specification 1.2), with the capabilities `body`, `body-markup`, `actions` and `icon-static`

Assumptions, to confirm while implementing; a wrong one comes back to this spec:

- mako, dunst, GNOME Shell and KDE Plasma show the `image-path` hint given as an absolute path (the specification allows a `file://` URI or a path). mako is checked by hand at acceptance; if it needs the URI or `app_icon`, "What the server receives" changes
- `play` of one track ID with notifications on can reuse the hook's metadata path (`hooked`) as it is
- A `dbus-daemon` test can host a recording notification server the way `tests/mpris.rs` hosts its listeners

## Decisions

1. **On by default.** *Answered (the user, 2026-11-16): on, as spotify-player.* Off by default lost: an upgrade would change nothing, but the feature would stay unknown
2. **Every player notifies.** *Answered (the user, 2026-11-16): standalone, daemon and `play`, as MPRIS and the event hook.* Leaving the standalone TUI out lost: the TUI is often on another workspace
3. **Wait up to 2 s for the cover.** *Answered (the user, 2026-11-16).* Sending at once and updating when the cover arrives lost: some servers show an update as a second pop-up. Sending at once without a cover lost: a first play of an album would rarely have one
4. **The same track again does not notify.** *Answered (the user, 2026-11-16).* Notifying every start, as the hook's `Changed`, lost: repeat-one would notify every few minutes
5. **The runtime, not a hub peer** (tech-lead, as 0025 decision 2). A hub peer sees `current` move as soon as a track is asked for, so fast skipping would notify tracks nobody heard
6. **An unknown placeholder is an error** (tech-lead). spotify-player drops it silently; here a typo in `app.toml` is found at start, as every other setting's
7. **Own bus connection, own worker** (tech-lead). Sharing MPRIS's connection would tie notifications to `mpris = true`
8. **Failures go to the log, not the message row** (tech-lead; the issue: "nothing happens, no error"). A desktop without a notification server is normal, unlike a hook that was set and fails
9. **`play` of one track ID fetches its metadata when notifications are on** (tech-lead). Without it the notification would have no title; the alternative, no notification for such a track, would make `play` the one player that stays silent by default

## Out of scope

- `playback_format` for the playback window (a later spec; the placeholders are shared)
- Actions on the notification (next, pause), a `desktop-entry` hint, urgency, sounds
- `notify_streaming_only` (spotify-player's: there is no Spotify Connect here)
- Notifications for anything but a started track (pause, errors, login)
- Closing the notification when the player stops or shuts down
- macOS and Windows notification systems
- Making the 2 s cover wait or the 5 s call limit configurable

## Implementation notes

- **The title helper**: `Track::display_title()` (`tidal-player-core::track`) is the title with its version, used by `{track}` and by the event hook's `TIDAL_PLAYER_TRACK_TITLE`
- **Parsing the format**: a placeholder is a `{` followed by a `}` with no `{` between; its name must be exactly `track`, `artists` or `album` (`{ track }` and `{Track}` are unknown, `{}` too). A `{` followed by another `{` or the end is text. The `app.toml` errors are `invalid notify_format.summary: unknown placeholder {title}: expected {track}, {artists} or {album}`, `invalid notify_format.summary: expected a non-empty string, got ""`, `invalid notify_format.body: expected a string, got 1` and `unknown setting "notify_format.icon"`
- **Escaping**: with `body-markup`, the whole rendered body is escaped, the format's own text included, so a body cannot carry markup of its own
- **Unknown metadata**: a started track whose title is empty (`play` of one track ID without its metadata request) is not handed to the notifier and does not count as the last notified track
- **The 5 s limit** is zbus's method timeout on the notifier's own connection (`Builder::method_timeout`); a timed-out call is `CallError::TimedOut`. When `GetCapabilities` fails, that track's `Notify` is not sent: the failure counts as the notification's, is logged as one, and the next track asks again
- **The log**: the lines are `No session bus, desktop notifications are off: <zbus error>` (`info`) and `Desktop notification failed: <server error>` or `… : no answer within 5 s` (`warn`), through `tracing` under `tidal_player::player::notify`
- **The cover's path** is made absolute with `std::path::absolute` (no symlinks resolved), so a relative `TIDAL_PLAYER_CACHE_DIR` still gives an absolute `image-path`
- **The connection** is opened on the worker's thread, so a slow bus delays no player start. Shutting down drops a waiting notification; a call in flight ends on its own
- **Tests outside this spec**: `tests/mpris.rs` sets `TIDAL_PLAYER_ENABLE_NOTIFY=off` (it counts MPRIS's connections to the bus), and the two `tests/cli.rs` tests of `play` of one track ID without a metadata request (0003's errors) set it too, since notifications on make `play` fetch the track first (AC13)
- **End to end**: `tests/notify.rs` plays a one-second FLAC from the mock API to ALSA's `null` PCM (silent, no card), with a recording `org.freedesktop.Notifications` served by the test on a private `dbus-daemon`
