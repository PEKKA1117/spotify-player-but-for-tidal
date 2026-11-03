# 0021 — Lyrics

- **Status**: implemented (2026-11-03; approved 2026-11-03; the manual checks under "Test plan" are run on the user's machine); API shapes verified by the live probe (2026-11-03, "Facts")
- **Owner**: tech-lead (primary session)
- **Issue**: [#54](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/54)
- **Depends on**: 0004 (implemented: the queue, the current entry, `Position` events), 0005 (implemented: clients, `Library`/`LibraryReply`, AC14), 0006 (implemented: pages, the history, failures in the page, disconnected rules), 0008 (implemented: the keymap, the keys help), 0013 (implemented: key hints), 0017 (implemented: themes, components, skipped spotify-player components)
- **User docs**: [`docs/tui.md`](../tui.md) gains a "Lyrics" section and the keys; [`docs/config.md`](../config.md) gains `LyricsPage` and the `lyrics_played`/`lyrics_playing` components and drops them from the skipped lists; and their zh-TW copies (AC12)

## Context

spotify-player has a lyrics page (`LyricsPage`, `l` and `g L`): the playing track's lyrics, with synced lyrics highlighting the current line and keeping it in view. This player skips `LyricsPage` in a copied `keymap.toml` and `lyrics_played`/`lyrics_playing` in a copied `theme.toml` (0008, 0017). This spec adds the page.

tidalt had no lyrics. Tidal serves them from `GET /v1/tracks/{id}/lyrics`, verified by the probe ("Facts"): plain text and LRC-timed lines from Musixmatch, a right-to-left flag, and `404` when a track has none.

### What could go wrong here, and the criterion that covers it

1. **Clients that hold a session**: lyrics fetched by the TUI itself would need a session in every client, against 0005 AC14 → the client asks the player with a `Library` request, as every page does, and the player fetches (decision 1; AC3, AC4, AC11)
2. **Lyrics of the wrong track**: the track changes while a request is in flight, and the late answer is shown for the new track → an answer is shown only while its track is still the page's track (AC6)
3. **A request per key press or per position event** → the client asks once per track change while the page is on top, and the player keeps the answers of recent tracks, "no lyrics" included (decision 2; AC4, AC6)
4. **A `404` shown as a failure**: Tidal answers `404` for a track without lyrics (an instrumental) → `No lyrics for this track`, not an error (AC3)
5. **LRC variations** (no space after the stamp, `[mm:ss]` or `[mm:ss.xxx]`, several stamps on one line, metadata tags, CRLF, blank timed lines) → one pure, table-tested parser (AC2)
6. **The highlight drifting from the music**: the current line is computed from the client's position, which the player sends every 200 ms and on every seek → a pure function of lines and position (AC2, AC7)
7. **Right-to-left text, long lines, CJK, tiny terminals** → right-aligned when Tidal marks the lyrics right-to-left, wrapped by display width, no panic at any size (AC9)

## Behaviour

### The page

| Opened with | Shows | Title row |
|---|---|---|
| `l` or `g L` (`LyricsPage`), from anywhere | the lyrics of the player's current entry | `Lyrics · <title> · <artists>` |

- The page is a page of the history (0006): it is pushed on top; on top already, the key does nothing; going back leaves it; going back onto it shows it again (the history has no forward step)
- **It follows the current entry.** Whenever the current entry's track differs from the track the page shows, and the page is on top, the page shows `Loading…` and asks for the new track's lyrics (one request per change). When the page is not on top, nothing is asked; coming back onto it asks if the track changed meanwhile, and shows the kept lyrics with no request if not
- The current entry counts whether it plays, is paused or is stopped (0009 restores a stopped one): the page shows its lyrics and the highlight at its position. With no current entry: `Nothing playing`, and no request
- The window is titled `Lyrics`, and `Lyrics (synced)` for synced lyrics (`Lyrics (synced) · scrolled` while not following, below)
- Messages in the window: `Loading…`, `No lyrics for this track`, `Nothing playing`; failures `Could not load the lyrics: <reason>` (0006's reasons: `Tidal answered 429: try again in a moment`, the session-expired message); a lost connection shows 0006's disconnected message as is, without the prefix, as the mix and radio pages do (0011)

### Synced and plain lyrics

- **Synced** (Tidal's `subtitles` holds at least one timed line): every line is drawn; the **current line** is the last line whose time is at or before the position; lines before it are drawn in `lyrics_played`, the current line in `lyrics_playing`, the rest in the screen's style. Before the first line's time no line is current and every line is unplayed. Lines with the same time keep Tidal's order, and the current one is the last of them. A timed line with no text (an instrumental gap, the closing stamp) is an empty row, drawn as `♪` while it is the current line (decision 7)
- **Following**: while the page *follows*, the window scrolls so that the current line's first row stays at the middle row of the window once it is past the middle, as spotify-player does; at the top of the lyrics it does not scroll. Seeking moves the highlight and the view with the next `Position`
- **Scrolling by hand** (decision 5): the list movement keys (`j`/`k`/`down`/`up`, `C-f`/`C-b`/`page_down`/`page_up`, `g g`/`G`) scroll synced lyrics too, from where the view is, clamped to the lyrics. The first of them stops following: the view stays where it was put while the highlight keeps moving, and the window's title shows `Lyrics (synced) · scrolled`. **`Esc`** (`ClosePopup`, with no popup or prompt open) follows again: the view jumps back to the current line; when following already, it does nothing. Opening the page and a new track start following
- **Plain** (no timed line, `lyrics` not empty): the text as Tidal gives it, scrolled with the list movement keys (`j`/`k`/`down`/`up`, `C-f`/`C-b`/`page_down`/`page_up`, `g g`/`G`), starting at the top; nothing is highlighted
- **Neither** (both empty or missing): `No lyrics for this track`
- **Long lines** wrap at the window's width by display width (CJK counts double); a wrapped current line is highlighted on all its rows
- **Right-to-left** (`isRightToLeft: true`): every row is right-aligned; the characters are sent in Tidal's (logical) order and the terminal orders them (decision 6)
- Keys that act on rows (`Enter`, `Z`, the actions popup on a row, `/`, `s …`, `d …`) do nothing on the lyrics page; global keys (playback, `a` for the playing track's actions, other pages, `?`) work as everywhere

### Talking to the player

0006's `Library` request, additions (`tidal_player_core::library`):

- `PageRequest::Lyrics(u64)` (the track ID) → `PageData::Lyrics { track: u64, lyrics: Option<Lyrics> }`; `None` is "no lyrics"
- `Lyrics { synced: Vec<LyricLine>, plain: String, right_to_left: bool }`, `LyricLine { at: Duration, text: String }` (`at` in milliseconds on the wire). The player parses `subtitles` (below), so clients receive lines, not LRC

### What the player fetches and keeps

| Request | API call | Kept |
|---|---|---|
| `Page(Lyrics(id))` | `GET /tracks/{id}/lyrics?countryCode=…` | `subtitles` parsed into `synced`, `lyrics` into `plain` (CRLF → LF, trailing blank lines dropped), `isRightToLeft` (missing → `false`). `404` (any body) → `lyrics: None` |

- **The lyrics cache** (decision 2): the player keeps the answers of the last **50** tracks it was asked for, in memory, "no lyrics" included; an asked track already there is answered with no API call, and moves to the newest place. Failures (`LoginRequired`, `429`, transport, `5xx`, malformed) are not kept. The cache lives as long as the player process (a daemon's clients share it); it is not written to disk and has no setting
- Other errors are 0006's, with the page's wording above

### Parsing LRC (`tidal_player_core::lyrics`, pure)

- A line is zero or more **time stamps** `[mm:ss]`, `[mm:ss.x]`, `[mm:ss.xx]` or `[mm:ss.xxx]` (minutes any number of digits; the fraction read as a decimal fraction of a second), then the text with leading and trailing spaces trimmed. A line with several stamps gives one `LyricLine` per stamp, with the same text
- Lines whose first bracket is not a time stamp (`[ar:Adele]`, `[offset:…]`, any other tag) and lines with no stamp are skipped. `\r` is dropped. A stamp with seconds ≥ 60 is not a stamp
- The lines are sorted by time, stably (equal times keep file order)
- `current_line(lines, position) -> Option<usize>`: the index of the last line with `at <= position`; `None` before the first and for no lines

### Keys

| Command | Default keys | Acts in | Help text |
|---|---|---|---|
| `LyricsPage` | `l`, `g L` | everywhere | lyrics of the playing track |
| `ClosePopup` (existing) | `esc` | the lyrics page, with no popup or prompt open (new) | follow the current line again |

`l` and `g L` are no binding's prefix and start none (0008's conflict check stays clean); `g L` appears among `g`'s key hints (0013). `LyricsPage` is no longer reported as an unsupported spotify-player command.

### Theme components

| Component | Drawn on | Default (`default` theme) | spotify-player's default |
|---|---|---|---|
| `lyrics_played` | synced lines before the current one | `Dim` | `Dim` |
| `lyrics_playing` | the current synced line | `fg = "Cyan"`, `Bold` | `fg = "Green"`, `Bold` |

They leave 0017's skipped list (which keeps `playback_genres` and `visualization`). The `spotify_player` theme in `examples/theme.toml` gains both, set as 0017 AC14 sets that theme. The cursor (`selection`) is not drawn on the lyrics page.

### Rendering

```
┌tidal-player──────────────────────────────────────────────────────────────────┐
│▶ Paper Lanterns · The Example Band                                       100%│
│  Night Market                                                                │
│  LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · bit-perfect                        │
│  ━━━━━━━━━━━━━━━━━━━━━━──────────────────────────────────────────  1:10 / 3:42│
│Lyrics · Paper Lanterns · The Example Band                                    │
│┌Lyrics (synced)──────────────────────────────────────────────────────────────┐│
││We hung the lanterns on the wire                                     (played)││
││And watched the river carry them away                                (played)││
││…                                                                            ││
││Paper lanterns, burning slow                                        (playing)││
││Lighting up the road back home                                               ││
│…                                                                             │
```

(The `(played)`/`(playing)` notes mark the styles; they are not drawn. The track and its lyrics are invented, as in the tests.)

## Acceptance criteria

Protocol and types (`tidal_player_core`, pure):

- **AC1** — `Lyrics`, `LyricLine`, `PageRequest::Lyrics` and `PageData::Lyrics` (with and without lyrics) survive a JSON round trip and 0005's codec (0001 AC11's and 0005 AC2's tests, new rows)
- **AC2** — LRC (table): the probe's form `[00:06.32] text`; no space; `[mm:ss]`, `[m:ss.x]`, `[mm:ss.xxx]`, `[123:00.00]`; two stamps on one line → two lines; metadata tags and stamp-less lines skipped; `[00:75.00]` not a stamp; CRLF; a stamp with no text → an empty line; unsorted input sorted stably. `current_line` (table): no lines; before the first; exactly on a stamp; between two; after the last; equal stamps → the last of them

API (`tidal-player-api`, wiremock fixtures from the probe's shapes):

- **AC3** — `Page(Lyrics(id))` sends `GET /tracks/{id}/lyrics` with `countryCode` and the bearer token; `200` with `subtitles` → `synced` parsed, `plain`, `right_to_left`; `200` with an empty `subtitles` → only `plain`; `isRightToLeft: true` → `right_to_left`; `404` with the probe's body → `lyrics: None`; `401`/`LoginRequired`, `429` and `5xx` as 0006 (table)

Player (`tidal-player`):

- **AC4** — The lyrics cache (table, fake API counting calls): a second request for the same track makes no call; "no lyrics" is kept; a failure is not (the next request calls again); the 51st distinct track evicts the least recently asked one; asking for a kept track makes it the newest. Every request is answered with one `LibraryReply` to its sender (0006 AC8's harness, a new row)

Client model (`tidal_player_core::ui`, pure):

- **AC5** — Opening (table): `l` and `g L` push the lyrics page and emit `Library { Page(Lyrics(current track)) }`; on top already, nothing; with no current entry, the page shows `Nothing playing` and nothing is emitted; going back onto the page with the same track emits nothing
- **AC6** — Following the track (table): a snapshot with a new current track while the page is on top emits one request and shows `Loading…`; a new track while the page is not on top emits nothing, and coming back onto it emits one; a `Position` event emits nothing; an answer for a track that is no longer the page's track is dropped (the page stays `Loading…` for the current one); a snapshot with the same track emits nothing
- **AC7** — Display state (table): for synced lyrics and a position, which line is current and which are played; seeking backwards moves it back; plain lyrics scroll with `j`/`k`/`C-f`/`C-b`/`g g`/`G` and clamp; on synced lyrics a movement key scrolls from the view's place and stops following (the view no longer moves with `Position`), `Esc` follows again (the view back on the current line; with a popup open `Esc` closes it and still does not follow; following already, nothing), and a new track or reopening the page follows; `Enter`, `Z`, `/`, `s c`, `d d` on the page emit nothing
- **AC8** — Keymap (0008 AC2's table, new rows): the defaults hold `l` and `g L` → `LyricsPage`, in the keys help (`lyrics of the playing track`) and `g`'s key hints; on the lyrics page the keys help lists `esc` as `follow the current line again`; `LyricsPage` is no longer unsupported; `key_sequence = "l"`, `command = "None"` removes it; the default keymap has no prefix conflict. Disconnected and session expired: 0006 AC16's rows for the lyrics page

TUI rendering (`tidal-player`):

- **AC9** — Rendering (`insta`, 80×24, reviewed by eye, plus `contains` checks): synced lyrics before the first line, in the middle (current line at the middle row), at the end, and scrolled by hand (`· scrolled`, the current line out of view); plain lyrics scrolled; right-to-left lyrics (right-aligned); a long line wrapped with the highlight on every row; an empty current line as `♪`; `Loading…`; `No lyrics for this track`; `Nothing playing`; a failure. 50×20: synced lyrics. No panic from 0×0 to 120×40 with CJK and right-to-left lines
- **AC10** — Themes: `lyrics_played` and `lyrics_playing` are components (`default`: `Dim`; `fg = "Cyan"`, `Bold`) and no longer skipped (0017 AC6's notice test, rows updated); a theme setting them colours the played and current lines (a buffer check); `examples/theme.toml` loads and its `spotify_player` theme sets both (0017 AC14's test)

Wiring:

- **AC11** — An attached client's `l` is answered from the test player's wiremock API, and the client opens no session store (0005 AC14's test, an `l` step)

Docs:

- **AC12** — `docs/tui.md` documents the lyrics page, `l`/`g L`, synced and plain lyrics and the messages; `docs/config.md` lists `LyricsPage` and the two components and drops them from the unsupported and skipped lists; `examples/keymap.toml` shows `l` and `g L`; the zh-TW copies and `README.md`/`README.zh-TW.md` match; `CLAUDE.md` "Features" has the entry; 0008 and 0017 no longer name `LyricsPage`, `lyrics_played` or `lyrics_playing` as skipped

## Edge cases & errors

| Situation | Behaviour |
|---|---|
| An instrumental (Tidal `404`) | `No lyrics for this track`; kept in the cache |
| An unknown track ID (also `404`) | The same: Tidal does not tell them apart |
| Synced lyrics whose `subtitles` hold only tags or garbage | No timed line: shown as plain lyrics (from `lyrics`), or `No lyrics for this track` |
| The track changes while a request is in flight | The late answer is dropped; the new track's request is sent (AC6) |
| Autoplay moves to a suggested track | The current entry changed: the page follows (AC6) |
| Repeat one: the track starts again | Same track: no request; the position goes back, so does the highlight |
| Paused or stopped | The lyrics stay, the highlight at the kept position |
| Two TUIs on one daemon open the page | Two requests; the second is answered from the cache |
| `429`, session expired, disconnected | In the page as 0006 (`Could not load the lyrics: …`; the disconnected message without the prefix); disconnected re-fetched on the next `Welcome` when on top (AC8) |
| A line wider than the window | Wrapped; the current line highlighted on every row (AC9) |
| Right-to-left lyrics in a terminal without bidi support | Right-aligned, characters in logical order: the terminal may show them reversed (decision 6) |
| Scrolled by hand, then the track changes | The new track's lyrics, following again |
| Scrolled by hand, then `Esc` with the actions popup open | The popup closes; still not following; a second `Esc` follows |
| A window 1 row tall, or 0×0 | The current line's first row (or nothing); no panic (AC9) |
| Logout | The player process ends or loses its session; the cache goes with the process |

## Test plan

Each test is named after its criterion (`ac4_…`). Red is a failing assertion against stub types and functions with stub bodies (no `todo!()`, no compile errors). API tests use `wiremock` with fixtures under `crates/api/tests/fixtures/lyrics/`, written from the shapes under "Facts" with **invented lyric text** (lyrics are copyrighted; the probe's output keeps only the first lines and is not committed).

| AC | Test (file :: name) | What it asserts | Expected red |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/protocol.rs` :: `ac11_round_trip` (new rows) + `crates/app/src/ipc/codec.rs` :: `ac2_framing` (new rows) | round trip; decoding | stub `Serialize` writes `null` |
| AC2 | `crates/core/src/lyrics.rs` :: `ac2_parse_lrc` (table), `ac2_current_line` (table) | lines, times, order; current index | stub parser returns no lines; stub `current_line` returns `None` |
| AC3 | `crates/api/tests/lyrics.rs` :: `ac3_lyrics` (table) | path, params, mapping, `404` → `None`, errors | stub answers every request `NotFound` |
| AC4 | `crates/app/src/player_runtime.rs` :: `ac4_lyrics_cache` (table) + `crates/app/src/ipc/server/tests.rs` :: `ac8_reply_to_sender` (new row) | API call counts; eviction order; one reply | stub cache keeps nothing, so calls are 2 not 1 |
| AC5 | `crates/core/src/ui/lyrics/tests.rs` :: `ac5_open_lyrics` (table) | push, request, `Nothing playing`, history | stub `LyricsPage` does nothing |
| AC6 | `crates/core/src/ui/lyrics/tests.rs` :: `ac6_follow_track` (table) | requests per change; late answer dropped | stub never re-requests |
| AC7 | `crates/core/src/ui/lyrics/tests.rs` :: `ac7_display_state` (table) | current/played; scroll; following and `Esc`; inert keys | stub never stops following, so a movement key leaves the view on the current line |
| AC8 | `crates/core/src/ui/keymap/tests.rs` :: `ac8_lyrics_keys` + 0008's help and unsupported tests (rows updated) + `crates/core/src/ui/browse/tests.rs` :: `ac16_disconnected_and_login` (new rows) | default keys, help text, hints, unbinding; disconnected | stub keeps `LyricsPage` unsupported |
| AC9 | `crates/app/src/ui.rs` :: `ac9_lyrics_80x24` (one snapshot per row), `ac9_lyrics_narrow_50x20`, `ac17_no_panic_any_size` (new page) | `contains` + snapshots | stub render draws the queue |
| AC10 | `crates/core/src/ui/theme/tests.rs` :: `ac10_lyrics_components`, `ac6_theme_notice` (rows updated) + `crates/app/tests/examples.rs` :: `ac14_example_theme_toml_loads` | defaults; not skipped; buffer styles; example | stub keeps them skipped |
| AC11 | `crates/app/tests/daemon.rs` :: `ac14_client_needs_no_session` (an `l` step) | answered; no session | stub client drops the request |
| AC12 | — reviewed at acceptance | docs match this spec, linked, zh-TW in step | — |

Checked by hand at acceptance with a real account (results in the PR description): `l` on a playing English, a Chinese and an Arabic track shows their lyrics (the Arabic right-aligned); the highlight follows the singing within about a quarter second and jumps with a seek; `n` moves the page to the next track's lyrics; an instrumental says `No lyrics for this track`; a daemon with two TUIs shows the same.

## Crate placement

- `tidal-player-core`: `lyrics` (new: `LyricLine`, `Lyrics`, `parse_lrc`, `current_line`), `library` (`PageRequest::Lyrics`, `PageData::Lyrics`), `ui` (the lyrics page: `PageKind::Lyrics`, following the track, scroll state), `ui::keymap` (`LyricsPage` and its defaults), `ui::theme` (the two components). No I/O, no new dependency
- `tidal-player-api`: the lyrics call and its DTO mapping (`library.rs`, or a new `lyrics.rs` beside it)
- `tidal-player`: `player_runtime` (the cache in front of the `Library` trait's lyrics request), `ui.rs`/`ui/pages.rs` (the page)
- `xtask layering`: no change

## Facts vs. assumptions

Verified on 2026-11-03 against the **live API** by `scripts/tidal-lyrics-probe.sh` (country NG, a `PREMIUM` subscription with `HI_RES`; read-only; the script was deleted once the spec was implemented; its output is not committed). Fixtures are written from these shapes:

- `GET /v1/tracks/{id}/lyrics?countryCode=NG` → `200`, `content-type: application/json`, `{trackId, lyricsProvider: "MUSIXMATCH", providerCommontrackId, providerLyricsId, lyrics, subtitles, isRightToLeft}`, for Adele *Hello* (62300893), Fairuz *Kifak Inta* (1558491, `isRightToLeft: true`, Arabic), Jay Chou 青花瓷 (337502711) and Queen *Bohemian Rhapsody* (534050211)
- `lyrics`: LF-separated plain text, blank lines between stanzas (60 lines, 1965 bytes for *Hello*). `subtitles`: one line per sung line, `[mm:ss.xx] text` (always two-digit minutes, two-digit fraction, **one space** after the stamp), no blank lines between stanzas, some stamps with no text (`[01:55.15] ` gaps; *Hello* has 4 such lines), the last line a lone stamp near the track's end (`[04:40.31]` for a 4:56 track). No CRLF. Every track with lyrics had both
- Both tokens the player may hold work: the device-flow client's and the PKCE client's (0002 AC19). `deviceType`/`locale` change nothing. No token → `400 {"error":"invalid_request","description":"Token is missing","subStatus":11002}`
- No lyrics (Brian Eno *1/1*, 1403577, an instrumental) and an unknown track (1) → `404 {"timestamp", "status": 404, "error": "Not Found", "path": "/lyrics/public/<id>"}` (not Tidal's usual `subStatus` body)
- A track (`GET /v1/tracks/{id}`, search results) carries no field saying whether it has lyrics: the only way to know is to ask
- `listen.tidal.com/v1/tracks/{id}/lyrics` → `301`; not used
- `openapi.tidal.com/v2/tracks/{id}/relationships/lyrics` and `?include=lyrics` → a `lyrics` resource with `text`, `lrcText` (stamps with no space, blank lines between stanzas), `technicalStatus: "OK"`, `provider {source: "THIRD_PARTY", name: "MUSIXMATCH", …}`, `direction: "LEFT_TO_RIGHT"`. Not used: v1 carries the same, in the shape the rest of the player uses
- The client's position: the engine reports it every 200 ms (`POSITION_EVERY`, `crates/audio/src/engine.rs`), forwarded to clients as `Event::Position`, applied by the client model when it is for the current entry (`crates/core/src/ui/mod.rs`)
- spotify-player (`/home/df/CLionProjects/spotify-player`, `f1522b8`, read 2026-11-03): `LyricsPage` bound to `g L` and `l` (`src/config/keymap.rs`); `render_lyrics_page` (`src/ui/page.rs`) titles the page `<track> by <artists>`, styles lines before the current one `lyrics_played` (default `Dim`), the current one `lyrics_playing` (default `Green`, `Bold`), and scrolls so the current line stays at the middle once past it; the page takes no commands (`src/event/page.rs`); `Lyrics not found` when there are none
- The current code: `l` and `g L` are unbound (`examples/keymap.toml`); `LyricsPage` is reported unsupported (`crates/core/src/ui/keymap.rs`); `lyrics_played`, `lyrics_playing` are in `SKIPPED_COMPONENTS` (`crates/core/src/ui/theme.rs`)

Not verified:

- Other subscription tiers and countries: one account was probed. A tier or country without lyrics most likely answers `404` (shown as `No lyrics for this track`) or `403` (shown as a failure); either way nothing breaks
- A track with plain lyrics but no `subtitles`: none was found; the parser and the page handle it (AC3, AC7)
- How far the stamps lead or lag the audio; checked by hand at acceptance

## Decisions

Answered by the user (2026-11-03; folded into the body above):

1. **Who fetches**: *the client asks on demand, the player fetches* (a `Library` request, as 0006's pages; nothing is fetched while the page is not on top). 0005 AC14 stands: clients hold no session. Alternative: the TUI fetches with its own session (would undo AC14)
2. **Cache**: *in memory, in the player*, the last 50 tracks, "no lyrics" included, failures not. Alternatives: none; on disk
3. **`tidal-player playback lyrics`**: *out of scope*
4. **Keys**: *`l` and `g L`*, as spotify-player

5. **Scrolling synced lyrics** (the user: "a key to follow back when scrolling"; the key: `Esc`): the movement keys scroll synced lyrics and stop following; `Esc` follows again; a new track or reopening the page follows. Alternatives: no scrolling (spotify-player's page takes no commands); scrolling that snaps back at the next line; `Enter` or a new `FollowLyrics` command as the key
6. **Right-to-left**: *right-align, character order left to the terminal* (the player orders no text anywhere else, and terminals with bidi support, such as VTE's and Konsole, would reverse a reordered line again). Alternative: reorder each line for display with `unicode-bidi`, as spotify-player does, which reads right in terminals without bidi support and wrong in those with it
7. **An empty current line** (an instrumental gap): *drawn as `♪`*, so the highlight is visible. Alternative: an empty row, highlighted invisibly
8. **Defaults**: *as spotify-player's*, as far as 0017 AC14 lets its `spotify_player` theme follow them: `lyrics_played` `Dim`, `lyrics_playing` `fg = "Cyan"`, `Bold`. Alternative: uncoloured (`Dim`, `Bold`)

## Implementation notes (choices made where the spec was silent, 2026-11-03)

- **Core**: `tidal_player_core::lyrics` holds `Lyrics`, `LyricLine` (`at` as whole milliseconds on the wire), `parse_lrc` and `current_line` (`partition_point`). `PageKind::Lyrics` has no payload; the page carries `Page.lyrics: Option<LyricsView>` (`track`, `lyrics`, `following`, `scroll`), and `Page::request()` replaces `PageKind::request()` in `fetch_page`. `apply_snapshot` returns effects and ends with `follow_track`, which acts only on a lyrics page on top and compares track IDs (the same track under a new entry ID asks nothing); `browse::back` calls it too, which is how coming back asks once; on `Welcome` it runs before `reconnected`, so a reconnect asks once. An answer is dropped when its request ID is stale or its `track` is not the page's. `l` on a failed lyrics page on top fetches it again, as other pages. Both texts blank → `No lyrics for this track`
- **Scrolling**: the model scrolls in *lines* (it does not know the width); the TUI turns the line offset into rows with `pages.rs`'s `wrap` (display width). Following, the current line's first row sits at row `h / 2` (never above 0). A hand scroll clamps to `len - height`, or to the followed offset when that is further; any movement key on synced lyrics stops following even if the offset does not move; plain lyrics never show `· scrolled`. The window's inner height is 0006's `list_height`, so `Resize` is unchanged
- **Keys help**: a `Where::Lyrics` section lists `ClosePopup` as `follow the current line again` (dim while following) with the Lists, Pages, Playback and App sections. `COMMANDS` holds 51 commands, `UNSUPPORTED_COMMANDS` 26
- **API**: the call sits in `crates/api/src/library.rs` beside `page()`. It reads the body itself, so any `404` is `lyrics: None` whatever its body; other statuses go through 0006's `check`. A `200` that is not JSON is `malformed lyrics response from Tidal`. Every field is optional; `plain` turns CRLF into LF and drops trailing blank lines
- **Player**: `LyricsCache` (`crates/app/src/player_runtime.rs`) wraps the `Library` given to `TokioJobs::with_library`: a `Mutex<VecDeque>` of `LYRICS_CACHE_SIZE` = 50, hits move to the back, inserts evict from the front, only `Ok(Page(Lyrics))` is kept. Two clients asking for the same uncached track at once both call the API (the answers are the same)
- **TUI**: the messages are dim in the window, failures in `error`; plain lyrics are not styled and no cursor is drawn. Right-to-left lyrics use `Alignment::Right`
- **Themes**: `COMPONENTS` holds 21; `SKIPPED_COMPONENTS` keeps `playback_genres` and `visualization`, which the tests that needed a skipped name now use
- **Fixtures** (`crates/api/tests/fixtures/lyrics/`): invented text in the probe's shapes; `lyrics_synced.json` also carries CRLF and trailing blank lines, which the probe did not see

## Out of scope

- `tidal-player playback lyrics` (decision 3)
- Lyrics on disk, prefetching the next track's lyrics
- Word-by-word (karaoke) timing; the provider's credit line
- Seeking by choosing a line (`Enter` on a line); scrolling that snaps back on its own
- Lyrics in MPRIS metadata (`xesam:asText`)
- Translations of lyrics

## Bugs

- **Test plan red reasons (found at acceptance, 2026-11-03).** Some tests did not fail on the red reason the plan names. AC6 and AC7: the red stub left `l` unbound, so `ac6_follow_track` and the `ac7_*` tests failed on opening the page (an index out of bounds in the test helper), not on "never re-requests" or "never stops following"; with the opening made real in a throwaway run they failed for the planned reasons. AC9: the stub drew nothing, not the queue (`"Lyrics (synced)" missing`). AC11: slice A's client already sent the request, so the red was the stub API's `Track 1001 was not found`, not a dropped request. The new `ac8_reply_to_sender_lyrics` row and `ac17_no_panic_any_size` passed on the stubs and stay as regression guards. The tests were right; this entry corrects the plan
