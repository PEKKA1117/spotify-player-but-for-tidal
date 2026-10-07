//! Sample-format choice and packing (spec 0003 "Output (ALSA)", AC10, AC11).

use crate::sink::SampleFormat;

use SampleFormat::{S16Le, S24_3Le, S24Le, S32Le};

/// Preference for 16-bit sources (some DACs have a broken `S16_LE` endpoint).
const PREFER_16: &[SampleFormat] = &[S32Le, S16Le, S24_3Le, S24Le];
/// Preference for 24-bit (and deeper) sources.
const PREFER_24: &[SampleFormat] = &[S24_3Le, S24Le, S32Le];
/// Preference for lossy sources: keeps the most of the decoder's precision.
const PREFER_LOSSY: &[SampleFormat] = &[S32Le, S24_3Le, S24Le, S16Le];

/// The format preference list for a source with `source_bits` significant
/// bits (`None`: lossy).
pub fn preference(source_bits: Option<u16>) -> &'static [SampleFormat] {
    match source_bits {
        None => PREFER_LOSSY,
        Some(bits) if bits <= 16 => PREFER_16,
        Some(_) => PREFER_24,
    }
}

/// The first format of the preference list for `source_bits` that the device
/// accepts, or `None` when it accepts none of them (AC10).
pub fn choose_format(source_bits: Option<u16>, accepted: &[SampleFormat]) -> Option<SampleFormat> {
    preference(source_bits)
        .iter()
        .copied()
        .find(|format| accepted.contains(format))
}

/// Bytes one sample takes in `format`.
pub fn bytes_per_sample(format: SampleFormat) -> usize {
    match format {
        S16Le => 2,
        S24_3Le => 3,
        S24Le | S32Le => 4,
    }
}

/// Significant bits `format` holds.
pub fn format_bits(format: SampleFormat) -> u16 {
    match format {
        S16Le => 16,
        S24_3Le | S24Le => 24,
        S32Le => 32,
    }
}

/// ALSA's name for `format` (`S24_3LE`, …), as shown to the user.
pub fn format_name(format: SampleFormat) -> &'static str {
    match format {
        S16Le => "S16_LE",
        S24Le => "S24_LE",
        S24_3Le => "S24_3LE",
        S32Le => "S32_LE",
    }
}

/// Pack left-justified `i32` samples into `format`'s bytes, dropping low bits
/// only (AC11). Appends to `out`.
pub fn pack_into(samples: &[i32], format: SampleFormat, out: &mut Vec<u8>) {
    out.reserve(samples.len() * bytes_per_sample(format));
    match format {
        S16Le => out.extend(
            samples
                .iter()
                .flat_map(|&s| ((s >> 16) as i16).to_le_bytes()),
        ),
        // Arithmetic shift: the 4-byte word is sign-extended.
        S24Le => out.extend(samples.iter().flat_map(|&s| (s >> 8).to_le_bytes())),
        S24_3Le => {
            for &s in samples {
                let [b0, b1, b2, _] = (s >> 8).to_le_bytes();
                out.extend([b0, b1, b2]);
            }
        }
        S32Le => out.extend(samples.iter().flat_map(|&s| s.to_le_bytes())),
    }
}

