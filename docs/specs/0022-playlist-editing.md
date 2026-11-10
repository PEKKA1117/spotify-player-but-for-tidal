# 0022 — Editing your playlists

- **Status**: implemented (2026-11-05)
- **Owner**: tech-lead (primary session)
- **Issue**: [#66](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/66) (folders moved to [#87](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/87), the CLI to [#88](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/88))
- **Depends on**: 0006 (implemented: pages, the actions popup, playlist writes and the ETag rules), 0008 (implemented: the keymap, the skip list), 0012 (implemented: filters), 0020 (implemented: the selection)
- **User docs**: [`docs/tui.md`](../tui.md) ("Favorites and playlists", a new "Editing your playlists" section, the key table) and [`docs/config.md`](../config.md) (the new commands), and their zh-TW copies (AC13)

## Context

0006 can create a playlist only on the way to adding tracks (*Add to playlist… → New playlist…*), always with an empty description, and left renaming, descriptions, making a playlist public and reordering out of scope. Keeping a playlist in shape still means opening another Tidal app. spotify-player has `CreatePlaylist` (`N`) and `MovePlaylistItemUp`/`Down` (`C-k`/`C-j`); here those names sit in the keymap's skip list, so a keymap copied from spotify-player has them ignored.

tidalt had none of this.

## Behaviour

### Creating a playlist

`N` on the library page (any of its windows) asks for a name and then a description, in the one-row prompt of *New playlist…*:

1. `Playlist name: `: `Enter` goes on, `Esc` cancels. A name that is blank after trimming closes the prompt and sends nothing, as today
2. `Description (optional): `: `Enter` creates the playlist (an empty description is fine), `Esc` cancels and nothing is created

The playlist is private. The message says `Created <title>` and the library is fetched again.

*Add to playlist… → New playlist…* takes the same two steps, then adds the tracks as 0006 does.

`N` anywhere else does nothing.

### Editing a playlist

The actions popup of one of your own playlists (a row of the library's **Playlists** window) gains, after *Play next* and before *Delete playlist*:

| Action | Does |
|---|---|
| *Rename…* | Prompt `Playlist name: ` holding the current title. `Enter` saves, `Esc` cancels. A name blank after trimming, or unchanged, closes the prompt and sends nothing |
| *Edit description…* | Prompt `Description: ` holding the current description. `Enter` saves, an empty text clears the description, `Esc` cancels. An unchanged text sends nothing |
| *Make public* / *Make private* | The one that applies: changes it at once, with no question |

Messages: `Renamed <new title>`, `Saved the description of <title>`, `<title> is now public`, `<title> is now private`. The library and an open page of that playlist are fetched again.

A favorite (followed) playlist, and a playlist row in search results, offer none of these (0006, 0007).

### Moving a track

On the page of one of your own playlists, in its track window:

- `C-k` moves the track under the cursor up one row, `C-j` down one row. The cursor stays on the track
- The row moves at once. The player is told in the background (see "Holding the key")
- `C-k` on the first row and `C-j` on the last row of the list do nothing
- `C-j` on the last loaded row of a list with more to load does not move it: it loads the next page, as `j` there does. Pressing it again moves the row
- Marks of the selection stay on their tracks: a marked row keeps its mark when it or its neighbour moves
- The queue does not change, whatever it was filled from (0005: the queue is the player's and holds its own copy)

Refused, with a message and nothing moved:

| While | Message |
|---|---|
| The window has a kept filter | `Clear the filter to move tracks` |
| Motion mode is on | `Leave motion mode to move tracks` |

While a filter is being typed, the keys belong to the filter (0012). Anywhere else, a favorite playlist's page included, the keys do nothing.

#### Holding the key

A move needs the ETag the previous move returned, so one move at a time can be on its way. With the key held, the rows keep moving on screen and the moves in between are merged:

- With no move on its way, a key press sends one
- With a move on its way, a key press for the same track moves the row on screen only. When the answer comes, one request takes the track from where Tidal now has it to where the screen shows it. If that is the same place, nothing is sent
- With a move on its way, a key press on a different track does nothing, without a message, until the moves have settled

Leaving the page does not stop moves already pressed: the rest is still sent. A move that succeeds shows no message and does not fetch the page again: the screen is already right, and the next move uses the answer's ETag.

### How it looks

The playlist page's title gains `public` for a public playlist, whoever owns it:

```
Late night · 39 tracks · 2:41:07 · public
```

A playlist with a description shows it on one row under the title, cut with `…` to the page's width, and the windows start one row lower: the row and the rule of a mix's subtitle (0011), so a page too short for it leaves it out. Line breaks in a description are shown as spaces. A playlist with no description has no such row.

### Keymap

`CreatePlaylist` (`N`), `MovePlaylistItemUp` (`C-k`) and `MovePlaylistItemDown` (`C-j`) leave the skip list and become commands, listed in the keys help with the page they work on and in `examples/keymap.toml`.

### What the player fetches and sends

| Change | Request | Answer |
|---|---|---|
| Create | `POST /users/{user}/playlists` form `title`, `description` | `201`, the playlist |
| Move | `POST /playlists/{uuid}/items/{from}` form `toIndex`, `If-None-Match: <etag>` | `200`, the new ETag in the `ETag` header |
| Rename, description | `POST /playlists/{uuid}` form `title` and/or `description` (only what changes) | `200`, empty |
| Public, private | `PUT https://api.tidal.com/v2/playlists/{uuid}/set-public`, `…/set-private`, `countryCode` as usual | `200`, empty |

`toIndex` is the **final** position of the track, counted over Tidal's whole playlist. A row's position comes from `Window::positions`, not from its row number, because video items are dropped from the list and still hold their place on Tidal (0006 AC21). Moving a track past its neighbour asks for the neighbour's position, whatever videos lie between.

After a move from `p` to `q`, the client updates positions as Tidal does: going down, the items in `(p, q]` each take one less; going up, the items in `[q, p)` each take one more; the track takes `q`.

### Protocol

`tidal_player_core::library`:

- `LibraryRequest::CreatePlaylist { title, description }` (gains `description`)
- `LibraryRequest::MovePlaylistItem { uuid, from, to, etag }` → `LibraryResponse::Moved { etag: Option<String> }`
- `LibraryRequest::EditPlaylist { uuid, title: Option<String>, description: Option<String> }` → `Done`
- `LibraryRequest::SetPlaylistPublic { uuid, public }` → `Done`
- `PlaylistSummary` gains `description: Option<String>` (`None` when empty) and `public: bool`

Clients and the player are the same version (0005's greeting), so nothing has to stay compatible.

## Acceptance criteria

- **AC1** — `MovePlaylistItem` posts `toIndex` as a form field to `/playlists/{uuid}/items/{from}` with `If-None-Match`, answers `Moved` with the response's `ETag`, and on `412` fails with `The playlist changed: nothing was changed, try again` after one request, with no retry
- **AC2** — `EditPlaylist` posts to `/playlists/{uuid}` exactly the fields given, with no `If-None-Match`
- **AC3** — `SetPlaylistPublic` sends `PUT` to `/v2/playlists/{uuid}/set-public` or `/set-private` on the v2 base, with `countryCode`, and answers `Done` on an empty `200`
- **AC4** — `PlaylistSummary` carries `description` (an empty string is `None`) and `public`, from the playlists list and from the playlist header; `CreatePlaylist` posts the description it is given
- **AC5** — Creating: `N` in any library window opens the name prompt, then the description prompt, then sends `CreatePlaylist` once; `Esc` at either sends nothing; a blank name closes the prompt and sends nothing; the title is trimmed; *New playlist…* takes the same steps and adds its tracks after; `N` on any other page does nothing
- **AC6** — Editing actions: own playlist rows list *Rename…*, *Edit description…* and exactly one of *Make public*, *Make private*, between *Play next* and *Delete playlist*; favorite playlists and search rows list none
- **AC7** — Editing prompts: they open holding the current text; `Enter` sends `EditPlaylist` with only the changed field; a blank name or unchanged text sends nothing; an empty description sends `description: Some("")`; *Make public*/*Make private* send `SetPlaylistPublic` at once; each success gives its message and fetches the library and an open page of the playlist again
- **AC8** — Moving: on an own playlist's track window `C-j`/`C-k` swap the row with its neighbour, the cursor stays on the track, and `MovePlaylistItem` carries the track's position and the neighbour's from `Window::positions` (videos between them included) and the page's ETag; `Window::positions` afterwards matches Tidal's order; first row up and last row down do nothing; the last loaded row with more to load loads the next page and does not move
- **AC9** — Merging: with a move on its way, further presses on that track move the row only; the answer sends one request from Tidal's position to the screen's position with the answer's ETag, or none if they are equal; a press on another track does nothing; the moves continue after the page is left; a settled move shows no message, fetches nothing and leaves the page holding the last ETag
- **AC10** — A failed move: the moves not yet sent are dropped, the error shows in the message row and an open page of the playlist is fetched again, for `412` and any other failure alike. A `Moved` with no ETag fetches the page again
- **AC11** — Selection and refusals: a marked row keeps its mark through a move, whether it is the moved row or the neighbour; in a window with a kept filter, and in motion mode, the keys show their message and change nothing; on a favorite playlist's page and any other page they do nothing
- **AC12** — Rendering: the playlist page's title ends in `· public` for a public playlist; a description takes one row, cut with `…`, line breaks as spaces; without one the layout is unchanged; the description prompt draws its label
- **AC13** — Keymap and docs: `CreatePlaylist`, `MovePlaylistItemUp`, `MovePlaylistItemDown` are commands bound by default to `N`, `C-k`, `C-j`, off the skip list, in the keys help and `examples/keymap.toml`; `docs/tui.md`, `docs/config.md`, `README.md` and their zh-TW copies cover them

## Edge cases & errors

| Case | Behaviour |
|---|---|
| Disconnected from the player | `write`'s existing behaviour: the row does not move and nothing is queued |
| The playlist changed elsewhere before a move | `412`: AC10's message, the page reloads, the track goes back to where Tidal has it |
| The playlist shrank elsewhere and the position is gone | `400`/`1002` `Invalid indices`: as any failed move (AC10) |
| Renamed elsewhere at the same time | Tidal checks no ETag on rename, description or public: the last write wins, nothing warns. Stated in `docs/tui.md` |
| A playlist of one track | Both keys do nothing |
| The same track twice in a playlist | Rows are told apart by position, so each moves on its own |
| A very long title or description | Tidal took 300 and 1 000 characters, so no limit is set here. The prompt shows the end of the text (0006) |
| A description with line breaks, edited here | The prompt holds one line: line breaks become spaces, and saving writes that. Saving unchanged text sends nothing, so they survive unless the text is edited |
| `N` with a prompt or popup open | Text goes to the prompt; popups keep their own keys (`N` in the selection popup is spec 0020's) |
| Session expired during a move | `LoginRequired`'s message, as any failed move (AC10) |
| `Enter` on a track while moves are settling | Plays the list in the order on screen (the client's rows) |

## Test plan

| AC | Test | Asserts | Expected red failure |
|---|---|---|---|
| AC1 | `crates/api/tests/library.rs` :: `ac1_move_item_sends_to_index_and_etag`, `ac1_move_item_answers_the_new_etag`, `ac1_move_item_412_is_not_retried` | Path, form, header, `Moved.etag`, one request on `412` | `LibraryRequest::MovePlaylistItem` is not handled (stubbed to `unimplemented` error in the red commit) |
| AC2 | `crates/api/tests/library.rs` :: `ac2_edit_playlist_sends_only_given_fields` (table: title, description, both, empty description) | Form body, no `If-None-Match` | The request is not sent |
| AC3 | `crates/api/tests/library.rs` :: `ac3_set_public_uses_the_v2_base` (table: public, private) | `PUT`, path, `countryCode`, `Done` on an empty body | The request is not sent |
| AC4 | `crates/api/tests/library.rs` :: `ac4_summary_carries_description_and_public`, `ac4_create_sends_the_description` | Fields from a recorded list and header; form body | `description` stays `None`, `public` stays `false` |
| AC5 | `crates/core/src/ui/browse/tests.rs` :: `ac5_create_from_the_library`, `ac5_create_cancelled_or_blank` (table), `ac5_new_playlist_asks_the_description`, `ac5_n_elsewhere_does_nothing` | Prompts, one request, tracks added after | `N` does nothing |
| AC6 | `crates/core/src/ui/browse/tests.rs` :: `ac6_actions_of_a_playlist` (table: own private, own public, favorite, search) | Action lists and order | The actions are missing |
| AC7 | `crates/core/src/ui/browse/tests.rs` :: `ac7_edit_prompts` (table), `ac7_public_at_once`, `ac7_success_messages_and_refetch` | Prefill, requests, messages, refetches | The prompts do not open |
| AC8 | `crates/core/src/ui/browse/tests.rs` :: `ac8_move_swaps_and_keeps_the_cursor`, `ac8_move_over_videos_uses_positions`, `ac8_ends_of_the_list`, `ac8_last_loaded_row_loads_more`; `crates/core/src/ui/page.rs` tests :: `ac8_positions_after_a_move` (table) | Rows, cursor, request, positions | The keys do nothing |
| AC9 | `crates/core/src/ui/browse/tests.rs` :: `ac9_presses_merge_into_one_request`, `ac9_back_to_where_it_was_sends_nothing`, `ac9_other_track_waits`, `ac9_moves_continue_after_leaving`, `ac9_settled_move_is_silent` | Requests sent per reply, ETag used, no refetch | A request per press |
| AC10 | `crates/core/src/ui/browse/tests.rs` :: `ac10_failed_move_reloads` (table: `412` message, other error), `ac10_no_etag_reloads` | Message, refetch, pending moves dropped | The failure leaves the optimistic order |
| AC11 | `crates/core/src/ui/browse/tests.rs` :: `ac11_marks_follow_a_move` (table: moved row, neighbour), `ac11_refused_with_a_filter_or_in_motion`, `ac11_not_an_own_playlist` | Marks, messages, nothing sent | Marks stay on the row numbers |
| AC12 | `crates/app/src/tui/render.rs` tests :: `ac12_playlist_title_public`, `ac12_description_row`, `ac12_description_cut_and_flattened`, `ac12_no_description_unchanged`, `ac12_description_prompt` (snapshots at 80 × 24 and 150 × 42) | The drawn page | New snapshots missing |
| AC13 | `crates/core/src/ui/keymap/tests.rs` :: `ac13_playlist_commands_bound`, `ac13_off_the_skip_list`; `crates/core/src/ui/help/tests.rs` :: `ac13_keys_help_lists_them`; `examples/keymap.toml` by the existing `crates/app/tests/examples.rs` (0008 AC18) | Bindings, skip list, help | The commands are skipped with a warning |
| AC13 (docs) | Read at acceptance: no test can judge prose. `cargo xtask wiki` builds | | |

**Hands-on check** (live Tidal, a throwaway playlist, deleted after): `N` with a description; rename; edit and clear the description; make public and private; hold `C-j` over ten rows, then reload the page and compare the order; the same over a playlist holding a video; a move after editing the playlist in another Tidal app (`412`). `C-j` and `C-k` are checked inside tmux as well as directly, as terminals can read `C-j` as `Enter`.

## Crate placement

- `tidal-player-core`: `library` (the types), `ui` (keys, prompts, the move state, positions: all pure)
- `tidal-player-api`: `library.rs` (the requests). The client has one base URL ending in `/v1`: the v2 call uses the path `../v2/playlists/…`, which `Url` resolves, so tests reach the mock server with no second base
- `tidal-player`: `ui.rs`, `ui/pages.rs` (the title and description row)

## Facts vs. assumptions

Probed against the live API on 2026-11-05 with a throwaway playlist of three tracks, deleted after (`404` confirmed):

- **Move**: `toIndex` is the final position. `ABC` move 0 to 1 gives `BAC`; `BAC` 0 to 2 gives `ACB`; `ACB` 2 to 0 gives `BAC`. Past the end clamps to the last place (`ACB` 0 to 99 gives `CBA`). Same place is a `200`. Several indices work (`CBA` `0,1` to 3 gives `ACB`), not used here
- **Move errors**: no ETag or a stale one: `412`, `subStatus` `7002`. A position that does not exist: `400`, `subStatus` `1002`, `Invalid indices`
- **Move ETag**: the `200`'s `ETag` header equals the next `GET`'s
- **Edit**: `title` alone keeps the description and `description` alone keeps the title. `description=` clears it. `title=` is **ignored without an error** (`200`, title unchanged). A title of spaces is **stored as it is**. CJK and emoji survive. 300 and 1 000 characters were taken; no limit was found
- **Edit ETag**: **not checked**: a rename with no ETag and with a stale ETag both applied
- **Public**: `PUT /v2/playlists/{uuid}/set-public` and `set-private` answer `200` with an empty body and no ETag; the ETag is not checked either. The list shows the change at once
- **List**: `playlistsAndFavoritePlaylists` rows carry `description` and `publicPlaylist`

Checked in the hands-on run (2026-11-05, a private playlist of a track, a video and 14 tracks, deleted after):

- A move over a hidden video follows the positions rule: up and down over it, the order on screen and after a reload were the same
- Ten `C-j` at once ended where the screen showed, after a reload too
- `C-j` and `C-k` reach the player inside tmux. Outside tmux is left to the maintainer: the run had no other terminal

## Decisions

1. **Rows move at once, requests merge** (over ignoring presses while a move is on its way): holding the key stays smooth and a long move costs a few requests, because Tidal moves any distance in one request
2. **A settled move fetches nothing**: the answer's ETag and the local positions are enough, and a fetch would reset the list under a held key. Failure is the one case that reloads
3. **`Enter` in the name prompt goes to a second prompt on both paths**: the issue asks for an optional description from the library and the picker; one flow is less to learn than two, and skipping it costs one key
4. **No question for *Make public***: it is two steps away in the actions popup and undone with the same action. `confirm_removals` covers what cannot be undone
5. **A blank name is refused here**: Tidal ignores an empty one without saying so and stores one of spaces, so the client would otherwise report a rename that did not happen, or make an unreadable row
6. **Refuse moves under a filter**: neighbours on screen are not neighbours in the playlist, so the result could not be predicted from what is shown
7. **No fake protection for rename**: Tidal checks no ETag there, so the docs say the last write wins rather than sending an ETag that does nothing

## Out of scope

- Playlist folders (#87) and CLI playlist commands (`playlist new|delete|fork|import`, #88)
- Moving several selected rows at once (the API allows it; 0020's selection keys would need a design)
- Editing from a playlist's own page: actions there belong to the row, and spotify-player's `ShowActionsOnCurrentContext` stays skipped
- Editing the playlist image, collaborators
- Sorting a list (#52): when it comes, a sorted window refuses moves as a filtered one does
