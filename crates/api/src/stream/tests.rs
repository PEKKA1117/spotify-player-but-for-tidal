use super::*;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;

const HIRES: &str = include_str!("../../tests/fixtures/stream/playbackinfo_hires_dash.json");
const LOSSLESS: &str = include_str!("../../tests/fixtures/stream/playbackinfo_lossless_dash.json");
const HIGH: &str = include_str!("../../tests/fixtures/stream/playbackinfo_high_bts.json");
const BTS_FLAC: &str =
    include_str!("../../tests/fixtures/stream/playbackinfo_lossless_bts_flac.json");
const MPD_HIRES: &str = include_str!("../../tests/fixtures/stream/manifest_hires_24_96.mpd");
const MPD_LOSSLESS: &str = include_str!("../../tests/fixtures/stream/manifest_lossless_16_44.mpd");

/// `fixture` with top-level fields replaced.
fn edit(fixture: &str, fields: &[(&str, Value)]) -> Vec<u8> {
    let mut info: Value = serde_json::from_str(fixture).unwrap();
    for (key, value) in fields {
        info[*key] = value.clone();
    }
    serde_json::to_vec(&info).unwrap()
}

/// The decoded manifest of `fixture`.
fn manifest(fixture: &str) -> String {
    let info: Value = serde_json::from_str(fixture).unwrap();
    let bytes = STANDARD.decode(info["manifest"].as_str().unwrap()).unwrap();
    String::from_utf8(bytes).unwrap()
}

/// `fixture` with its manifest passed through `change`.
fn edit_manifest(fixture: &str, change: impl Fn(String) -> String) -> Vec<u8> {
    let encoded = STANDARD.encode(change(manifest(fixture)));
    edit(fixture, &[("manifest", Value::String(encoded))])
}

/// The `HIGH` fixture with one field of its BTS manifest replaced.
fn bts_with(field: &str, value: Value) -> Vec<u8> {
    edit_manifest(HIGH, |m| {
        let mut bts: Value = serde_json::from_str(&m).unwrap();
        bts[field] = value.clone();
        bts.to_string()
    })
}

fn unsupported(what: Unsupported) -> StreamError {
    StreamError::Unsupported(what)
}

#[test]
fn ac2_bts_manifest() {
    let rows: Vec<(&str, Vec<u8>, Result<StreamPlan, StreamError>)> = vec![
        (
            "FLAC",
            BTS_FLAC.as_bytes().to_vec(),
            Ok(StreamPlan::Single {
                url: "https://amz-pr-fa.audio.tidal.com/FAKE.flac?token=FAKE".into(),
                codec: Codec::Flac,
            }),
        ),
        (
            "AAC-LC",
            HIGH.as_bytes().to_vec(),
            Ok(StreamPlan::Single {
                url: "https://amz-pr-fa.audio.tidal.com/FAKE.mp4?token=FAKE".into(),
                codec: Codec::AacLc,
            }),
        ),
        (
            "encrypted BTS",
            bts_with("encryptionType", "OLD_AES".into()),
            Err(unsupported(Unsupported::Encrypted)),
        ),
        (
            "encrypted DASH",
            edit_manifest(HIRES, |m| {
                m.replace(
                        "<Role ",
                        r#"<ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" value="cenc"/><Role "#,
                    )
            }),
            Err(unsupported(Unsupported::Encrypted)),
        ),
        (
            "unknown manifest type",
            edit(
                HIGH,
                &[("manifestMimeType", "application/vnd.tidal.emu".into())],
            ),
            Err(unsupported(Unsupported::Manifest(
                "application/vnd.tidal.emu".into(),
            ))),
        ),
    ];
    for (name, body, expected) in rows {
        let got = parse_playback_info(&body).map(|r| r.plan);
        assert_eq!(got, expected, "{name}");
    }

    for (name, body) in [
        ("no URL", bts_with("urls", serde_json::json!([]))),
        ("not base64", edit(HIGH, &[("manifest", "%%%".into())])),
        ("not JSON", edit_manifest(HIGH, |_| "nope".into())),
    ] {
        let got = parse_playback_info(&body);
        assert!(
            matches!(got, Err(StreamError::Malformed(_))),
            "{name}: {got:?}"
        );
    }
}

fn segments(plan: &StreamPlan) -> (&str, &[Segment], Codec) {
    match plan {
        StreamPlan::Segmented {
            init_url,
            segments,
            codec,
        } => (init_url, segments, *codec),
        other => panic!("not segmented: {other:?}"),
    }
}

/// Checks numbering, contiguity and the total of `segments`.
fn assert_timeline(segments: &[Segment], first_number: u64, total_ticks: u64) {
    let mut next_start = segments.first().map_or(0, |s| s.start);
    for (i, segment) in segments.iter().enumerate() {
        assert_eq!(segment.number, first_number + i as u64, "number of #{i}");
        assert_eq!(segment.start, next_start, "start of #{i}");
        next_start += segment.duration;
    }
    let total: u64 = segments.iter().map(|s| s.duration).sum();
    assert_eq!(total, total_ticks);
}

