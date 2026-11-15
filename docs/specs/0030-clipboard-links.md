# 0030 — Copy a link, open a link from the clipboard

English | [正體中文](zh-TW/0030-clipboard-links.md)

- **Status**: approved (2026-11-16; the user approved every spec in advance and asked for the PR to be left for review)
- **Owner**: tech-lead (primary session)
- **Issue**: [#57](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/57)
- **Depends on**: 0004 (implemented: links and track IDs, `Open`), 0006 (implemented: the actions popup, pages), 0007 (implemented: the `o`/`O` prompts), 0008 (implemented: the keymap, `[[actions]]`, the skipped names), 0011 (implemented: mix pages), 0020 (implemented: the selection actions)
- **User docs**: [`docs/tui.md`](../tui.md) (a new "Links and the clipboard" section, the keys table), [`docs/config.md`](../config.md) (the commands and actions tables, the skipped names), `examples/keymap.toml`, `README.md`, and their zh-TW copies

## Context

Sharing what is playing, or opening what someone shared, means leaving the TUI today: there is no way to get an item's Tidal link out of it, and a link copied in a browser has to be pasted into the `o` prompt, which only queues (and refuses artist and mix links). spotify-player has a `CopyLink` action and an `OpenSpotifyLinkFromClipboard` command for this; both are on our keymap's list of skipped spotify-player names (0008 decision 3), so a spotify-player `keymap.toml` that binds them loses those bindings with a notice.

tidalt had neither.

## Behaviour

### Copy link

The actions popup gains **Copy link**, the last entry, on every item that has a Tidal link:

| Row | Link copied |
|---|---|
| a track (any track list, credits, a queue entry, the playing track with `a`) | `https://tidal.com/browse/track/<id>` |
| an album | `https://tidal.com/browse/album/<id>` |
| an artist | `https://tidal.com/browse/artist/<id>` |
| a playlist (yours or not) | `https://tidal.com/browse/playlist/<uuid>` |
| a mix | `https://tidal.com/browse/mix/<id>` |

*Collected Tracks* (0028) has no link and no *Copy link*; a browse link (0024) has no actions popup; the selection's actions (`s a`, 0020) do not copy.

Choosing it closes the popup and puts the link on the clipboard (see "The clipboard"). The message row then says `Link copied: https://tidal.com/browse/album/10`, or why it could not.

`CopyLink` (spotify-player's name) is an `[[actions]]` action: `target = "SelectedItem"` (the default) copies the row under the cursor's link, `target = "PlayingTrack"` the playing track's, exactly as the popup would, and nothing where the popup would not list it. It has no default key, as in spotify-player.

### Open a link from the clipboard

`OpenSpotifyLinkFromClipboard` (spotify-player's name, so its `keymap.toml` entries work as they are) reads the clipboard and opens the Tidal link in it. Its default key is `g p` ("paste"): spotify-player's `O` is `PlayNextPrompt` here (0007), and `g p` was free. It acts on a page, as the page commands do, and not over a popup, a prompt or the keys help.

The clipboard's text, trimmed, is read with 0004's rules for links (an `https://` link on `tidal.com`, `www.tidal.com` or `listen.tidal.com`, or `tidal://`; an optional `/browse`; a trailing slash, `/u`, query or fragment), widened to artist and mix links, and a bare number is a track ID:

| The clipboard holds | What happens |
|---|---|
| an album, playlist, artist or mix link | its page opens (pushed on the history, like *Open*); nothing plays or changes in the queue |
| a track link or a track ID | the track is added to the end of the queue, as `o` would |
| anything else | the message row says `Not a Tidal link: <text>`, the text cut to 60 characters with `…` |
| nothing (or only spaces) | `The clipboard is empty` |
| no clipboard to read | the reason (see below) |

The answer comes back a moment after the key; when a popup, a prompt or the keys help opened in between, it is dropped, so that it never acts on something the user did not see.

The `o`/`O` prompts are unchanged: they still take track, album and playlist links only, since they queue.

### The clipboard

The TUI uses the clipboard of the machine it draws on, chosen each time from its environment:

| Environment | Copy | Read |
|---|---|---|
| `WAYLAND_DISPLAY` set | `wl-copy` | `wl-paste --no-newline` |
| else `DISPLAY` set | `xclip -selection clipboard`, or `xsel --clipboard --input` when `xclip` is not installed | `xclip -selection clipboard -o`, or `xsel --clipboard --output` |
| neither (a console, SSH) | OSC 52: the terminal's own clipboard escape sequence, written to the terminal | not possible |

- A tool that is not installed: `Copying the link needs wl-copy: install wl-clipboard` on Wayland, `Copying the link needs xclip or xsel: install one of them` on X11 (`Reading the clipboard needs …` for a read, the same tools)
- Reading with neither variable set: `Reading the clipboard needs wl-paste (Wayland), xclip or xsel (X11)`
- A tool that exits with an error: copying says `Could not copy the link: <tool> exited with status <n>`; reading says `The clipboard holds no text` (that is what `wl-paste` and `xclip` do on an empty or non-text clipboard)
- A tool that has not finished after 2 seconds is killed: `Could not copy the link: <tool> did not answer` / `Could not read the clipboard: <tool> did not answer`
- OSC 52 always "works" from the TUI's side: whether the terminal honours it is the terminal's business (kitty, foot, WezTerm, iTerm2 and xterm with `allowWindowOps` do; inside tmux it needs `set -g set-clipboard on`). The message row says `Link copied (OSC 52): <link>`, so a user whose terminal ignores it knows which way it went

Only the TUI touches the clipboard: copying and reading never reach the player, so an attached TUI copies on its own machine whatever the daemon's.

## Acceptance criteria

- **AC1** — Links are built and read by one pure module: each kind (track, album, artist, playlist, mix) gives the link in "Copy link"'s table, and reading it back gives the same kind and ID. Artist and mix links are read with all of 0004's variants (host, `tidal://`, `/browse`, trailing slash, `/u`, query, fragment); a mix ID is letters and digits, kept as written. 0004's `parse_item` still refuses artist and mix links
- **AC2** — The actions popup lists *Copy link* last on a track row, a credit row, a queue entry, the playing track, an album, an artist, a playlist (yours and not yours) and a mix, with that row's link; not on *Collected Tracks*, and not among the selection's actions
- **AC3** — Choosing *Copy link* closes the popup and asks the TUI to copy the link; its result sets the message row: `Link copied: <link>`, `Link copied (OSC 52): <link>`, or the reason it failed
- **AC4** — `CopyLink` is an `[[actions]]` action with both targets, running exactly what the popup would (nothing where the popup has no *Copy link*); it has no default key, and it is no longer a skipped name (a `keymap.toml` binding it gives no notice)
- **AC5** — `OpenSpotifyLinkFromClipboard` is a command, bound to `g p` by default, listed in the keys help ("open the Tidal link in the clipboard"); on a page it asks the TUI to read the clipboard, over a popup, a prompt or the keys help it does nothing, and it is no longer a skipped name
- **AC6** — The clipboard's text is handled per "Open a link from the clipboard"'s table: an album, playlist, artist or mix link opens its page (the page asked for, nothing sent to the player); a track link or ID sends `Open` of that track at the end of the queue; anything else, nothing and a failed read set the message row with the table's wording (60 characters and `…`); an answer that comes while a popup, a prompt or the keys help is open is dropped
- **AC7** — The clipboard is chosen from the environment per "The clipboard"'s table: Wayland first, then X11, else OSC 52 for copying and no reading
- **AC8** — Copying runs the table's tool with the link on its standard input and waits for it at most 2 seconds; `xsel` is tried when `xclip` is not installed; a missing tool, an error exit and a timeout give the messages above
- **AC9** — Reading runs the table's tool and returns its output; a missing tool, an error exit, a timeout and no display give the messages above; output past 64 KiB is cut there (it is not a link anyway)
- **AC10** — OSC 52 is `ESC ] 52 ; c ; <base64 of the link> BEL`, written to the terminal between two frames
- **AC11** — The user docs (`docs/tui.md`, `docs/config.md`), `examples/keymap.toml`, `README.md` and their zh-TW copies describe *Copy link*, `CopyLink`, `OpenSpotifyLinkFromClipboard` and `g p`, and the two names leave the docs' lists of skipped spotify-player names

## Edge cases & errors

- **The player is gone** (disconnected): copying still works (it is the TUI's); a track from the clipboard is not queued, as nothing reaches a player that is not there (0005); a page opens and shows its load failing like any page
- **A link whose page no longer exists** (a deleted playlist, a wrong ID): the page opens and says it could not load, as *Open* on a stale row does
- **Text around a link** (`Listen: https://tidal.com/…`): not a link; the whole text must be one, as in the `o` prompt
- **A huge clipboard** (an image, a file): the read stops at 64 KiB, and the text is not a link
- **Non-text clipboard**: `wl-paste` and `xclip` fail: `The clipboard holds no text`
- **Both `WAYLAND_DISPLAY` and `DISPLAY` set** (XWayland): Wayland's tools, which see the same clipboard
- **A tool that forks to keep serving the clipboard** (`wl-copy`, `xclip`): its output is not waited for (standard output and error go to `/dev/null`), only the exit of the process started
- **Tiny terminals**: nothing new is drawn but the message row and a popup entry

## Test plan

| AC | Test | Asserts | Expected red failure |
|---|---|---|---|
| AC1 | `crates/core/src/item.rs` :: `ac1_link_round_trip` (table) + `crates/core/src/item.rs` :: `ac1_artist_and_mix_links` (table) + `crates/core/src/item.rs` :: `ac1_parse_item_refuses_artist_and_mix` | kind → link → kind; each variant of an artist and a mix link; `parse_item` unchanged | the stub `parse_link` reads nothing |
| AC2 | `crates/core/src/ui/popup/tests.rs` :: `ac2_copy_link_entries` (table) | the last entry of each row kind's actions, and its absence on *Collected Tracks* and the selection | no *Copy link* entry |
| AC3 | `crates/core/src/ui/clipboard.rs` :: `ac3_copy_link_from_popup` + `crates/core/src/ui/clipboard.rs` :: `ac3_copied_message` (table) | the popup closes and `Effect::Copy` carries the link; each result's message | the entry does nothing |
| AC4 | `crates/core/src/ui/keymap/tests.rs` :: `ac4_copy_link_action` + `crates/core/src/ui/clipboard.rs` :: `ac4_copy_link_binding` (table) | the name parses with both targets and gives no notice; both targets' effects, nothing on *Collected Tracks* | `CopyLink` is skipped with a notice |
| AC5 | `crates/core/src/ui/keymap/tests.rs` :: `ac5_open_link_from_clipboard_command` + `crates/core/src/ui/clipboard.rs` :: `ac5_reads_the_clipboard_on_a_page` (table) | default `g p`, no notice, help text; `Effect::ReadClipboard` on a page and nothing over a popup, a prompt or the help | the command is skipped with a notice |
| AC6 | `crates/core/src/ui/clipboard.rs` :: `ac6_clipboard_text` (table) + `crates/core/src/ui/clipboard.rs` :: `ac6_late_answer_dropped` | per text: the page pushed and asked for, `Open` at the end, or the message; the answer dropped under a popup, a prompt or the help | the answer is ignored |
| AC7 | `crates/app/src/tui/clipboard.rs` :: `ac7_provider` (table) | environment → provider | the stub always picks OSC 52 |
| AC8 | `crates/app/src/tui/clipboard.rs` :: `ac8_copy` (table, fake runner) | programs and arguments run, the input given, the result per runner outcome | the stub runs nothing and fails |
| AC9 | `crates/app/src/tui/clipboard.rs` :: `ac9_read` (table, fake runner) + `crates/app/src/tui/clipboard.rs` :: `ac9_runner_cuts_and_times_out` (real `sh`) | programs run and the result per outcome; 64 KiB cut and the 2-second kill with a real process | the stub runs nothing and fails |
| AC10 | `crates/app/src/tui/clipboard.rs` :: `ac10_osc52` (table) | the sequence for a few links | the stub returns an empty sequence |
| AC11 | Checked at acceptance: the pages, the example and the README read against this spec, and `grep` finds neither name in the skipped lists | no test: prose | — |

Manual checks (hands-on, live Tidal, tmux 150 × 42, Wayland): *Copy link* on an album, then `wl-paste` prints its link; `wl-copy` an artist link, `g p` opens the artist's page; `wl-copy` a track link, `g p` adds it to the queue; `wl-copy` some text, `g p` says `Not a Tidal link: …`.

## Crate placement

- `tidal-player-core::item`: `Link` (the five kinds), `Link::url`, `parse_link`, sharing `parse_item`'s reading
- `tidal-player-core::ui`: `MenuAction::CopyLink` and the popups' lists (`popup`), `ActionKind::CopyLink` and `UiCommand::OpenSpotifyLinkFromClipboard` (`keymap`), the help texts (`help`), `Effect::Copy` / `Effect::ReadClipboard` and `Action::Copied` / `Action::ClipboardRead` (`mod`), what they do (a new `clipboard` module)
- `tidal-player` binary, `tui::clipboard`: the provider choice, the commands per provider, the messages, OSC 52 (pure), and the process runner behind a `Runner` trait with a fake; `main.rs` runs the effects in the TUI's loop and feeds the results back as the next frame's actions

## Facts vs. assumptions

Verified (2026-11-16, on the maintainer's machine, Sway, wl-clipboard 2.3.0, xclip 0.13, xsel 1.2.1; the clipboard itself left untouched):

- `wl-paste --no-newline` and `xclip -selection clipboard -o` exit with status 1 and a line on standard error when the clipboard has nothing in the type asked for (probed with a type nothing offers)
- `wl-copy` and `xclip` fork by default and serve the clipboard in the background (their man pages: `--foreground`, `-quiet` turn it off), so the process started exits soon while a child keeps the inherited standard output and error open
- spotify-player's source (`spotify_player/src/command.rs`, `event/mod.rs`, its README): `CopyLink` is an action on tracks, albums, artists and playlists; `OpenSpotifyLinkFromClipboard` is a command bound to `O`, which plays a track link and opens the page of a playlist, artist or album link. Both are in this keymap's skipped lists today (`crates/core/src/ui/keymap.rs`)

Assumed until the hands-on check:

- Mix links have the form `https://tidal.com/browse/mix/<id>` (the form 0011's mix IDs and Tidal's share menu use); the hands-on check opens one

## Decisions

1. **The command's name**: spotify-player's `OpenSpotifyLinkFromClipboard`, so its `keymap.toml` works as is (0008's rule: spotify-player's name where it has one). Lost: a name of our own (`OpenLinkFromClipboard`), clearer but a second name for the same thing
2. **Its default key**: `g p`. spotify-player's `O` is `PlayNextPrompt` here and moving that would break 0007's keys. Lost: no default key at all
3. **A track link from the clipboard**: added to the end of the queue, as `o` does. spotify-player starts playing it, replacing what plays; here nothing replaces the queue without `Enter` on a list. Lost: playing it at once; opening its album
4. **The clipboard tools**: the system's (`wl-clipboard`, `xclip`, `xsel`), run as processes, with OSC 52 when there is no display. Lost: a clipboard crate (`arboard`), a new dependency that on Wayland needs the window's focus a TUI does not have; a setting to force a provider (left out until someone needs it)
5. **Reading over SSH**: not offered; OSC 52's read is off in most terminals and answers through the terminal's input, which the TUI reads as keys. Lost: an OSC 52 read with a timeout

## Out of scope

- Copying the links of a selection (several lines), or of a page (the album or playlist shown) without a row under the cursor
- A link to the queue, *Collected Tracks*, a radio or a browse page
- Reading a link out of surrounding text
- A `clipboard` setting in `app.toml`, or a custom copy/paste command (spotify-player's `copy_command`/`paste_command`)
- `tidal-player playback` one-shot commands for links (#67)
