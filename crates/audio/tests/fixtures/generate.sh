#!/bin/sh
# Regenerates the audio fixtures (spec 0003 test plan). Tests never run this.
# Needs ffmpeg and python3. Run from this directory.
set -eu
T=$(mktemp -d)
trap 'rm -r "$T"' EXIT

# 16-bit 44.1 kHz stereo FLAC, 1 s (440 Hz left, 660 Hz right), with a seek
# point every 4096 frames (every FLAC frame) for the single-file seek test.
ffmpeg -nostdin -y -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" -f lavfi -i "sine=frequency=660:sample_rate=44100:duration=1" -filter_complex amerge -sample_fmt s16 -c:a flac "$T/flac16_44.flac"
python3 fixture_tool.py seektable "$T/flac16_44.flac" flac16_44.flac 4608
# Its first 1024 frames as raw PCM (ffmpeg's decode), for left-justification.
ffmpeg -nostdin -y -loglevel error -i flac16_44.flac -frames:a 1 -f s16le -ac 2 "$T/pcm16" && head -c 4096 "$T/pcm16" > flac16_44.s16le

# 16-bit 44.1 kHz mono FLAC, 0.5 s, 550 Hz.
ffmpeg -nostdin -y -loglevel error -f lavfi -i "sine=frequency=550:sample_rate=44100:duration=0.5" -ac 1 -sample_fmt s16 -c:a flac "$T/mono16_44.flac"
python3 fixture_tool.py strip-padding "$T/mono16_44.flac" mono16_44.flac
ffmpeg -nostdin -y -loglevel error -i mono16_44.flac -f s16le -ac 1 "$T/pcm16m" && head -c 2048 "$T/pcm16m" > mono16_44.s16le

# 24-bit 96 kHz stereo FLAC, 0.6 s (the plain-FLAC reference for the fMP4).
ffmpeg -nostdin -y -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=96000:duration=0.6" -f lavfi -i "sine=frequency=660:sample_rate=96000:duration=0.6" -filter_complex amerge -sample_fmt s32 -bits_per_raw_sample 24 -c:a flac "$T/flac24_96.flac"
python3 fixture_tool.py strip-padding "$T/flac24_96.flac" flac24_96.flac
ffmpeg -nostdin -y -loglevel error -i flac24_96.flac -f s24le -ac 2 "$T/pcm24" && head -c 6144 "$T/pcm24" > flac24_96.s24le

# The same FLAC stream in fragmented MP4 (stream copy: same PCM), ~0.25 s
# fragments, split into an init segment and media segments like Tidal's DASH.
ffmpeg -nostdin -y -loglevel error -i flac24_96.flac -c:a copy -f mp4 -movflags frag_keyframe+empty_moov+default_base_moof -frag_duration 250000 -strict -2 "$T/flac24_96.mp4"
python3 fixture_tool.py split-fmp4 "$T/flac24_96.mp4" flac24_96_dash

# AAC-LC in MP4 (moov first, like Tidal's BTS streams), 0.6 s.
ffmpeg -nostdin -y -loglevel error -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=0.6" -f lavfi -i "sine=frequency=660:sample_rate=44100:duration=0.6" -filter_complex amerge -c:a aac -b:a 128k -movflags +faststart aac_lc.m4a

# HE-AAC: ffmpeg's native encoder cannot do SBR and libfdk_aac is not in this
# ffmpeg, so the AAC-LC file's AudioSpecificConfig is patched to signal SBR
# explicitly (what an mp4a.40.5 stream carries). Only the decoder setup matters.
python3 fixture_tool.py he-aac aac_lc.m4a he_aac.m4a
