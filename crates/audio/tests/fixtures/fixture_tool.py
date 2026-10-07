#!/usr/bin/env python3
"""Post-processing for the audio test fixtures (see README.md).

  fixture_tool.py seektable IN.flac OUT.flac FRAMES_PER_POINT
      Drop PADDING blocks and insert a SEEKTABLE with one seek point every
      FRAMES_PER_POINT frames (at FLAC frame boundaries), so that a seek can
      jump straight to a byte offset (spec 0003 AC19, single-file row).
  fixture_tool.py strip-padding IN.flac OUT.flac
      Drop PADDING blocks (keeps the fixtures small).
  fixture_tool.py split-fmp4 IN.mp4 OUTPREFIX
      Split a fragmented MP4 into an init segment (ftyp + moov) and media
      segments (each moof + mdat pair), like Tidal's DASH segments. The mfra
      trailer is dropped. Writes OUTPREFIX-init.mp4, OUTPREFIX-N.m4s (N from 1)
      and OUTPREFIX-segments.txt ("timescale T", then "N start duration" per
      segment, in timescale ticks, from the trun/tfhd/trex sample durations).
  fixture_tool.py he-aac IN.m4a OUT.m4a
      Turn ffmpeg's AAC-LC AudioSpecificConfig (LC + sync extension 0x2b7,
      SBR object type, sbrPresentFlag 0) into explicit backward-compatible
      HE-AAC signalling: sbrPresentFlag 1 and an extension sampling frequency
      index, in the padding bits of the same 5 bytes (no size change).
"""
import struct
import sys


def flac_blocks(data):
    assert data[:4] == b"fLaC"
    i = 4
    blocks = []
    while True:
        h = data[i]
        length = int.from_bytes(data[i + 1 : i + 4], "big")
        blocks.append((h & 0x7F, data[i + 4 : i + 4 + length]))
        i += 4 + length
        if h & 0x80:
            return blocks, i


def write_flac(blocks, frames, path):
    out = bytearray(b"fLaC")
    for n, (t, body) in enumerate(blocks):
        last = 0x80 if n == len(blocks) - 1 else 0
        out += bytes([last | t]) + len(body).to_bytes(3, "big") + body
    out += frames
    open(path, "wb").write(out)


def crc8(data):
    crc = 0
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = ((crc << 1) ^ 0x07) & 0xFF if crc & 0x80 else (crc << 1) & 0xFF
    return crc


def frame_offsets(frames):
    """Byte offsets (relative to the first frame) of fixed-blocksize frames."""
    offsets = []
    i = 0
    while i + 16 < len(frames):
        if frames[i] == 0xFF and frames[i + 1] == 0xF8:
            # Header: 4 fixed bytes, UTF-8 frame number, optional size/rate bytes, CRC-8.
            j = i + 4
            first = frames[j]
            n = 0
            while first & (0x80 >> n):
                n += 1
            j += max(n, 1)
            bs_code = frames[i + 2] >> 4
            sr_code = frames[i + 2] & 0x0F
            j += {6: 1, 7: 2}.get(bs_code, 0)
            j += {12: 1, 13: 2, 14: 2}.get(sr_code, 0)
            if crc8(frames[i:j]) == frames[j]:
                offsets.append(i)
                i = j
                continue
        i += 1
    return offsets


def seektable(src, dst, every):
    data = open(src, "rb").read()
    blocks, start = flac_blocks(data)
    frames = data[start:]
    info = blocks[0][1]
    block_size = struct.unpack(">H", info[2:4])[0]
    assert block_size == struct.unpack(">H", info[0:2])[0], "fixed block size expected"
    points = b""
    for n, off in enumerate(frame_offsets(frames)):
        sample = n * block_size
        if sample % every == 0:
            points += struct.pack(">QQH", sample, off, block_size)
    blocks = [b for b in blocks if b[0] not in (1, 3)]
    blocks.insert(1, (3, points))
    write_flac(blocks, frames, dst)