#[test]
fn ac3_dash_manifest() {
    // The recorded hi-res MPD: 73 × 380928 + 129030 ticks at 96 kHz.
    let plan = parse_mpd(MPD_HIRES).unwrap();
    let (init_url, segs, codec) = segments(&plan);
    assert_eq!(segs.len(), 74);
    assert_eq!(codec, Codec::Flac);
    assert_eq!(
        init_url,
        "https://sp-ad-fa.audio.tidal.com/mediatracks/FAKE/0.mp4?token=FAKE"
    );
    assert_timeline(segs, 1, 27_936_774);
    let url = |n| format!("https://sp-ad-fa.audio.tidal.com/mediatracks/FAKE/{n}.mp4?token=FAKE");
    assert_eq!(segs[0].url, url(1));
    assert_eq!(segs[1].url, url(2));
    assert_eq!(segs[73].url, url(74));
    assert_eq!(segs[0].start, 0);
    assert_eq!(segs[0].start_time(), Duration::ZERO);
    assert_eq!(segs[1].start_time(), Duration::from_millis(3968));
    assert_eq!(segs[72].duration, 380_928);
    assert_eq!(segs[73].start, 73 * 380_928);
    assert_eq!(segs[73].duration, 129_030);
    assert!(segs.iter().all(|s| s.timescale == 96_000));
    // = mediaPresentationDuration PT4M51.008S, to the millisecond.
    assert_eq!(segs[73].end_time().as_millis(), 291_008);

    // The recorded 16/44.1 MPD: `&amp;` in the signed URLs is unescaped.
    let plan = parse_mpd(MPD_LOSSLESS).unwrap();
    let (init_url, segs, _) = segments(&plan);
    let query = "Policy=FAKE-POLICY&Signature=FAKE-SIG&Key-Pair-Id=FAKE-KEY";
    assert_eq!(
        init_url,
        format!("https://sp-ad-cf.audio.tidal.com/mediatracks/FAKE/0.mp4?{query}")
    );
    assert_eq!(segs.len(), 57);
    assert_timeline(segs, 1, 56 * 176_128 + 52_930);
    assert_eq!(
        segs[56].url,
        format!("https://sp-ad-cf.audio.tidal.com/mediatracks/FAKE/57.mp4?{query}")
    );
    // = mediaPresentationDuration PT3M44.854S, to the millisecond.
    assert_eq!(segs[56].end_time().as_millis(), 224_854);

    // Hand-written: `t` on the first entry, repeats, startNumber 5.
    let mpd = r#"<MPD><Period><AdaptationSet><Representation codecs="flac">
            <SegmentTemplate timescale="10" initialization="i.mp4" media="s$Number$.m4s" startNumber="5">
              <SegmentTimeline><S t="100" d="20" r="2"/><S d="7"/><S d="3" r="1"/></SegmentTimeline>
            </SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
    let plan = parse_mpd(mpd).unwrap();
    let (init_url, segs, _) = segments(&plan);
    assert_eq!(init_url, "i.mp4");
    let got: Vec<(u64, &str, u64, u64)> = segs
        .iter()
        .map(|s| (s.number, s.url.as_str(), s.start, s.duration))
        .collect();
    assert_eq!(
        got,
        [
            (5, "s5.m4s", 100, 20),
            (6, "s6.m4s", 120, 20),
            (7, "s7.m4s", 140, 20),
            (8, "s8.m4s", 160, 7),
            (9, "s9.m4s", 167, 3),
            (10, "s10.m4s", 170, 3),
        ]
    );
    assert_eq!(segs[0].start_time(), Duration::from_secs(10));
}

#[test]
fn ac4_validate_playbackinfo() {
    let errors: Vec<(&str, Vec<u8>, StreamError)> = vec![
        (
            "preview",
            edit(HIRES, &[("assetPresentation", "PREVIEW".into())]),
            StreamError::PreviewOnly,
        ),
        (
            "Dolby Atmos",
            edit(HIRES, &[("audioMode", "DOLBY_ATMOS".into())]),
            unsupported(Unsupported::AudioMode("DOLBY_ATMOS".into())),
        ),
        (
            "HE-AAC (LOW)",
            bts_with("codecs", "mp4a.40.5".into()),
            unsupported(Unsupported::Codec("mp4a.40.5".into())),
        ),
        (
            "HE-AAC v2",
            bts_with("codecs", "mp4a.40.29".into()),
            unsupported(Unsupported::Codec("mp4a.40.29".into())),
        ),
        (
            "HE-AAC in DASH",
            edit_manifest(HIRES, |m| {
                m.replace(r#"codecs="flac""#, r#"codecs="mp4a.40.5""#)
            }),
            unsupported(Unsupported::Codec("mp4a.40.5".into())),
        ),
        (
            "unknown quality",
            edit(HIRES, &[("audioQuality", "HI_RES".into())]),
            unsupported(Unsupported::Quality("HI_RES".into())),
        ),
    ];
    for (name, body, expected) in errors {
        assert_eq!(parse_playback_info(&body), Err(expected), "{name}");
    }

    let grants = [
        (
            "hi-res",
            HIRES,
            1001,
            AudioQuality::HiResLossless,
            Some(24),
            Some(96_000),
            Codec::Flac,
        ),
        (
            "lossless",
            LOSSLESS,
            1002,
            AudioQuality::Lossless,
            Some(16),
            Some(44_100),
            Codec::Flac,
        ),
        (
            "high",
            HIGH,
            1002,
            AudioQuality::High,
            None,
            None,
            Codec::AacLc,
        ),
    ];
    for (name, body, track_id, quality, bits, rate, codec) in grants {
        let got = parse_playback_info(body.as_bytes());
        let got = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(got.track_id, track_id, "{name}");
        assert_eq!(got.quality, quality, "{name}");
        assert_eq!(got.bit_depth, bits, "{name}");
        assert_eq!(got.sample_rate, rate, "{name}");
        assert_eq!(got.plan.codec(), codec, "{name}");
    }
}