/// Pack left-justified `i32` samples into `format`'s bytes (AC11).
pub fn pack(samples: &[i32], format: SampleFormat) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * bytes_per_sample(format));
    pack_into(samples, format, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SampleFormat; 4] = [S16Le, S24Le, S24_3Le, S32Le];

    /// The lists as written in spec 0003 "Output (ALSA)".
    const SPEC_16: [SampleFormat; 4] = [S32Le, S16Le, S24_3Le, S24Le];
    const SPEC_24: [SampleFormat; 3] = [S24_3Le, S24Le, S32Le];
    const SPEC_LOSSY: [SampleFormat; 4] = [S32Le, S24_3Le, S24Le, S16Le];

    fn subsets() -> impl Iterator<Item = Vec<SampleFormat>> {
        (0u8..16).map(|mask| {
            ALL.iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, f)| *f)
                .collect()
        })
    }

    #[test]
    fn ac10_choose_format() {
        // Hand-picked rows, then every accepted subset for each source kind.
        let rows: &[(Option<u16>, &[SampleFormat], Option<SampleFormat>)] = &[
            (Some(16), &ALL, Some(S32Le)),
            (Some(16), &[S16Le, S24Le], Some(S16Le)),
            (Some(16), &[S24Le, S24_3Le], Some(S24_3Le)),
            (Some(16), &[S24Le], Some(S24Le)),
            (Some(16), &[], None),
            (Some(24), &ALL, Some(S24_3Le)),
            (Some(24), &[S32Le, S24Le], Some(S24Le)),
            (Some(24), &[S32Le, S16Le], Some(S32Le)),
            (Some(24), &[S16Le], None),
            (Some(24), &[], None),
            (None, &ALL, Some(S32Le)),
            (None, &[S16Le, S24Le], Some(S24Le)),
            (None, &[S16Le], Some(S16Le)),
            (None, &[], None),
        ];
        for (bits, accepted, want) in rows {
            assert_eq!(
                choose_format(*bits, accepted),
                *want,
                "bits {bits:?}, accepted {accepted:?}"
            );
        }
        for (bits, list) in [
            (Some(16), &SPEC_16[..]),
            (Some(24), &SPEC_24[..]),
            (None, &SPEC_LOSSY[..]),
        ] {
            for accepted in subsets() {
                let want = list.iter().copied().find(|f| accepted.contains(f));
                assert_eq!(
                    choose_format(bits, &accepted),
                    want,
                    "bits {bits:?}, accepted {accepted:?}"
                );
            }
        }
    }

    #[test]
    fn ac11_pack() {
        // (source sample, left-justified as the decoder gives it), then the
        // expected bytes in S16_LE, S24_LE, S24_3LE, S32_LE.
        type Row = (&'static str, i32, [&'static [u8]; 4]);
        let s16 = |s: i32| s << 16;
        let s24 = |s: i32| s << 8;
        #[rustfmt::skip]
        let rows: &[Row] = &[
            ("16-bit 1", s16(1), [&[0x01, 0x00], &[0x00, 0x01, 0x00, 0x00], &[0x00, 0x01, 0x00], &[0x00, 0x00, 0x01, 0x00]]),
            ("16-bit -1", s16(-1), [&[0xFF, 0xFF], &[0x00, 0xFF, 0xFF, 0xFF], &[0x00, 0xFF, 0xFF], &[0x00, 0x00, 0xFF, 0xFF]]),
            ("16-bit max", s16(32_767), [&[0xFF, 0x7F], &[0x00, 0xFF, 0x7F, 0x00], &[0x00, 0xFF, 0x7F], &[0x00, 0x00, 0xFF, 0x7F]]),
            ("16-bit min", s16(-32_768), [&[0x00, 0x80], &[0x00, 0x00, 0x80, 0xFF], &[0x00, 0x00, 0x80], &[0x00, 0x00, 0x00, 0x80]]),
            ("24-bit 0x123456", s24(0x12_3456), [&[0x34, 0x12], &[0x56, 0x34, 0x12, 0x00], &[0x56, 0x34, 0x12], &[0x00, 0x56, 0x34, 0x12]]),
            ("24-bit -2", s24(-2), [&[0xFF, 0xFF], &[0xFE, 0xFF, 0xFF, 0xFF], &[0xFE, 0xFF, 0xFF], &[0x00, 0xFE, 0xFF, 0xFF]]),
            ("24-bit max", s24(0x7F_FFFF), [&[0xFF, 0x7F], &[0xFF, 0xFF, 0x7F, 0x00], &[0xFF, 0xFF, 0x7F], &[0x00, 0xFF, 0xFF, 0x7F]]),
            ("24-bit min", s24(-0x80_0000), [&[0x00, 0x80], &[0x00, 0x00, 0x80, 0xFF], &[0x00, 0x00, 0x80], &[0x00, 0x00, 0x00, 0x80]]),
            ("zero", 0, [&[0; 2], &[0; 4], &[0; 3], &[0; 4]]),
        ];
        let formats = [S16Le, S24Le, S24_3Le, S32Le];
        for (name, sample, want) in rows {
            for (format, want) in formats.iter().zip(want) {
                assert_eq!(pack(&[*sample], *format), *want, "{name} as {format:?}");
            }
        }
        // Several samples are packed in order, back to back.
        let frame = [s24(0x12_3456), s24(-2)];
        assert_eq!(pack(&frame, S24_3Le), [0x56, 0x34, 0x12, 0xFE, 0xFF, 0xFF]);
        let mut out = vec![0xAA];
        pack_into(&[s16(1)], S16Le, &mut out);
        assert_eq!(out, [0xAA, 0x01, 0x00], "pack_into appends");
    }
}