def strip_padding(src, dst):
    data = open(src, "rb").read()
    blocks, start = flac_blocks(data)
    write_flac([b for b in blocks if b[0] != 1], data[start:], dst)


def boxes(data, start=0, end=None):
    end = len(data) if end is None else end
    i = start
    while i < end:
        size, kind = struct.unpack(">I4s", data[i : i + 8])
        yield kind.decode(), i, size
        i += size


def find(data, path, start=0, end=None):
    for kind, i, size in boxes(data, start, end):
        if kind == path[0]:
            if len(path) == 1:
                return i, size
            return find(data, path[1:], i + 8, i + size)
    return None


def split_fmp4(src, prefix):
    data = open(src, "rb").read()
    top = list(boxes(data))
    moov_i, moov_size = find(data, ["moov"])
    mdhd_i, _ = find(data, ["moov", "trak", "mdia", "mdhd"])
    version = data[mdhd_i + 8]
    timescale = struct.unpack(">I", data[mdhd_i + (28 if version == 1 else 20) : mdhd_i + (32 if version == 1 else 24)])[0]
    trex = find(data, ["moov", "mvex", "trex"])
    trex_default = struct.unpack(">I", data[trex[0] + 20 : trex[0] + 24])[0] if trex else 0
    init_end = moov_i + moov_size
    open(prefix + "-init.mp4", "wb").write(data[:init_end])
    lines = [f"timescale {timescale}"]
    start = 0
    n = 0
    for k, (kind, i, size) in enumerate(top):
        if kind != "moof":
            continue
        mdat_kind, mdat_i, mdat_size = top[k + 1]
        assert mdat_kind == "mdat"
        n += 1
        open(f"{prefix}-{n}.m4s", "wb").write(data[i : mdat_i + mdat_size])
        tfhd_i, _ = find(data, ["traf", "tfhd"], i + 8, i + size)
        tf_flags = int.from_bytes(data[tfhd_i + 9 : tfhd_i + 12], "big")
        j = tfhd_i + 16
        if tf_flags & 0x01:
            j += 8
        if tf_flags & 0x02:
            j += 4
        default = trex_default
        if tf_flags & 0x08:
            default = struct.unpack(">I", data[j : j + 4])[0]
        trun_i, _ = find(data, ["traf", "trun"], i + 8, i + size)
        tr_flags = int.from_bytes(data[trun_i + 9 : trun_i + 12], "big")
        count = struct.unpack(">I", data[trun_i + 12 : trun_i + 16])[0]
        j = trun_i + 16
        if tr_flags & 0x01:
            j += 4
        if tr_flags & 0x04:
            j += 4
        per = 4 * sum(1 for f in (0x100, 0x200, 0x400, 0x800) if tr_flags & f)
        duration = 0
        for s in range(count):
            if tr_flags & 0x100:
                duration += struct.unpack(">I", data[j + s * per : j + s * per + 4])[0]
            else:
                duration += default
        lines.append(f"{n} {start} {duration}")
        start += duration
    open(prefix + "-segments.txt", "w").write("\n".join(lines) + "\n")


def he_aac(src, dst):
    data = bytearray(open(src, "rb").read())
    # DecoderSpecificInfo tag 5, length 5: 2 bytes LC config + 0x2b7 sync
    # extension (11 bits), object type 5 (SBR), sbrPresentFlag 0, padding.
    k = data.find(bytes.fromhex("0580808005"))
    assert k > 0 and data[k + 7 : k + 9] == bytes.fromhex("56e5") and data[k + 9] == 0
    # sbrPresentFlag 1, extensionSamplingFrequencyIndex 3 (48 kHz), 3 bits padding.
    data[k + 9] = 0b1_0011_000
    open(dst, "wb").write(data)


if __name__ == "__main__":
    cmd, *args = sys.argv[1:]
    {"seektable": lambda a, b, c: seektable(a, b, int(c)), "strip-padding": strip_padding, "split-fmp4": split_fmp4, "he-aac": he_aac}[cmd](*args)
