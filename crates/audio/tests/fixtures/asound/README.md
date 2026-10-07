# `/proc/asound` fixtures (spec 0003 AC25)

Each directory holds a `cards` and a `pcm` file in the shape the kernel writes
to `/proc/asound/cards` and `/proc/asound/pcm` (hand-written; card names are
examples). `tidal-player devices` reads them from `TIDAL_PLAYER_ASOUND_DIR`
when that is set.

| Directory | What it covers |
|---|---|
| `no_cards` | no sound card at all (`--- no soundcards ---`, empty `pcm`) |
| `onboard` | one onboard card: two playback devices, one capture-only |
| `onboard_usb` | onboard card + a USB DAC (card 1) |
| `capture_only` | onboard card + a USB microphone with no playback stream (not listed) |
