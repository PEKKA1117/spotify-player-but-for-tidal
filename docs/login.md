# Logging in

`tidal-player` signs in to Tidal with the OAuth2 device flow: you approve a short code on a Tidal web page, and the player never sees your password. Design: [spec 0002](specs/0002-auth.md).

## Commands

### `tidal-player login`

Prints a link and a code, and tries to open the link in your browser:

```
To log in, open this link and approve the code:

  https://link.tidal.com

Code: ABCDE   (expires in 5 min)
Waiting for approval… (Ctrl-C to cancel)
```

Open the link (if the browser did not open by itself), enter the code and approve. The player then prints `Logged in as user <id> (<country>)` and exits 0. A new login replaces any stored session.

- The code expires after a few minutes: `Login code expired, run "tidal-player login" again` (exit 1)
- You deny it on the Tidal page: `Login was denied` (exit 1)
- Ctrl-C cancels: nothing is stored, exit 130
- `Warning: Tidal did not accept the PKCE client: this session streams CD-quality tracks as AAC instead of FLAC`: the login worked, but switching the session to the client that gets 16-bit FLAC failed, so it keeps the login's own token; hi-res tracks are not affected. Logging in again may fix it

Running plain `tidal-player` without a stored session prints `Not logged in.` and runs the same flow before the TUI starts.

### `tidal-player logout`

Deletes the stored session from every store on this machine and prints `Logged out` (or `Not logged in` when there was none). It exits 0 either way and never asks for a passphrase, so it also clears a session whose passphrase you forgot.

`logout` cannot revoke the login on Tidal's side: Tidal refuses revocation for this client. The login stays valid server-side until Tidal expires it, but no copy of it remains on this machine.

### `tidal-player daemon`

Never starts a login. Without a stored session it exits 1 with `Not logged in: run "tidal-player login"`. (Daemon mode itself is not implemented yet, spec 0005.)

## Where the session is stored

1. The system keyring (Secret Service), service `tidal-player`, account `session`.
2. If there is no Secret Service (no D-Bus session bus, no provider running, locked), an age-encrypted file: `$XDG_STATE_HOME/tidal-player/session.age`, by default `~/.local/state/tidal-player/session.age`. It is created with mode `0600` in a `0700` directory.

Reads look in the keyring first, then the file; `logout` deletes from both.

## The passphrase

The encrypted file needs a passphrase. The player asks for it once per process and keeps it in memory. It looks for it in this order:

1. `TIDAL_PLAYER_PASSPHRASE_FILE`: path to a file whose first line is the passphrase (a warning is printed if other users can read it)
2. A systemd credential named `tidal-player-passphrase`, i.e. `$CREDENTIALS_DIRECTORY/tidal-player-passphrase`
3. An interactive prompt (`Passphrase for the tidal-player session:`), only when stdin and stderr are terminals. A new file asks twice; a wrong passphrase re-prompts up to 3 times

There is deliberately no environment variable holding the passphrase itself: environment variables leak through `/proc/<pid>/environ` and into child processes.

### Giving the passphrase to the daemon

A daemon has no terminal, so use a file or a systemd credential. Without either, it exits 1 with `Session file is encrypted and no passphrase is available: set TIDAL_PLAYER_PASSPHRASE_FILE or a systemd credential`. With an out-of-date passphrase it exits 1 with `Wrong passphrase for the session file` (it never overwrites the file).

```sh
install -m 600 /dev/null ~/.config/tidal-player-passphrase
printf '%s\n' 'my passphrase' > ~/.config/tidal-player-passphrase
TIDAL_PLAYER_PASSPHRASE_FILE=~/.config/tidal-player-passphrase tidal-player daemon
```

In a systemd user unit, use a credential instead:

```ini
[Service]
LoadCredential=tidal-player-passphrase:%h/.config/tidal-player-passphrase
```

(`LoadCredentialEncrypted=` works the same way. The unit file itself is part of spec 0005.)

## Environment variables

| Variable | Effect |
|---|---|
| `TIDAL_PLAYER_PASSPHRASE_FILE` | Passphrase file for the encrypted session file (see above) |
| `TIDAL_PLAYER_STATE_DIR` | Replaces the state directory; the session file is `<dir>/session.age` |
| `TIDAL_PLAYER_NO_KEYRING=1` | Skips the keyring entirely and uses only the encrypted file (for machines without a Secret Service) |

## "Session expired"

Tidal can reject the stored refresh token (for example after a long time unused, or when the login is invalidated on Tidal's side). The player then cannot refresh and shows `Session expired — run "tidal-player login" in another terminal`; API calls fail until you log in again.

To recover, run `tidal-player login` from any terminal, even while the TUI or daemon is running. Within a few seconds the running process notices the new session in storage, clears the message and carries on; you do not need to restart it.

Short network failures or Tidal server errors are not "session expired": the player keeps the session and retries.
