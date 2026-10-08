English | [繁體中文](zh-TW/daemon.md)

# The daemon and clients

`tidal-player` can run as a **player** on its own (`tidal-player daemon`, for example as a systemd user service) and be controlled by any number of **clients**: TUIs, one-shot commands such as `tidal-player playback next`, or a second `tidal-player --add-to-queue <link>`. Design: [spec 0005](specs/0005-daemon-and-clients.md).

## Players and clients

A **player** plays: it holds the queue, the modes, the session and the audio device. A **client** only sends commands to a player and shows what it sends back. There is at most one player per user at a time.

| You run | No player running | A player running |
|---|---|---|
| `tidal-player [ITEM]...` | Becomes the player and shows the [TUI](tui.md) (standalone) | **Attaches** a TUI to it; with items, first sends them to it (below) |
| `tidal-player daemon` | Becomes the player, headless | Exits 3: `Another player is running (pid N)` |
| `tidal-player play ITEM...` | Becomes the player, headless, in the foreground ([`play`](playback.md#commands)) | Exits 3: `Another player is running (pid N): use "tidal-player playback load"` |
| `tidal-player playback …` | Exits 1: `No player is running: start "tidal-player" or "tidal-player daemon"` | Sends one command, prints the answer, exits |
| `tidal-player daemon stop` | Exits 1, same message | Asks the player to shut down and waits until it is gone (at most 5 s) |

Every player (the standalone TUI, `daemon` and `play`) accepts clients, so `playback` and attached TUIs work with any of them.

**Quitting** (`q`) an attached TUI only detaches it: the player keeps playing. Quitting the TUI of a standalone player stops that player, as before; its other clients show `The player shut down: waiting for it to come back…` and attach to the next player that starts.

### Attaching a TUI

`tidal-player` looks for a running player first. If it finds one, the TUI attaches to it instead of starting a second player. With items, they are sent to the player before the TUI opens:

| Command | The player |
|---|---|
| `tidal-player ITEM...` | replaces its queue with the items and plays the first |
| `tidal-player --add-to-queue ITEM...` | adds them at the end of its queue |
| `tidal-player --play-next ITEM...` | adds them right after the current track |

When the player had nothing playing (an empty queue, or stopped with no current track), the first added track starts. The player fetches the items (`Album 123 was not found` shows in the playback window); the client needs no login of its own: it never reads the session, the keyring or the passphrase, and never opens the audio device.

An attached TUI looks and works like the standalone one ([The TUI](tui.md)), with its own keys and TUI settings: it reads its own `keymap.toml` and `app.toml` (volume and seek steps, page sizes, library layout), not the daemon's (see [Configuration](config.md)). It shows only what the player sends: an `o` add appears in the queue once the player has added it. If the connection is lost, the last screen stays, the message row says `Disconnected from the player: reconnecting…` (or `The player shut down: waiting for it to come back…`), the playback keys do nothing, and the TUI tries again every second. When it gets back in, the screen shows the player as it is now. The cursor keys and `q` work all the while.

## Running the daemon under systemd

`tidal-player daemon unit` prints a systemd user unit that starts the daemon from wherever this `tidal-player` is installed. Install and start it:

```sh
mkdir -p ~/.config/systemd/user
tidal-player daemon unit > ~/.config/systemd/user/tidal-player.service
systemctl --user daemon-reload
systemctl --user enable --now tidal-player
```

The program never writes the unit or runs `systemctl` itself. The unit looks like this:

```ini
[Unit]
Description=tidal-player daemon
Documentation=https://github.com/PEKKA1117/spotify-player-but-for-tidal/blob/main/docs/daemon.md
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=notify
ExecStart=/home/you/.cargo/bin/tidal-player daemon
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

- **Settings** come from [`app.toml`](config.md#apptoml), as for the TUI: `~/.config/tidal-player/app.toml` (for another directory, `ExecStart=… daemon -c DIR`). The environment still works and beats the file: uncomment and edit the `Environment=` line, one line per variable ([Settings](playback.md#settings)). The daemon reads the file once, at start: after changing it, `systemctl --user restart tidal-player`. A broken `app.toml` or `keymap.toml` stops it with exit 2 and the message in the journal
- **The passphrase**: a daemon cannot ask for one. If your session is in the encrypted file rather than the keyring, give it as a credential (uncomment the `LoadCredential=` line, see [Logging in](login.md#giving-the-passphrase-to-the-daemon))
- **Log in first** with `tidal-player login`. The daemon never starts a login
- **Exit codes 1, 2 and 3 are not restarted**: not logged in or an unusable session store (1), a bad setting (2), another player running (3). Trying again every few seconds would not fix any of them, so systemd leaves the unit failed. Fix the cause (`systemctl --user status tidal-player` and `journalctl --user -u tidal-player` show the message), then `systemctl --user restart tidal-player`. Any other failure is restarted after 5 s, at most 5 times in 5 minutes
- With a standalone TUI open, the service cannot start (exit 3, not restarted). Quit the TUI first, or attach the TUI to the daemon instead (`tidal-player` does that once the daemon runs)

The daemon tells systemd it is ready once clients can connect (`Type=notify`). On `systemctl --user stop` (`SIGTERM`), `SIGINT`, or `tidal-player daemon stop`, it tells its clients, stops playback, releases the device and exits 0. Messages go to stderr, which is the journal under systemd. The queue starts empty each time; remembering it across restarts comes later (spec 0009).

After `tidal-player login` in any terminal, a running daemon picks up the new session within a few seconds (see ["Session expired"](login.md#session-expired)).

## One-shot commands

`tidal-player playback <command>` sends one command to the running player and exits:

| Command | Does |
|---|---|
| `play-pause` | play/pause |
| `next`, `previous` | next / previous track (as `n` / `p` in the TUI) |
| `seek S`, `seek +S`, `seek -S` | seek to `S` seconds into the track, or `S` seconds forward / back (`seek 90`, `seek +5`, `seek -2.5`) |
| `volume N`, `volume +N`, `volume -N` | set the volume to `N` % (0–100), or change it by `N` points |
| `mute`, `shuffle`, `repeat`, `autoplay` | toggle mute, shuffle, autoplay; cycle repeat off → queue → track |
| `load ITEM...` | replace the queue with the [items](playback.md#items) and play the first |
| `add ITEM...`, `add --next ITEM...` | add the items at the end of the queue, or right after the current track |
| `status`, `status --json` | print what is playing |

It prints nothing and exits 0 when the player did it. Otherwise:

- a bad argument or item (`volume 101`, `seek x`, an artist link) exits 2 and sends nothing
- no player running exits 1 with `No player is running: start "tidal-player" or "tidal-player daemon"`
- the player's error (`Album 123 was not found`) is printed on stderr, exit 1
- no answer within 5 s: `The player did not answer`, exit 1

`status` prints three lines, with the TUI's symbols and modes:

```
$ tidal-player playback status
▶ Hell Above · Pierce The Veil · Collide With The Sky
1:23 / 3:32 · shuffle · repeat: queue · 80%
Queue: 2 of 12
```

`Nothing playing` alone when nothing is current. A fourth line shows the player's message, if any (`Output hw:1,0 is busy …`), or `Session expired: run "tidal-player login"`. While the device is released, the second line ends with `· device released`. `status --json` prints the player's whole state as one JSON line, for scripts.

`playback add` makes a simple handler for links: for example `tidal-player playback add --next "$1"`.

### Stopping the daemon

`tidal-player daemon stop` asks the player to shut down and returns once it is gone (exit 0), at most 5 s later. It works for any player, a standalone TUI included. Under systemd, prefer `systemctl --user stop tidal-player`: `daemon stop` makes the daemon exit 0, which systemd does not restart, but leaves the unit stopped until the next `start`.

## Releasing the device while paused

While paused, the player lets go of the audio device so other applications can use your DAC:

- After being paused for the **release delay** (10 s by default), it closes the device and, for an exclusive (`hw:`) output, gives the card back to PipeWire or PulseAudio. The track, its position and what was buffered are kept. The TUI's third row then ends with `· device released`, and `playback status` says so too
- When another application asks for the card (through PipeWire's device reservation) while paused, it is released at once, whatever the delay. While playing or loading the player refuses, as before
- **Resuming** reopens the device and continues exactly where it stopped: nothing is skipped or played twice. If the device is now busy or gone, the player stays paused with the message (`Output hw:1,0 is busy (used by …)`) and never skips; `Space` (or `playback play-pause`) tries again
- Seeking while released moves the position; the new position plays on resume

| Setting | `app.toml` | Environment | Accepted | Default |
|---|---|---|---|---|
| Release the device after pausing for (s) | `release_paused_secs` | `TIDAL_PLAYER_RELEASE_PAUSED` | integer 0–3600, or `never` | `10` |

`0` releases at once on pause; `never` keeps the device open while paused (another application asking for it still gets it). The setting applies to every player: the TUI, `play` and the daemon (`release_paused_secs = 0` in `app.toml`, or in the unit `Environment=TIDAL_PLAYER_RELEASE_PAUSED=0`).

## Where clients find the player

Players and clients meet in the **runtime directory**: `$TIDAL_PLAYER_RUNTIME_DIR` if set, else `$XDG_RUNTIME_DIR/tidal-player`, else `/tmp/tidal-player-<uid>`. It holds the player's socket (`player.sock`) and its lock (`player.lock`, with the player's pid in it). The directory is created private (mode `0700`); one that another user owns or others can read is refused, so no one else can reach your player.

`XDG_RUNTIME_DIR` is set by your login session. Where it is not (some SSH sessions, no logind), a shell and a daemon under systemd may look in different places and not find each other: set `XDG_RUNTIME_DIR=/run/user/$(id -u)` (or the same `TIDAL_PLAYER_RUNTIME_DIR`) in both.

## Troubleshooting

| Message | Meaning |
|---|---|
| `Another player is running (pid N)` | `daemon` or `play` found a player already running (a TUI, another daemon, a `play`). Use it as a client (`tidal-player`, `tidal-player playback …`), or stop it first (`tidal-player daemon stop`, or quit its TUI). Exit 3 |
| `The running player is tidal-player X, this is Y: restart it ("tidal-player daemon stop", or "systemctl --user restart tidal-player")` | The player is another version of the program, typically an old daemon after an upgrade. Restart it so both are the same version. A TUI shows this and does not retry |
| `A player is running (pid N) but not answering on <socket>` | A process holds the lock but nothing answered on the socket within 2 s: a player still starting (try again), or one that is stuck (`kill N`). Exit 1 |
| `Runtime directory <path> is not private: …` | The runtime directory is not a directory, belongs to another user, or others can access it. It is never changed for you: remove it, or fix its owner and mode (`chmod 700`). Exit 1 |
| `No player is running: start "tidal-player" or "tidal-player daemon"` | `playback` or `daemon stop` found no player. If one is running, check it uses the same runtime directory (above) |
| `Disconnected from the player: reconnecting…` | An attached TUI lost its player (it crashed, or was restarted); it reconnects by itself |
