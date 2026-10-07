//! Decode tests (spec 0003 AC7–AC9).

mod common;

use common::*;
use tidal_player_audio::decode::{Decoder, decode_all};
use tidal_player_audio::testing::FakeSource;
use tidal_player_audio::{Codec, EngineError, Event, SinkCall, SinkScript, SourceFormat};

/// AC7: raw FLAC 16/44.1, and FLAC 24/96 in fragmented MP4 read as init +
/// segments joined from a non-seekable reader, decode to the plain-FLAC
/// reference, report the right format, and are left-justified.
#[test]
fn ac7_flac_raw_and_fmp4() {
    struct Row {
        name: &'static str,
        source: fn() -> FakeSource,
        reference: fn() -> FakeSource,
        format: SourceFormat,
        frames: usize,
        pcm: &'static str,
        pcm_bytes: usize,
    }
    let rows = [
        Row {
            name: "raw FLAC 16/44.1",
            source: flac16,
            reference: flac16,
            format: SourceFormat {
                codec: Codec::Flac,
                sample_rate: 44_100,
                channels: 2,
                bits_per_sample: Some(16),
            },
            frames: 44_100,
            pcm: "flac16_44.s16le",
            pcm_bytes: 2,
        },
        Row {
            name: "FLAC 24/96 in fMP4 segments",
            source: dash24,
            reference: flac24,
            format: SourceFormat {
                codec: Codec::Flac,
                sample_rate: 96_000,
                channels: 2,
                bits_per_sample: Some(24),
            },
            frames: 57_600,
            pcm: "flac24_96.s24le",
            pcm_bytes: 3,
        },
    ];
    for row in rows {
        let (format, samples) = decode_all(Box::new((row.source)())).expect(row.name);
        let (_, reference) = reference((row.reference)());
        assert_eq!(format, row.format, "{}: format", row.name);
        assert_eq!(samples.len(), row.frames * 2, "{}: sample count", row.name);
        assert!(
            samples == reference,
            "{}: differs from the plain-FLAC reference",
            row.name
        );

        // Left-justified: decoded == pcm << (32 - bits), against ffmpeg's decode.
        let bits = row.format.bits_per_sample.unwrap();
        let pcm = pcm_left_justified(&fixture(row.pcm), row.pcm_bytes);
        assert_eq!(
            &samples[..pcm.len()],
            &pcm[..],
            "{}: left-justified PCM",
            row.name
        );
        let low_bits = (1i32 << (32 - bits)) - 1;
        assert!(
            samples.iter().all(|s| s & low_bits == 0),
            "{}: low bits",
            row.name
        );
        assert!(samples.iter().any(|&s| s != 0), "{}: silence", row.name);
    }
}

/// AC8: AAC-LC decodes as lossy (`bits: None`); HE-AAC fails with
/// `Unsupported` before the output device is opened.
#[test]
fn ac8_aac_lc_and_he_aac() {
    let (format, samples) =
        decode_all(Box::new(FakeSource::file(fixture("aac_lc.m4a"), "mp4"))).expect("AAC-LC");
    assert_eq!(
        format,
        SourceFormat {
            codec: Codec::AacLc,
            sample_rate: 44_100,
            channels: 2,
            bits_per_sample: None,
        }
    );
    // 0.6 s of audio, plus the encoder delay and padding (not trimmed).
    assert!(samples.len() >= 26_460 * 2, "{} samples", samples.len());
    assert!(samples.iter().any(|&s| s != 0));

    let he_aac = || FakeSource::file(fixture("he_aac.m4a"), "mp4");
    assert!(
        matches!(
            Decoder::open(Box::new(he_aac())),
            Err(EngineError::Unsupported(_))
        ),
        "HE-AAC must be Unsupported"
    );

    let rig = rig(SinkScript::default());
    rig.play(he_aac());
    let events = rig.until_end();
    assert!(
        matches!(
            events.last(),
            Some(Event::Error {
                error: EngineError::Unsupported(_),
                ..
            })
        ),
        "{events:?}"
    );
    assert_eq!(count(&events, |e| matches!(e, Event::TrackEnded { .. })), 0);
    rig.stop();
    assert!(
        !rig.sinks
            .calls()
            .iter()
            .any(|c| matches!(c, SinkCall::Open { .. })),
        "the device was opened: {:?}",
        rig.sinks.calls()
    );
}

/// AC9: a mono FLAC is output as stereo, both channels equal to the source.
#[test]
fn ac9_mono_to_stereo() {
    let (format, samples) = decode_all(Box::new(mono16())).expect("mono");
    assert_eq!(samples.len(), 22_050 * 2, "two samples per source frame");
    assert_eq!(format.channels, 1);
    let source = pcm_left_justified(&fixture("mono16_44.s16le"), 2);
    for (i, &s) in source.iter().enumerate() {
        assert_eq!((samples[2 * i], samples[2 * i + 1]), (s, s), "frame {i}");
    }
    assert!(samples.chunks(2).all(|f| f[0] == f[1]), "L == R everywhere");
}
