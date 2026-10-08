# 0009 — Persistence: resume where you left off

- **Status**: approved (2026-10-08: decisions 1, 2, 4–9 as proposed; decision 3 changed by the user to "also cache library pages", specified under "The library cache")
- **Owner**: tech-lead (primary session)
- **Depends on**: 0002 (implemented: the state directory, `logout`), 0004 (implemented: the player state), 0005 (implemented: one player per user, clients own nothing on disk), 0008 (implemented: `app.toml` and its precedence)
- **User docs**: [`docs/playback.md`](../playback.md) gains "Resuming the last session"; [`docs/daemon.md`](../daemon.md) (a restarted daemon resumes), [`docs/config.md`](../config.md) (`remember_playback`, `library_cache_secs`, `RefreshPage`), [`docs/tui.md`](../tui.md) (`R` refreshes a page), [`docs/login.md`](../login.md) (`logout` also forgets it) and their zh-TW copies (AC20)

## Context

Every run starts empty: queue, position, shuffle, repeat, autoplay and volume are lost when the player exits, so `systemctl --user restart tidal-player`, a reboot or quitting the standalone TUI throws away what was playing. 0003, 0004, 0005, 0006 and 0007 each deferred "remembering" to this spec, and 0001 listed it as "Persistence: last session, volume, device, metadata cache".

This spec makes the **player process** remember its playback state and restore it, stopped at the position, at start, and keep an on-disk **cache of library pages** so pages open without waiting for Tidal. It decides the rest of 0001's list (the device, the TUI's own state) explicitly: see "Decisions".

### What tidalt did

Read from tidalt's `internal/store/store.go`, `internal/ui/model.go`, `keys.go`, `library_sections.go` and commits `0ab2897`, `f904af2`, `8ee1989`, `d3827b7`, `c3a59c3`, `1d3faed`, `a8b5e7e`:

