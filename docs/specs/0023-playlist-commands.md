# 0023 — Playlist commands

- **Status**: approved (2026-11-05)
- **Owner**: tech-lead (primary session)
- **Issue**: [#88](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/88)
- **Depends on**: 0005 (implemented: the socket, one-shot commands, exit codes), 0006 (implemented: the library requests), 0022 (implemented: the create, edit and public requests)
- **User docs**: [`docs/daemon.md`](../daemon.md) (a new "Playlists from a shell" section) and its zh-TW copy, `README.md` and its zh-TW copy (AC11)

## Context

0022 lets the TUI create and edit playlists, but scripts can only reach the player through `tidal-player playback`, which has no playlist commands. spotify-player has `playlist new/delete/import/fork/update/list`. This spec adds `tidal-player playlist`, which talks to the running player as `playback` does.

tidalt had none of this.

## Behaviour

`tidal-player playlist <command>` sends its requests to the running player (standalone, daemon or `play`) over the socket and exits.

`PLAYLIST`, `FROM` and `TO` are a playlist's UUID, as `list` prints it, or its Tidal link (see [items](../playback.md#items)). A title is not accepted: titles repeat, and `delete` must not guess.

| Command | Does | Prints on success |
|---|---|---|
| `list`, `list --json` | Lists your playlists and the ones you follow, newest first | See below |
| `new NAME [-d DESCRIPTION] [--public]` | Creates a playlist, private unless `--public` | The new UUID |
| `edit PLAYLIST [--name NAME] [--description TEXT] [--public \| --private]` | Changes what is given, on one of your own playlists. `--description ""` clears the description | Nothing |
| `delete PLAYLIST [--yes]` | Deletes one of your own playlists | Nothing |
| `fork PLAYLIST [--name NAME]` | Copies a playlist's tracks and description into a new private playlist, by default named `<title> (copy)` | The new UUID |
| `import FROM TO [--duplicates]` | Adds `FROM`'s tracks at the end of `TO`, one of your own playlists, in `FROM`'s order | `Added 12 tracks to <title>, skipped 3` |

Exit codes as `playback` (0005):

- `0`: done
- `1`: no player running (`No player is running: …`), the player's error on stderr, or no answer within 5 s (`The player did not answer`)
- `2`: a bad argument, nothing sent

### `list`

One playlist per line, tab-separated: UUID, `own` or `followed`, the number of tracks, the title.

```
$ tidal-player playlist list
a5f9389f-ab20-4d4a-937a-a21ec2559abb	own	39	Late night
edad2071-060a-4392-870b-4dc9c5c6ac66	followed	25	Linkin Park Essentials
```

A title's tabs and line breaks are printed as spaces, so a line stays a line. `--json` prints one array of `{"uuid", "title", "own", "public", "tracks", "description"}`, with `tracks` and `description` `null` when unknown or empty.

### Asking before `delete`

`delete` reads the playlist first. On a terminal it asks `Delete <title>? (y/n)` on stderr: `y` deletes, anything else exits 0 with nothing deleted. `--yes` does not ask. With no terminal on stdin and no `--yes`, it exits 2 with `Use --yes to delete without a terminal`, and nothing is deleted.

### Your own playlists only

`edit`, `delete` and the `TO` of `import` read the playlist's header first. One you do not own exits 1 with `<title> is not your playlist`, and nothing is written.

### `fork` and `import`

Both are built from the library requests, in the client:

1. The source's tracks are read page by page (`More`, 100 at a time). `import` reads `TO`'s too
2. `fork` creates the playlist. `import` keeps the tracks of `FROM` not in `TO`, by track ID, and the first of any repeated in `FROM`. `--duplicates` keeps them all
3. The tracks are added 100 at a time, in order: one `AddToPlaylist` each, with `allow_duplicates` set, so each has its own 5 s

A source's videos are not copied: the player's lists hold tracks only (0006). When the source has any, stderr says `Skipped 2 videos`, from its total less its tracks.

If a step fails after anything was written, stderr says how far it got and the exit code is 1:

```
Added 300 of 1 234 tracks to Late night (copy): Could not reach Tidal: timeout
```

`fork` then still prints the new UUID on stdout, so that a script can find what was left. Nothing is rolled back.

A source with no tracks is fine: `fork` creates an empty playlist, and `import` prints `Added 0 tracks to <title>, skipped 0`.

## Acceptance criteria

- **AC1** — Parsing: every row of the table parses to its plan; `PLAYLIST` takes a bare UUID (which `parse_item` refuses, so it is read here first) or a playlist link; a track, an album or an unparsable text is exit 2 with nothing sent, as are a blank `NAME` (`new`, `edit --name`, `fork --name`), `edit` with nothing to change, and `edit --public --private`
- **AC2** — `list` pages through `More { Playlists }` to the end and prints the tab-separated lines, tabs and line breaks in titles as spaces; `--json` prints the array
- **AC3** — `new` sends `CreatePlaylist` (title trimmed) and prints the UUID; `--public` then sends `SetPlaylistPublic`, and if that fails stdout still has the UUID and the exit code is 1
- **AC4** — `edit` sends one `EditPlaylist` holding only what was given, then `SetPlaylistPublic` if asked; only the public flag sends no `EditPlaylist`; `--description ""` sends `Some("")`
- **AC5** — Own playlists: `edit`, `delete` and `import`'s `TO` read the header, and one not owned is exit 1 with `<title> is not your playlist` and no write
- **AC6** — `delete`: `--yes` deletes without asking; on a terminal `y` deletes, and anything else exits 0 with nothing sent; with no terminal and no `--yes`, exit 2 before anything is sent
- **AC7** — `fork` reads every page of the source, creates the playlist with the source's description (the name given, or `<title> (copy)`), adds the tracks 100 at a time in order, and prints the UUID. An empty source creates an empty playlist
- **AC8** — `import` adds `FROM`'s tracks at the end of `TO` in order, 100 at a time, skipping those in `TO` and repeats in `FROM`, and prints `Added N tracks to <title>, skipped M`; `--duplicates` adds them all
- **AC9** — Videos and partial failures: a source with fewer tracks than its total prints `Skipped N videos` on stderr; a failure after a write prints `Added N of M tracks to <title>: <reason>`, exit 1, and `fork` still prints the UUID
- **AC10** — Exit codes through the binary: no player running is 1 with 0005's message; a bad argument is 2; a silent player is `The player did not answer`, 1
- **AC11** — `docs/daemon.md`, `README.md` and their zh-TW copies document the commands

## Edge cases & errors

| Case | Behaviour |
|---|---|
| Not logged in | The player's `LoginRequired` message, exit 1 |
| `FROM` and `TO` are the same playlist | Exit 2, nothing sent: `FROM and TO are the same playlist` |
| A playlist that does not exist | `Playlist <uuid> was not found`, exit 1 |
| The list changes during a read | Pages are read by offset: a track can be missed or read twice. `import` removes repeats, and nothing checks further |
| `fork` of a followed playlist | Allowed: that is what a fork is for |
| `import` from a followed playlist | Allowed: only `TO` must be your own |
| Tracks that are no longer available | Copied as Tidal lists them; Tidal may refuse them, which is a failure as any other (AC9) |
| `NAME` with surrounding spaces | Trimmed, as the TUI's prompts do (0022) |
| Another client edits `TO` during an import | The player reads the ETag per add and retries once (0006); a second `412` is a failure as any other (AC9) |
| `list --json` with no playlists | `[]` |

## Test plan

| AC | Test | Asserts | Expected red failure |
|---|---|---|---|
| AC1 | `crates/app/src/playlist_cli.rs` :: `ac1_parse` (table), `ac1_refused` (table) | Plans and usage errors | The stub `plan` refuses everything |
| AC2 | `crates/app/src/playlist_cli.rs` :: `ac2_list_lines`, `ac2_list_pages`, `ac2_list_json` | Output and the `More` requests | Nothing is sent |
| AC3 | `crates/app/src/playlist_cli.rs` :: `ac3_new`, `ac3_new_public_fails` | Requests, stdout, exit code | Nothing is sent |
| AC4 | `crates/app/src/playlist_cli.rs` :: `ac4_edit` (table) | Requests | Nothing is sent |
| AC5 | `crates/app/src/playlist_cli.rs` :: `ac5_not_your_playlist` (table: edit, delete, import) | The message, no write | Nothing is sent |
| AC6 | `crates/app/src/playlist_cli.rs` :: `ac6_delete` (table: `--yes`, `y`, `n`, no terminal) | Requests, exit code | Nothing is sent |
| AC7 | `crates/app/src/playlist_cli.rs` :: `ac7_fork`, `ac7_fork_batches`, `ac7_fork_empty` | Requests in order, stdout | Nothing is sent |
| AC8 | `crates/app/src/playlist_cli.rs` :: `ac8_import`, `ac8_import_duplicates` | Tracks sent, order, the line | Nothing is sent |
| AC9 | `crates/app/src/playlist_cli.rs` :: `ac9_skipped_videos`, `ac9_partial_failure` (table: fork, import) | stderr, stdout, exit code | Nothing is sent |
| AC10 | `crates/app/tests/daemon.rs` :: `ac10_playlist_exit_codes` | The binary's exit codes and messages | The subcommand does not exist |
| AC11 | Read at acceptance: no test can judge prose. `cargo xtask wiki` builds | | |

**Hands-on check** (live Tidal, against an isolated player with the existing session, throwaway playlists deleted after): `new` with a description and `--public`, `edit`, `list` and `list --json | jq`, `fork` of a playlist of over 100 tracks, `import` twice (the second adds nothing), `delete` with `--yes` and from a terminal. It prints to a shell, so the PR quotes the output rather than screenshots.

## Crate placement

- `tidal-player`: `playlist_cli.rs` (new): the clap commands, `plan`, the flows, generic over `Link`; `main.rs` wires the subcommand. `oneshot.rs` keeps its `REPLY_TIMEOUT` and `NO_ANSWER`
- `tidal-player-core`, `tidal-player-api`: no change, the requests exist

## Facts vs. assumptions

Facts, from the code and 0022's probe:

- `ClientMessage::Library { id, request }` and `ServerMessage::LibraryReply { id, result }` carry every library request to any client over the socket
- `AddToPlaylist` reads the ETag and sends at most 100 tracks per request to Tidal; a large one takes one request per 100 before it answers
- `PlaylistTracks` pages are at most 100 items, and `ListPage::total` counts videos
- `PageData::Playlist` holds `own`, `description`, `tracks` and the first page. `own` is the header's `creator.id` compared with the user's, for any playlist
- `parse_item` reads links only: a bare UUID is refused, and a bare number is a track

Assumed:

- Tidal has no native fork worth using: not looked for, since building it from existing requests needs no new endpoint and no probe. It costs the videos

## Decisions

1. **`delete` asks on a terminal and refuses without one**: it cannot be undone, and a script should say `--yes` on purpose
2. **`import` skips what `TO` has**: running it twice then adds nothing, which is what a script wants. `--duplicates` is the exception
3. **`fork` and `import` run in the client**: no change to the player or the API, and the client sees each step, so a failure can say how far it got
4. **Batches of 100 from the client**: a single `AddToPlaylist` of 4 000 tracks would outlast the 5 s a one-shot waits; 100 is one request to Tidal
5. **No rollback**: deleting a half-made fork could lose nothing but also explains nothing; the UUID and the count let the user decide
6. **`list` is in, though the issue does not name it**: nothing else can tell a user a UUID. `edit` is in as it only wires 0022's requests

## Out of scope

- spotify-player's `sync`, which keeps a record of what was imported
- Naming a playlist by title
- Moving or removing tracks from the CLI
- Copying a playlist's videos
- Other one-shot commands (#67)
