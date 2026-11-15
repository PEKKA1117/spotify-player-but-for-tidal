use std::path::Path;
use std::time::Duration;

use super::*;
use crate::{AlbumRef, ArtistRef, TrackId};

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("Title {id}"),
        version: None,
        artists: vec![ArtistRef {
            id: 1,
            name: "Artist".into(),
        }],
        album: Some(AlbumRef {
            id: 1,
            title: "Album".into(),
            cover: None,
        }),
        duration: Some(Duration::from_secs(200)),
        streamable: true,
    }
}

/// `Song (Live)` by two artists on `The Album`.
fn song() -> Track {
    Track {
        title: "Song".into(),
        version: Some("Live".into()),
        artists: vec![
            ArtistRef {
                id: 1,
                name: "First Artist".into(),
            },
            ArtistRef {
                id: 2,
                name: "Second Artist".into(),
            },
        ],
        album: Some(AlbumRef {
            id: 9,
            title: "The Album".into(),
            cover: None,
        }),
        ..track(1)
    }
}

fn format(summary: &str, body: &str) -> NotifyFormat {
    NotifyFormat::new(summary, body).unwrap_or_else(|e| panic!("{summary:?} {body:?}: {e}"))
}

/// AC2: the placeholders render as "The format" says.
#[test]
fn ac2_render() {
    let plain = Track {
        version: None,
        album: None,
        ..song()
    };
    let rows: &[(&str, &str, Track, &str)] = &[
        (
            "the default summary",
            DEFAULT_SUMMARY,
            song(),
            "Song (Live) • First Artist, Second Artist",
        ),
        ("the default body", DEFAULT_BODY, song(), "The Album"),
        ("no version", "{track}", plain.clone(), "Song"),
        ("no album", "{album}", plain.clone(), ""),
        ("text around", "[{album}]", plain, "[]"),
        (
            "twice",
            "{track} / {track}",
            song(),
            "Song (Live) / Song (Live)",
        ),
        (
            "text and a newline kept",
            "by {artists}\non {album}!",
            song(),
            "by First Artist, Second Artist\non The Album!",
        ),
        ("only text", "Now playing", song(), "Now playing"),
        ("one artist", "{artists}", track(3), "Artist"),
        ("an unclosed brace", "{track} {", song(), "Song (Live) {"),
    ];
    for (name, text, track, want) in rows {
        let got = format(text, text).render(track);
        assert_eq!(got, ((*want).to_owned(), (*want).to_owned()), "{name}");
    }
    // The default format is spotify-player's.
    assert_eq!(
        NotifyFormat::default().render(&song()),
        (
            "Song (Live) • First Artist, Second Artist".to_owned(),
            "The Album".to_owned()
        )
    );
}

/// AC3: a `{name}` other than the three names itself; an unclosed `{` is
/// text; an empty summary is refused, an empty body is not.
#[test]
fn ac3_unknown_placeholders() {
    let rows: &[(&str, &str, Result<(), FormatError>)] = &[
        ("{track}", "{album}", Ok(())),
        ("{track}", "", Ok(())),
        ("{track} {", "{ album", Ok(())),
        ("a { b {track}", "}{artists}{", Ok(())),
        ("{title}", "", Err(FormatError::Summary("{title}".into()))),
        (
            "{track}",
            "on {albums}",
            Err(FormatError::Body("{albums}".into())),
        ),
        ("{Track}", "", Err(FormatError::Summary("{Track}".into()))),
        (
            "{ track }",
            "",
            Err(FormatError::Summary("{ track }".into())),
        ),
        ("{}", "", Err(FormatError::Summary("{}".into()))),
        ("", "{album}", Err(FormatError::EmptySummary)),
    ];
    for (summary, body, want) in rows {
        let got = NotifyFormat::new(summary, body).map(|_| ());
        assert_eq!(&got, want, "{summary:?} {body:?}");
    }
    assert_eq!(
        FormatError::Body("{albums}".into()).to_string(),
        "unknown placeholder {albums}: expected {track}, {artists} or {album}"
    );
}

fn settings(timeout_secs: u16, transient: bool) -> NotifySettings {
    NotifySettings {
        timeout_secs,
        transient,
        ..NotifySettings::default()
    }
}

