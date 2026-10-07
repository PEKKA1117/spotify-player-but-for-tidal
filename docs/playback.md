# Playing tracks

`tidal-player` decodes Tidal's streams itself (FLAC and AAC, in pure Rust) and writes them to an ALSA device: through the system mixer by default, or straight to your DAC, bit-perfect, when you ask for it. Design: [spec 0003](specs/0003-playback-engine.md).

Until the queue and the TUI controls exist (specs 0004 and 0005), playback is driven from the command line. Both commands below stay available after that.

## Commands

### `tidal-player play <track-id> [--quality Q] [--device PCM] [--start SECONDS]`

Plays one track in the foreground, without the TUI, and exits when it ends. It needs a stored session (see [Logging in](login.md)); without one it exits 1 with `Not logged in: run "tidal-player login"`.

The track ID is the number in a Tidal track link (`https://tidal.com/browse/track/77640617` → `77640617`).

```
$ tidal-player play 77640617 --device hw:1,0
Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo
Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect
  1:23 / 4:56
```

- **Track** line: the quality Tidal **granted** (it may be lower than you asked: Tidal decides per track and per account), and the format the decoder found
- **Output** line: the device actually opened, its kind (below), the sample format and rate it runs at, then `bit-perfect` or why it is not
- The progress line redraws in place, only when stdout is a terminal (nothing is printed when you pipe the output). It shows `?:??` as the duration when the stream does not say
- `--start 90` starts 90 seconds in

Exit codes: `0` the track played to its end; `1` an error (one line on stderr, see [Errors](#errors)); `2` a bad argument or setting; `130` Ctrl-C (the device is closed and released first).

### `tidal-player devices`

Lists the devices `--device` accepts: `default` first, then every ALSA hardware device (`hw:CARD,DEVICE`) that can play, with the card's name. The device `play` would use is marked `*`:

```
$ tidal-player devices
* default  shared, through the system mixer
  hw:0,0   HDA Intel PCH: ALC892 Analog
  hw:0,1   HDA Intel PCH: ALC892 Digital
  hw:1,0   E30 II: USB Audio
```

It reads `/proc/asound/cards` and `/proc/asound/pcm`; capture-only devices (microphones) are not listed.

## Settings

| Setting | Flag | Environment | Default |
|---|---|---|---|
| Highest quality to ask for | `--quality` (`hi-res`, `lossless`, `high`) | `TIDAL_PLAYER_QUALITY` | `hi-res` |
| Output device | `--device` (any ALSA PCM name) | `TIDAL_PLAYER_DEVICE` | `default` |

A flag beats the environment, which beats the default. An empty variable counts as unset. An unknown quality exits 2. `low` is refused too: Tidal's `LOW` streams are HE-AAC, which the player cannot decode; `high` (AAC 320 kbit/s) is the lowest setting.

The quality is the **highest** to ask for. Tidal answers with what the track and your subscription allow: a CD-quality track asked at `hi-res` comes as `LOSSLESS` (FLAC 16-bit 44.1 kHz), and some tracks only exist as `HIGH`.

To make a DAC the default, put `export TIDAL_PLAYER_DEVICE=hw:1,0` in your shell profile (a config file comes with spec 0008).

## Output kinds and bit-perfect

| `--device` | Kind | What happens |
|---|---|---|
| `hw:C,D` (or `hw:C`) | **exclusive** | The card is opened directly. The player asks PipeWire/PulseAudio to release it first (`org.freedesktop.ReserveDevice1`) and holds it while the track plays; other applications cannot use the card meanwhile. The device runs at the track's exact rate, with no resampling |
| `hw:C,D` that refuses the track's format | **fallback** | Reopened as `plughw:C,D`: ALSA converts the rate or format. The Output line says why, e.g. `resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)` |
| `plughw:…` | **fallback** | As above, chosen by you |
| `default` or any other name | **shared** | Through the system mixer (PipeWire, PulseAudio, dmix), alongside other applications. Never bit-perfect |

**Bit-perfect** means the samples in the file reach the DAC unchanged. The Output line says `bit-perfect` only when all of these hold:

- the kind is exclusive (`hw:`),
- the track is lossless (FLAC; AAC is `lossy source`),
- the device runs at the track's sample rate and in stereo (mono tracks are sent as identical left and right channels, which stays bit-perfect),
- the sample format holds at least the track's bits (a 16-bit track in `S32_LE` is padded with zeros, which is still bit-perfect; a 24-bit track in `S16_LE` is not).

Otherwise it gives the reason: `shared (system mixer)`, `lossy source`, `plughw (ALSA may convert the samples)`, `resampled (plughw fallback: …)`, `device runs at 48 kHz, source is 44.1 kHz`, `24-bit source truncated to S16_LE`, ….

To check it while a track plays, read the card's hardware parameters; format and rate must match the Output line:

```sh
cat /proc/asound/card1/pcm0p/sub0/hw_params
```

### Finding your DAC's device

1. Plug the DAC in and run `tidal-player devices`. It shows up as a new card, usually named after the DAC (`hw:1,0   E30 II: USB Audio`). Most USB DACs have one device, `,0`
2. Try it: `tidal-player play <track-id> --device hw:1,0`. The Output line should say `(exclusive)` and `bit-perfect` for a FLAC track
3. Keep it: `export TIDAL_PLAYER_DEVICE=hw:1,0`

Card numbers can change when you plug devices in a different order. ALSA also accepts the card's ID instead of its number (`hw:CARD=DAC,DEV=0`, the ID is the name in brackets in `/proc/asound/cards`); the `devices` list shows numbers.

## Errors

| Message | What to do |
|---|---|
| `Not logged in: run "tidal-player login"` | Log in first ([Logging in](login.md)) |
| `Session expired: run "tidal-player login"` | Tidal no longer accepts the stored login: log in again |
| `Track 123 is only available as a preview for this account` | Your subscription or country only gets a 30-second preview of it |
| `Track 123 is not available in NO` | Tidal does not offer this track to your account or country |
| `Track 123 was not found, or cannot be streamed in NO` | Check the track ID; Tidal answers the same for a track that is not licensed in your country |
| `Track 123 is not playable: …` | The stream is in a form the player cannot decode (encrypted, Dolby Atmos / Sony 360, HE-AAC). Nothing is played rather than noise |
| `Output hw:1,0 is busy (used by …): close it, or use --device default` | Another application holds the card, or refused to release it. Close it, or play through the mixer with `--device default`. The player never silently switches to another device |
| `No such output device hw:5,0: see "tidal-player devices"` | Typo or unplugged device: pick a name from `tidal-player devices` |
| `Output hw:1,0 was lost` | The DAC was unplugged (or failed) while playing. The device and its reservation are released |
| `Network error while streaming track 123` | The connection dropped and did not come back after retries (0.5 s, 1 s and 2 s). Short drops are covered by the read-ahead buffer and the retries without you noticing |
| `this build has no ALSA output` | This binary was built without the `alsa` feature: rebuild with the default features |

Stream links expire after a while (a long pause, a seek much later). The player then asks Tidal for a fresh link once and continues where it was; you see nothing.
