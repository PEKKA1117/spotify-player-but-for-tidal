# Stream resolution fixtures (spec 0003, AC1–AC5)

Written from the shapes recorded by the live probes in spec 0003 "Facts vs.
assumptions" (`scripts/tidal-playback-probe.sh`, `scripts/tidal-playback-probe2.sh`,
2026-10-07). Every URL, token, signature and track ID is a fake.

| File | Source |
|---|---|
| `manifest_hires_24_96.mpd` | The recorded hi-res MPD (24/96, `<S d="380928" r="72"/><S d="129030"/>`, `PT4M51.008S`), URLs replaced |
| `manifest_lossless_16_44.mpd` | The recorded 16/44.1 MPD from probe 2 (`<S d="176128" r="55"/><S d="52930"/>`, `PT3M44.854S`), with CloudFront-style `Policy`/`Signature`/`Key-Pair-Id` query parameters escaped as `&amp;`. The `bandwidth` value is made up (the probe did not record it) |
| `manifest_high_aac.bts.json` | The recorded BTS manifest of a `HIGH` grant (AAC-LC) |
| `manifest_lossless_flac.bts.json` | **Not observed live**: a BTS manifest for FLAC, as Tidal sent it before 2026 and may still send (spec 0003, decision 6) |
| `playbackinfo_*.json` | A `playbackinfopostpaywall` response with the recorded keys, whose `manifest` is the base64 of the matching `manifest_*` file. `bitDepth`/`sampleRate` only on lossless grants. ReplayGain values are made up |
| `error_not_available_4005.json`, `error_not_found_999.json` | The recorded error bodies |
| `error_server_500.json` | A `500` with a `subStatus` other than `999` (made up) |

The `playbackinfo_*.json` files were generated with:

```python
import base64, json
def info(name, track, quality, mime, manifest_file, bits=None, rate=None):
    m = open(manifest_file, "rb").read()
    d = {"trackId": track, "assetPresentation": "FULL", "audioMode": "STEREO",
         "audioQuality": quality, "manifestMimeType": mime, "manifestHash": "FAKE-HASH",
         "manifest": base64.b64encode(m).decode(), "albumReplayGain": -9.5,
         "albumPeakAmplitude": 0.988, "trackReplayGain": -9.1, "trackPeakAmplitude": 0.977}
    if bits is not None:
        d["bitDepth"] = bits
        d["sampleRate"] = rate
    open(name, "w").write(json.dumps(d) + "\n")

info("playbackinfo_hires_dash.json", 1001, "HI_RES_LOSSLESS", "application/dash+xml", "manifest_hires_24_96.mpd", 24, 96000)
info("playbackinfo_lossless_dash.json", 1002, "LOSSLESS", "application/dash+xml", "manifest_lossless_16_44.mpd", 16, 44100)
info("playbackinfo_high_bts.json", 1002, "HIGH", "application/vnd.tidal.bts", "manifest_high_aac.bts.json")
info("playbackinfo_lossless_bts_flac.json", 1003, "LOSSLESS", "application/vnd.tidal.bts", "manifest_lossless_flac.bts.json", 16, 44100)
```