- One **bbolt** database, `~/.local/share/tidalt/tidal-cache.db`, buckets `Tracks`, `Settings`, `Cache`, opened with an exclusive lock and a 5 s timeout. Keys: `volume`, `device`, `theme`, the playlist (the queue's tracks), the last track ID, the last position, the recently-played history, every track seen (`CacheTrack`) and every plain-text search's results (`search:<query>`)
- All of it was written **from the UI model**, mostly as `_ = m.store.Save…(…)`, so every write error was dropped
- The position was written on **every one-second tick** while playing, and at quit
- At start the playlist was loaded **asynchronously**; the favorites load that ran at the same time replaced it until `f904af2` added a `playlistRestored` flag. The last track was found again by **track ID** (the first match), only the cursor was moved there, and the saved position was applied as a seek once the next track **started** (`restorePosition`)
- The restored playlist was re-shuffled (`applyShuffle`) instead of keeping its play order
- Search results were served from the cache **forever** with no expiry; `CacheTrack` was written on every play and never read

### What went wrong there, and the criterion that covers it here

1. **Two processes fought over one database** (`1d3faed`, `a8b5e7e`, bolt lock timeouts in tidalt #8): parent and clients opened the same store → only the player process reads or writes the file; a client opens nothing on disk (0005 AC14, extended here: AC10)
2. **Write errors were invisible** (`_ =` on every save): a full disk or a read-only directory lost the session silently → a failed write is logged and shown once in the player's message; playback goes on (AC7)
3. **A crash or kill mid-write could corrupt the store** (bbolt is crash-safe; a hand-rolled file would not be) → the file is replaced atomically (write a temporary file, `fsync`, rename); an unreadable or corrupt file never stops the player: it starts empty, says so, and keeps the bad file aside (AC5, AC6)
4. **The restore raced the start-up loads** (`f904af2`): the favorites replaced the restored queue → the state is read **synchronously, before the player handles its first input**; the first `Welcome` already carries it (AC8)
5. **The last track was found by ID** (the first entry with that track, wrong with duplicates) **and only the cursor moved** → the queue is saved with its entry IDs, the current entry ID and the play order, and restored exactly (AC2)
6. **Shuffle was re-applied on restore**, so the next track after a restart differed from the one before it → both orders are saved; nothing is re-shuffled (AC2)
7. **The saved position became a seek after the next track started** (`restorePosition`, and `c3a59c3`: a stale restored position over a zero duration drew a 100 % bar), so audio played from 0:00 before jumping → the player restores **stopped on the current entry at the saved position**; play starts the stream at that position (0004's `start_at`), with no seek (AC3)
8. **A write per second while playing** → position-only changes are saved at most every 30 s while playing, and at once on pause, stop, seek, track change and exit; every other change is coalesced within 2 s (AC4)
9. **Caches that were never invalidated or never read** (search results served forever, `CacheTrack` written and never read) → library pages are cached with an expiry, every edit made through this player drops the entries it changes before it is answered, `R` refetches a page, and search results are never cached (AC15–AC18). Restoring the queue does not use the cache: the queue stores its own tracks' metadata, so it works offline
10. **One account's data shown to another** (tidalt's store was not keyed by user) → cache entries carry the user ID and `logout` clears the cache (AC17, AC13)

## Behaviour

### What is remembered

The **playback state**, by the player process (standalone TUI or daemon), in one file:

| Remembered | Restored as |
|---|---|
| The queue: every entry with its entry ID, its track metadata (as queued: title, version, artists, album, duration, streamable) and its `suggested` mark | The same entries with the same IDs; new entries get IDs above the highest restored one |
| The original order and the play order | Both, unchanged (no re-shuffle) |
| The current entry | The same entry, **stopped** (`■`) |
| The position in the current entry | The position shown; play starts there |
| Shuffle, repeat, autoplay | The same modes (autoplay: see "Precedence") |
| Volume and mute | The same volume and mute |

Not remembered here: the playing/paused state (a restored player is always stopped, so a daemon started at boot is silent), the player's message, the now-playing details (quality, output: they come from the next start), preloads, failures, pending autoplay suggestions, the login session (0002 keeps it), anything a client holds (page history, cursors, search query: decision 4), the output device (decision 2).

### Starting from the remembered state

- The player reads the file **once, before it handles its first input** (and before a daemon binds its socket), so the first `Welcome` already carries the restored state. Nothing is fetched to restore it, and no stream is resolved and no device is opened until the user plays
- `space` (`TogglePause` on a stopped player with a current entry, 0004) plays the current entry **from the restored position**; `Next`, `Previous`, `PlayEntry` behave as on any stopped player
- A restored position at or past the entry's known duration is restored as `0:00`. A current entry ID that is not in the queue, or a play order that is not a permutation of the entries, is repaired: no current entry and position `0:00`; the play order rebuilt from the original order with shuffle **off**. Repairs are silent (the file was written by us; a hand-edited file gets what it can)
- Volume above 100 is clamped to 100
- `tidal-player ITEM…` with no player running restores first, then handles its `Open` as today (0005): the items replace the queue and play; shuffle, repeat, autoplay and volume stay restored
- `tidal-player play ITEM…` (the headless foreground command, 0004) neither reads nor writes the file: it plays exactly what it was given, with its own flags

### Saving

- **When**: a change to anything in "What is remembered" other than the position is saved within **2 s** (changes in between are coalesced into one write: mashing `+` writes once). While **playing**, a change of the position alone is saved at most every **30 s**. A pause, a stop, a seek, a track change and the player's **exit** (`Shutdown` from `q`, `daemon stop`, `SIGTERM`/`SIGINT`) save at once with the exact position. Nothing is written when nothing remembered changed
- **How**: the whole file is written to `playback.json.tmp` in the same directory with mode `0600`, `fsync`ed, then renamed over `playback.json`, so a crash leaves the old file or the new one, never half of one. The directory is created (`0700`) if missing
- **Crash or `SIGKILL`**: at most the last 30 s of position and 2 s of changes are lost
- **A write that fails** (disk full, read-only directory, permissions): logged, and the player's message (0004 "Failures") says `Could not save the playback state: <error>` once until a write succeeds again; playback is not affected. The next change retries

### Where

`playback.json` in the **state directory** (0002: `$TIDAL_PLAYER_STATE_DIR`, else `$XDG_STATE_HOME/tidal-player`, else `~/.local/state/tidal-player`), next to `session.age`. It is our data, not configuration: it is never read from the config directory and never documented as something to edit.

```json
{
  "version": 1,
  "entries": [ { "id": 7, "track": { "id": 77640617, "title": "…", … }, "suggested": false } ],
  "play_order": [7, 3, 9],
  "current": 7,
  "position_ms": 83000,
  "shuffle": true,
  "repeat": "Queue",
  "autoplay": false,
  "volume": 80,
  "muted": false
}
```

### A missing, unreadable or corrupt file

| File | Start | Message |
|---|---|---|
| Missing (first run, after `logout`) | Empty, as today | none |
| Unreadable (permissions, a directory) | Empty | `Could not restore the playback state: <path>: <io error>` |
| Not valid JSON, missing fields, or a `version` other than 1 | Empty; the file is renamed to `playback.json.bad` (replacing an older `.bad`) so the next save does not destroy the evidence | `Could not restore the playback state (kept as playback.json.bad): <reason>` |

The message is the player's (0004), so every client shows it until the first track starts. Never an exit code: a broken state file must not keep a daemon from starting (unlike a broken `app.toml`, which the user wrote, 0008 decision 2).

### Turning it off

`remember_playback` in `app.toml` (`true` by default), or `TIDAL_PLAYER_REMEMBER_PLAYBACK` (`on`/`off`), with 0008's precedence (environment > file > default). When off, the player neither reads nor writes `playback.json`, and leaves an existing one as it is.

### Precedence of autoplay

0004 has `autoplay` as a start setting (`--autoplay`, `TIDAL_PLAYER_AUTOPLAY`, `app.toml`). With a remembered state: **flag > environment > remembered > `app.toml` > default**: the file's `autoplay` is the default for a fresh queue, the remembered mode beats it, and an explicit per-run choice beats both (decision 5). No other setting overlaps a remembered value: volume, shuffle and repeat have no setting.

### `logout`

`tidal-player logout` also deletes `playback.json` (and `.bad`), so the next account does not inherit the last one's queue; its output and exit code are unchanged (0002). A player that is running keeps its queue and writes the file again on its next save (documented; `logout` does not stop the daemon, 0002).

### The library cache

The player (the process that answers `Library` requests, 0006) keeps the answers to library page and list requests on disk and answers from there while they are fresh. Clients are unchanged: they still send a request whenever a page is opened (0006 AC9); only where the answer comes from changes.

- **Cached**: the answers to `Page` and `More` for `Library`, `FavoriteTracks`, `Album`, `Playlist` and `Artist` pages and their lists (including the artist's *All tracks*). **Not cached**: searches (`Page(Search)` and the four `Search*` lists: tidalt's search cache was the stalest part), `IsFavorite`, and every edit
- **Fresh** means younger than `library_cache_secs` (`app.toml`, integer 0–604 800, default `3600`; `TIDAL_PLAYER_LIBRARY_CACHE_SECS`; `0` turns the cache off: nothing is read or written, and existing entries are left alone). A fresh entry is the answer, with no request to Tidal; anything else is fetched, answered, and stored
- **One list, one generation**: fetching a page fresh (a miss, an expired entry, `R`) drops the cached later pages (`More`) of every list on it, so a list is never assembled from pages fetched at different times. A `More` with no fresh entry is fetched and stored under the page's generation
- **Edits drop what they change**, before the edit's reply is sent, so the next open fetches fresh:

  | Edit (through this player, any client) | Dropped |
  |---|---|
  | Add or remove a favorite track | `FavoriteTracks` |
  | Add or remove a favorite album / artist / playlist | `Library` (and its `More` pages) |
  | Add to or remove from a playlist; a `412` on a removal | that `Playlist` page; `Library` |
  | Create or delete a playlist | `Library`; a deleted playlist's page |

  Edits made elsewhere (Tidal's apps) show up when the entry expires or on `R`
- **Refresh** (`RefreshPage`*, default key `R`, acts on pages; a new row in 0008's command table and in the keys help's `Pages` section): sends `Library { Refresh(<the top page's request>) }`; the player fetches the page fresh, replaces its entry (dropping its lists' `More` pages, as above) and answers as for `Page`; the client shows it as a new open of that page, keeping the page in the history. On the queue page and the search page `R` does nothing (nothing cached there)
- **Where**: `$TIDAL_PLAYER_CACHE_DIR`, else `$XDG_CACHE_HOME/tidal-player`, else `~/.cache/tidal-player` (spotify-player's place for its cache), in `library/`: one JSON file per entry, named by a hash of the request, holding a `version`, the user ID, the time it was fetched, the request and the answer. Written as `playback.json` is (temp file, rename, `0600`, directory `0700`)
- **Bounded**: at start the player deletes entries older than 7 days and, while the directory is over **100 MB**, the oldest entries first. An entry for another user ID, of another `version`, or that does not parse is a miss and is deleted. A cache that cannot be read or written is never an error the user sees: the request is answered from Tidal as without a cache, and the failure is logged once
- `logout` deletes the cache directory's `library/` with the playback state


Core (`tidal_player_core::player`, pure; the saved type derives `Serialize`/`Deserialize`, `serde_json` only in tests):

- **AC1** — `PlayerState::saved() -> SavedPlayback` captures exactly the remembered fields (table over states: empty; stopped; playing with shuffle on; paused mid-track; loading with a start position; muted at 40 %; autoplay entries marked suggested); it does not change with the message, now-playing details, preloads or failures. `SavedPlayback` survives a JSON round-trip unchanged
- **AC2** — `PlayerState::restore(config, seed, saved)` (table): the snapshot equals the saved queue in play order, the same entry IDs, current entry, position, modes, volume and mute, state `Stopped`; the next `AddToQueue` gets an ID above the highest restored one; a position at or past the known duration → `0:00`; an unknown current ID → no current entry, `0:00`; a play order that is not a permutation → original order, shuffle off; volume 150 → 100; restoring an empty saved state equals `PlayerState::new`. `restore(…, s.saved()) .saved() == s.saved()` for every row of AC1
- **AC3** — Playing a restored state: `TogglePause` emits `Resolve` and then `EnginePlay { start_at: <restored position> }` (no `EngineSeek`); `Next`, `Previous` (restart threshold against the restored position) and `ToggleShuffle` behave as on a state that reached the same point through commands (compared effect by effect)

Saving (`tidal-player::persist`, pure, a fake clock):

- **AC4** — The save schedule (table of input sequences over time → writes): a volume change → one write at +2 s; ten volume changes within 1 s → one write; a queue edit, a mode toggle → one write within 2 s; position only, playing → a write every 30 s and not between; pause, stop, seek, a track change → a write at once; nothing changed → none; exit → a write at once with the exact position, after which nothing more is written
- **AC5** — Writing (temp dir): the file is `playback.json` in the state directory with mode `0600`, its directory created `0700`; it is written through `playback.json.tmp` and a rename (a fake filesystem seam fails between the write and the rename: the old file is intact)

Reading (`tidal-player::persist`, temp dir):

- **AC6** — Loading (table): missing → empty, no message; a directory or mode `000` → empty, the unreadable message; truncated JSON, a missing field, `"version": 2` → empty, the file renamed to `playback.json.bad` (an older `.bad` replaced), the corrupt message naming the reason; a valid file → its `SavedPlayback`. Each message as under "A missing, unreadable or corrupt file"
- **AC7** — A failed write (fake filesystem returning `ENOSPC`, then success): the player's message becomes `Could not save the playback state: …` once (no repeat on the next failure), playback effects are unaffected, and the next successful write clears the failing flag (the message itself stays until the next track starts, as 0004's messages do)

Runtime and modes (`tidal-player`):

- **AC8** — Daemon restart (0005's daemon harness, mock API, temp state dir): load a queue, shuffle on, repeat queue, volume 70, seek to 1:23, `daemon stop`; start the daemon again: the first `Welcome` of a new client equals the saved snapshot with state `Stopped` (position within the harness's tolerance), and the mock API received no request and the fake engine no command before the client's `TogglePause`; after it, the engine's `Play` starts at the saved position
- **AC9** — Standalone and one-shot: a standalone player (the in-process harness of 0004) restores before its first frame; `tidal-player ITEM` over a remembered state replaces the queue and keeps the restored modes and volume; `tidal-player play ITEM` leaves an existing `playback.json` byte-for-byte unchanged and does not create one in an empty state dir
- **AC10** — Only the player touches the file: 0005's `ac14` test (a client's state dir stays empty) stays green unchanged, with a new row in which the player's state dir holds a `playback.json` that the client run leaves byte-for-byte unchanged
- **AC11** — `remember_playback` (0008's `app.toml` and precedence tables gain its rows: `true`, `false`, wrong type, `TIDAL_PLAYER_REMEMBER_PLAYBACK=off` over a file's `true`, an invalid variable is an error): off → no read, no write, an existing file untouched
- **AC12** — Autoplay precedence (table): default; `app.toml`; remembered over `app.toml`; environment over remembered; `--autoplay` over all
- **AC13** — `logout` deletes `playback.json`, `playback.json.bad` and the cache's `library/` and prints what it printed before (0002's `logout` tests unchanged, one new row)

Library cache (`tidal-player::library_cache`, pure decisions over a fake clock and a fake filesystem seam; the runtime test with 0006's mock API):

- **AC15** — What is cached (table over every `LibraryRequest` variant): `Page`/`More` of the five page kinds and their lists are cacheable; searches, `IsFavorite` and every edit are not; with `library_cache_secs = 0` nothing is
- **AC16** — Hits and misses (table, fake clock): a fresh entry answers with no API request (the mock's `expect(0)`); at `library_cache_secs` it is expired and fetched; a miss is fetched and stored; fetching a page fresh drops its lists' `More` entries, and a `More` after it is fetched; `Refresh` always fetches and replaces; the cache directory resolves as under "Where" (table: variable, empty variable, `XDG_CACHE_HOME`, home)
- **AC17** — Invalidation (table, one row per edit in the table above, through the player runtime with the mock API): after the edit's `Done`, opening each dropped page requests Tidal again and every other cached page does not; a `412` drops the playlist. Entries of another user ID, another `version` or unparsable are misses and are deleted
- **AC18** — Bounds and failures: entries older than 7 days are deleted at start; over 100 MB the oldest go first until under it; an unwritable cache directory answers every request from Tidal, logs once, and puts nothing in the player's message
- **AC19** — `RefreshPage` (model, table): `R` on the library, favorite tracks, album, playlist and artist pages emits `Library { Refresh(…) }` for that page and shows `Loading…` keeping the page in the history; on the queue and search pages it emits nothing; 0008's default-keymap and keys-help tests gain its row

TUI and docs:

- **AC20** — Rendering (`insta`, 80×24, reviewed by eye, + `contains`): a restored state shows `■`, the current track's title, `1:23 / 4:56` and the progress bar at that point, and the queue with the current entry marked; the corrupt-file message on the message row. `docs/playback.md` "Resuming the last session" (what is remembered, when it is saved, where, how to turn it off, what a corrupt file does), `docs/tui.md` (the library cache in a few lines, and `R`), `docs/daemon.md`, `docs/config.md` (`remember_playback`, `library_cache_secs`, `RefreshPage`), `docs/login.md`, their zh-TW copies, `README.md`/`README.zh-TW.md` and `CLAUDE.md` "Features" say so; the "until 0009" notes in 0004, 0005 and 0006–0008 point here. Tests check that `remember_playback` and `library_cache_secs` appear in `docs/config.md` and `RefreshPage` with its key (0008 AC17's test, new rows); `examples/app.toml` and `examples/keymap.toml` gain them (0008 AC18's tests stay green)

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| First run, no state directory | Empty start; the directory is created on the first save |
| The restored current track is no longer streamable or was removed from Tidal | Restored as saved; on play it fails as any track does (0004 "Failures": skipped with its message) |
| Restored while logged out (`LoginRequired`) | Restored as usual; playing fails with 0002's message until `login` |
| A 10 000-entry queue | A file of a few MB; written at most every 2 s on edits and 30 s while playing |
| Two players | Impossible (0005: one player per user); `play` writes nothing, so it cannot clobber a daemon's file |
| `logout` while a player runs | The file is deleted; the running player writes it again on its next save |
| `TIDAL_PLAYER_STATE_DIR` changed between runs | Each directory has its own file; nothing is migrated |
| State dir on a read-only filesystem | Starts (empty or restored); the save-failure message once; plays normally |
| A restored position inside the last seconds of the track | Restored as is; play finishes the track and auto-advances (0004) |
| Clock jumps (suspend/resume) | The 30 s schedule uses a monotonic clock; a resume from suspend writes at the next position change past 30 s |
| `SIGKILL`, power loss | The last complete file (AC5); at most 30 s of position lost |
| A `.tmp` left by a crash | Overwritten by the next save; never read |
| A playlist edited in Tidal's app while its page is cached | The old copy until it expires or `R`; a removal from it gets `412`, which drops it and reloads (0006) |
| Two pages of one list with different `total`s | Impossible: a fresh page drops the list's later pages (one generation) |
| The cache directory is on a full disk | Answers come from Tidal; logged once; nothing shown |
| The library cache with no network | Fresh entries still answer; anything else fails as today (0006's message). Serving expired entries offline is out of scope |

## Test plan

Each test is named after its criterion (`ac4_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors), as in 0001–0008.

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/player/tests.rs` :: `ac1_saved_fields` (table), `ac1_saved_round_trip` | captured fields; JSON round-trip | stub `saved()` returns an empty `SavedPlayback` |
| AC2 | `crates/core/src/player/tests.rs` :: `ac2_restore` (table), `ac2_restore_saved_identity` | restored snapshot, IDs, repairs, clamps; restore∘saved identity | stub `restore` returns `PlayerState::new` |
| AC3 | `crates/core/src/player/tests.rs` :: `ac3_play_restored`, `ac3_restored_equals_reached` (table) | `EnginePlay` at the position, no seek; same effects as a reached state | stub restore drops the position, so `start_at` is 0 |
| AC4 | `crates/app/src/persist.rs` :: `ac4_save_schedule` (table, fake clock) | write times per input sequence | stub scheduler writes on every change |
| AC5 | `crates/app/src/persist.rs` :: `ac5_write_file`, `ac5_write_is_atomic` | path, modes, temp + rename, old file intact on a failed rename | stub writes `playback.json` in place |
| AC6 | `crates/app/src/persist.rs` :: `ac6_load` (table) | result, `.bad` rename, messages | stub returns empty and renames nothing |
| AC7 | `crates/app/src/persist.rs` :: `ac7_write_failure` | message once, retried, playback unaffected | stub ignores write errors (tidalt's `_ =`) |
| AC8 | `crates/app/tests/daemon.rs` :: `ac8_daemon_resumes` | `Welcome` after restart, no requests before play, `Play` at the position | stub runtime never loads the file |
| AC9 | `crates/app/src/player_runtime.rs` :: `ac9_standalone_restores`, `ac9_items_replace_queue`; `crates/app/tests/cli.rs` :: `ac9_play_leaves_state_alone` | restore before the first frame; items replace; `play` writes nothing | stub restores nothing; stub `play` saves on exit |
| AC10 | `crates/app/tests/daemon.rs` :: the 0005 `ac14` test (unchanged) + `ac10_client_leaves_playback_file` | file bytes unchanged by a client | — the existing test is the safety net; the new row fails against a stub client that saves |
| AC11 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows) + `crates/app/src/persist.rs` :: `ac11_remember_off` | parsing, precedence, no I/O when off | stub ignores the setting |
| AC12 | `crates/app/src/play.rs` :: `ac12_autoplay_precedence` (table) | winning autoplay per source | stub lets `app.toml` beat the remembered mode |
| AC13 | `crates/app/tests/cli.rs` :: the 0002 `logout` tests (unchanged) + `ac13_logout_forgets_playback` | files gone, output unchanged | stub `logout` leaves the files |
| AC15 | `crates/app/src/library_cache.rs` :: `ac15_cacheable` (table) | cacheable per request | stub caches nothing |
| AC16 | `crates/app/src/library_cache.rs` :: `ac16_hits_and_misses` (table), `ac16_cache_dir` (table) | requests sent per sequence; directory | stub always misses |
| AC17 | `crates/app/src/player_runtime.rs` :: `ac17_edits_invalidate` (table, mock API), `crates/app/src/library_cache.rs` :: `ac17_foreign_entries` | refetched pages per edit; deleted entries | stub drops nothing on edits |
| AC18 | `crates/app/src/library_cache.rs` :: `ac18_prune`, `ac18_unwritable` | pruned files; answers without a cache | stub never prunes |
| AC19 | `crates/core/src/ui/tests.rs` :: `ac19_refresh_page` (table) + 0008's `ac2_build_keymap`, `ac7_help_sections` (new rows) | effects per page; keymap and help rows | stub `RefreshPage` does nothing |
| AC20 | `crates/app/src/ui.rs` :: `ac20_restored_80x24`, `ac20_restore_message_80x24`; `crates/app/tests/docs.rs` :: `ac17_every_command_documented` (new row) + reviewed at acceptance | `contains` + snapshots; docs | stub snapshot has no current entry |

Checked by hand at acceptance on the user's machine (results in the PR description): `systemctl --user restart tidal-player` resumes the queue paused at the position and `space` plays from there on the DAC with no audible jump from 0:00; quitting and restarting the standalone TUI does the same; a reboot resumes with the daemon silent until played.

## Crate placement

- `tidal-player-core::player`: `SavedPlayback` (+ `SavedEntry`), `PlayerState::saved`, `PlayerState::restore` with its repairs. No I/O, no new dependency (`serde` is already there)
- `tidal-player-core`: `protocol` gains `LibraryRequest::Refresh(PageRequest)`; `ui::keymap` gains `RefreshPage` on `R`
- `tidal-player`: `library_cache` (cacheability, keys, the generation rule, invalidation per edit, pruning, the directory; behind the same filesystem seam), wired where the runtime answers `Library` requests; `library_cache_secs` in `config`; `persist` (the path, load with the `.bad` rename, the atomic write behind a small filesystem seam, the save schedule as a pure function of the previous saved state, the new one, the playing flag and a monotonic instant), wired into the player runtime (standalone and daemon; not `play`); `remember_playback` in `config`; `logout` deleting the files. `serde_json` is already a dependency of the binary
- `xtask layering`: no change

## Facts vs. assumptions

Verified (2026-10-08, from code):

- tidalt: as under "What tidalt did" (files and commits named there)
- This repo: `PlayerState` holds the queue as entries plus a play order with an entry-ID counter (`crates/core/src/player.rs`, `player/queue.rs`); `TogglePause` on a stopped player plays the current entry from `position` via `EnginePlay { start_at }`; `Track` and `QueueEntry` derive `Serialize`/`Deserialize`; the state directory and `session.age` come from `store_setup.rs`; the daemon turns `SIGTERM`/`SIGINT` into `Shutdown` (`daemon.rs`); 0005's `ac14` test asserts a client's state dir stays empty
- spotify-player (README and `docs/config.md`, master, read 2026-10-08): a cache folder (`-C/--cache-folder`, `$HOME/.cache/spotify-player`) for logs, credentials and an optional audio cache; `enable_cover_image_cache`; `device.volume` is a start setting (70); `pause_on_startup` starts paused instead of resuming the previous session (resuming is Spotify Connect's, server-side)

Assumptions, checked at acceptance: `rename(2)` within the state directory is atomic on the user's filesystem (true for every local Linux filesystem; not guaranteed on some network filesystems, which the docs mention).

## Decisions (answered by the user, 2026-10-08: 1, 2, 4–9 as proposed; 3 changed)

1. **Restore paused/stopped, never playing**: a restored player is stopped at the position, so a daemon started at boot is silent and nothing touches the network or the device until `space`. Alternative: resume playing if it was playing at exit (spotify-player's default when `pause_on_startup = false`)
2. **The output device is not remembered**: there is no device picker yet (0004 deferred it), so the device is only ever chosen by `--device`/`output_device`, and remembering it would add a second source for one setting (0008's "two sources of truth"). When a picker spec comes, its choice is remembered in this file. Alternative: remember the last device a run used and let it beat `app.toml`
3. **Library cache**: *answered (the user, 2026-10-08): cache library pages*, with an expiry: specified under "The library cache" (proposed there: one TTL `library_cache_secs`, default 1 h; edits through this player invalidate; `R` refreshes; no search cache; 100 MB / 7 days bound; in the XDG cache dir)
4. **Nothing on the client side** (page history, search query, cursors): 0005 forbids a client to open anything on disk but the socket, and an attached TUI is a client. Alternative: the player stores a per-client blob sent over the protocol
5. **Autoplay precedence**: flag > environment > remembered > `app.toml` > default (the file is the default for a fresh queue; a per-run choice still wins). Alternative: `app.toml` beats the remembered mode
6. **A corrupt file**: start empty, keep it as `playback.json.bad`, say so; never refuse to start. Alternative: refuse to start (as for `app.toml`)
7. **`logout` forgets the playback state** (the next account should not inherit the last one's queue). Alternative: keep it
8. **On by default** with `remember_playback = false` to turn it off. Alternative: off by default
9. **Save timing**: 2 s coalescing for changes, 30 s for the position while playing, immediate on pause/seek/stop/track change/exit

## Out of scope

- A device picker and remembering its choice (decision 2)
- Caching search results or images; serving expired library entries when offline
- Remembering the TUI's page history, cursors or search query (decision 4)
- A recently-played history (tidalt had one; it would be its own spec, with its page)
- Migrating anything from tidalt's `tidal-cache.db`
- MPRIS (0010), mixes and radio (0011), filter and sort (0012)