/// AC4: the `Notify` arguments, for each combination of a cover, the
/// transient hint and the timeout.
#[test]
fn ac4_request() {
    let cover = Path::new("/cache/covers/x.jpg");
    for with_cover in [false, true] {
        for transient in [false, true] {
            for (timeout, expire) in [(0, -1), (7, 7000)] {
                let name = format!("cover {with_cover}, transient {transient}, timeout {timeout}");
                let got = Request::new(
                    &settings(timeout, transient),
                    &song(),
                    with_cover.then_some(cover),
                    41,
                    false,
                );
                let mut hints = Vec::new();
                if with_cover {
                    hints.push((
                        "image-path".to_owned(),
                        Hint::Str(cover.display().to_string()),
                    ));
                }
                if transient {
                    hints.push(("transient".to_owned(), Hint::Bool(true)));
                }
                assert_eq!(got.hints, hints, "{name}: hints");
                assert_eq!(got.expire_timeout, expire, "{name}: expire_timeout");
                let want = Request {
                    app_name: "tidal-player".into(),
                    replaces_id: 41,
                    app_icon: String::new(),
                    summary: "Song (Live) • First Artist, Second Artist".into(),
                    body: "The Album".into(),
                    actions: Vec::new(),
                    hints,
                    expire_timeout: expire,
                };
                assert_eq!(got, want, "{name}");
            }
        }
    }
    // The largest timeout fits.
    let got = Request::new(&settings(3600, false), &song(), None, 0, false);
    assert_eq!(got.expire_timeout, 3_600_000);
}

/// AC5: which of the tracks that start, in order, notify.
#[test]
fn ac5_which_tracks_notify() {
    // `0` stands for a stop between starts: it changes nothing.
    let rows: &[(&str, &[u64], &[bool])] = &[
        ("the first", &[1], &[true]),
        ("a different track", &[1, 2], &[true, true]),
        ("the same track again", &[1, 1, 1], &[true, false, false]),
        ("after a stop", &[1, 0, 1], &[true, false, false]),
        ("A, B, A", &[1, 2, 1], &[true, true, true]),
        ("A, A, B, B", &[1, 1, 2, 2], &[true, false, true, false]),
    ];
    for (name, starts, want) in rows {
        let mut tracker = NotifyTracker::default();
        let got: Vec<bool> = starts
            .iter()
            .map(|id| *id != 0 && tracker.started(&track(*id)))
            .collect();
        assert_eq!(&got, want, "{name}");
    }
}

/// AC6: the body is escaped for a server reading markup, the summary
/// never.
#[test]
fn ac6_markup() {
    let rock = Track {
        title: "Rock & Roll <Live>".into(),
        version: None,
        album: Some(AlbumRef {
            id: 1,
            title: "A & B <C> > D".into(),
            cover: None,
        }),
        ..track(1)
    };
    // The format's own text is the body's too.
    let literal = NotifySettings {
        format: format("Tom & Jerry <3", "Tom & Jerry <3 >"),
        ..NotifySettings::default()
    };
    let got = Request::new(&literal, &rock, None, 0, true);
    assert_eq!(got.body, "Tom &amp; Jerry &lt;3 &gt;", "the format's text");
    assert_eq!(got.summary, "Tom & Jerry <3", "the format's text");
    let both = format("{track}", "{track} on {album}");
    let settings = NotifySettings {
        format: both,
        ..NotifySettings::default()
    };
    let rows = [
        (
            "body-markup",
            true,
            "Rock &amp; Roll &lt;Live&gt; on A &amp; B &lt;C&gt; &gt; D",
        ),
        (
            "no body-markup",
            false,
            "Rock & Roll <Live> on A & B <C> > D",
        ),
    ];
    for (name, markup, body) in rows {
        let got = Request::new(&settings, &rock, None, 0, markup);
        assert_eq!(got.summary, "Rock & Roll <Live>", "{name}");
        assert_eq!(got.body, body, "{name}");
    }
    assert_eq!(escape_markup("&amp;"), "&amp;amp;");
    assert!(supports_markup(&["body".into(), "body-markup".into()]));
    assert!(!supports_markup(&["body".into(), "actions".into()]));
}
