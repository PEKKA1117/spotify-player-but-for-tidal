use super::*;
use std::time::{Duration, SystemTime};
use tidal_player_core::library::{
    BrowseRef, LibraryRequest, LibraryResponse, ListItems, ListPage, ListRef, MixSummary, PageData,
    PageRequest,
};

use tidal_player_core::log::{LogBatch, LogLevel, LogLine};
use tidal_player_core::lyrics::{LyricLine, Lyrics};
use tidal_player_core::protocol::{ClientMessage, Command, Event, ServerMessage};

fn subscribe_line() -> Vec<u8> {
    b"\"Subscribe\"\n".to_vec()
}

fn request_line(id: u64) -> Vec<u8> {
    let mut line =
        serde_json::to_vec(&serde_json::json!({ "Request": { "id": id, "command": "Next" } }))
            .unwrap();
    line.push(b'\n');
    line
}

fn request(id: u64) -> ClientMessage {
    ClientMessage::Request {
        id,
        command: Command::Next,
    }
}

/// What one row expects of a decoded message.
#[derive(Debug)]
enum Want {
    Message(ClientMessage),
    /// An error whose text contains this.
    Error(&'static str),
}

/// A row: its name, the reads, what they decode to.
type Row = (&'static str, Vec<Vec<u8>>, Vec<Want>);

fn check(rows: Vec<Row>, limit: usize) {
    for (name, reads, want) in rows {
        let mut decoder = Decoder::<ClientMessage>::with_limit(limit);
        let got: Vec<_> = reads.iter().flat_map(|r| decoder.feed(r)).collect();
        assert_eq!(got.len(), want.len(), "{name}: {got:?}");
        for (got, want) in got.iter().zip(&want) {
            match (got, want) {
                (Ok(m), Want::Message(w)) => assert_eq!(m, w, "{name}"),
                (Err(e), Want::Error(text)) => {
                    assert!(e.to_string().contains(text), "{name}: {e}")
                }
                _ => panic!("{name}: got {got:?}, want {want:?}"),
            }
        }
    }
}

/// AC2: `encode` writes one line; the decoder assembles split
/// messages, decodes several per read, refuses invalid JSON, unknown
/// variants and long lines, naming the problem.
#[test]
fn ac2_framing() {
    // `encode`: one JSON line, which decodes back.
    let message = ServerMessage::Event(Event::ShuttingDown);
    let line = encode(&message);
    assert_eq!(line.last(), Some(&b'\n'), "{line:?}");
    assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
    let mut decoder = Decoder::<ServerMessage>::new();
    assert_eq!(decoder.feed(&line), vec![Ok(message)]);

    let both = [subscribe_line(), request_line(7)].concat();
    let rows: Vec<Row> = vec![
        (
            "one per read",
            vec![subscribe_line(), request_line(1)],
            vec![
                Want::Message(ClientMessage::Subscribe),
                Want::Message(request(1)),
            ],
        ),
        (
            "split across reads",
            {
                let line = request_line(2);
                line.chunks(3).map(<[u8]>::to_vec).collect()
            },
            vec![Want::Message(request(2))],
        ),
        (
            "several in one read",
            vec![[both.clone(), request_line(8)].concat()],
            vec![
                Want::Message(ClientMessage::Subscribe),
                Want::Message(request(7)),
                Want::Message(request(8)),
            ],
        ),
        (
            "a split right after a newline",
            vec![both[..12].to_vec(), both[12..].to_vec()],
            vec![
                Want::Message(ClientMessage::Subscribe),
                Want::Message(request(7)),
            ],
        ),
        (
            "unfinished line: nothing yet",
            vec![request_line(3)[..10].to_vec()],
            vec![],
        ),
        (
            "invalid JSON",
            vec![b"{\"Request\": \n".to_vec()],
            vec![Want::Error("invalid message: EOF while parsing")],
        ),
        (
            "unknown variant",
            vec![b"\"Unsubscribe\"\n".to_vec()],
            vec![Want::Error("unknown variant `Unsubscribe`")],
        ),
        (
            "unknown command",
            vec![b"{\"Request\":{\"id\":1,\"command\":\"Explode\"}}\n".to_vec()],
            vec![Want::Error("unknown variant `Explode`")],
        ),
        (
            "an invalid line, then a good one",
            vec![b"nope\n".to_vec(), subscribe_line()],
            vec![
                Want::Error("invalid message"),
                Want::Message(ClientMessage::Subscribe),
            ],
        ),
        (
            "a line at the limit",
            vec![format!("{:<64}\n", "\"Subscribe\"").into_bytes()],
            vec![Want::Message(ClientMessage::Subscribe)],
        ),
        (
            "a line over the limit, in one read",
            vec![format!("{:<65}\n", "\"Subscribe\"").into_bytes()],
            vec![Want::Error("message longer than 64 bytes")],
        ),
        (
            "a line over the limit, across reads, then a good one",
            vec![
                vec![b' '; 40],
                vec![b' '; 40],
                vec![b' '; 40],
                [b"\n".to_vec(), subscribe_line()].concat(),
            ],
            vec![
                Want::Error("message longer than 64 bytes"),
                Want::Message(ClientMessage::Subscribe),
            ],
        ),
    ];
    check(rows, 64);

    // 0006 AC1: the library's request decodes, as written by hand and
    // as encoded; so does its reply, on the client's side.
    let library = ClientMessage::Library {
        id: 9,
        request: LibraryRequest::More {
            list: ListRef::PlaylistTracks("36ea71a8-445e-41a4-82ab-6628c581535d".into()),
            offset: 100,
            limit: 50,
        },
    };
    check(
            vec![
                (
                    "a library request",
                    vec![b"{\"Library\":{\"id\":3,\"request\":{\"Page\":\"Library\"}}}\n".to_vec()],
                    vec![Want::Message(ClientMessage::Library {
                        id: 3,
                        request: LibraryRequest::Page(PageRequest::Library),
                    })],
                ),
                (
                    "a search request (0007 AC1)",
                    vec![
                        "{\"Library\":{\"id\":4,\"request\":{\"Page\":{\"Search\":\"Sigur Rós\"}}}}\n"
                            .as_bytes()
                            .to_vec(),
                    ],
                    vec![Want::Message(ClientMessage::Library {
                        id: 4,
                        request: LibraryRequest::Page(PageRequest::Search("Sigur Rós".into())),
                    })],
                ),
                (
                    "a mix page request (0011 AC1)",
                    vec![
                        b"{\"Library\":{\"id\":5,\"request\":{\"Page\":{\"Mix\":\"016214541b7a008d226272cf9d0964\"}}}}\n"
                            .to_vec(),
                    ],
                    vec![Want::Message(ClientMessage::Library {
                        id: 5,
                        request: LibraryRequest::Page(PageRequest::Mix(
                            "016214541b7a008d226272cf9d0964".into(),
                        )),
                    })],
                ),
                (
                    "a browse request (0024 AC1)",
                    vec![
                        b"{\"Library\":{\"id\":8,\"request\":{\"Page\":{\"Browse\":{\"Page\":\"pages/genre_pop\"}}}}}\n"
                            .to_vec(),
                    ],
                    vec![Want::Message(ClientMessage::Library {
                        id: 8,
                        request: LibraryRequest::Page(PageRequest::Browse(BrowseRef::Page(
                            "pages/genre_pop".into(),
                        ))),
                    })],
                ),
                (
                    "a lyrics request (0021 AC1)",
                    vec![
                        b"{\"Library\":{\"id\":7,\"request\":{\"Page\":{\"Lyrics\":62300893}}}}\n".to_vec(),
                    ],
                    vec![Want::Message(ClientMessage::Library {
                        id: 7,
                        request: LibraryRequest::Page(PageRequest::Lyrics(62300893)),
                    })],
                ),
                (
                    "a log request for everything (0027 AC1)",
                    vec![b"{\"Logs\":{\"id\":11,\"after\":null}}\n".to_vec()],
                    vec![Want::Message(ClientMessage::Logs { id: 11, after: None })],
                ),
                (
                    "a log request after a line (0027 AC1)",
                    vec![b"{\"Logs\":{\"id\":12,\"after\":4096}}\n".to_vec()],
                    vec![Want::Message(ClientMessage::Logs {
                        id: 12,
                        after: Some(4096),
                    })],
                ),
                (
                    "a log request without `after` (0027 AC1)",
                    vec![b"{\"Logs\":{\"id\":13}}\n".to_vec()],
                    vec![Want::Message(ClientMessage::Logs { id: 13, after: None })],
                ),
                (
                    "an encoded library request, split",
                    encode(&library).chunks(5).map(<[u8]>::to_vec).collect(),
                    vec![Want::Message(library)],
                ),
            ],
            MAX_LINE,
        );
    let replies = [
        ServerMessage::LibraryReply {
            id: 3,
            result: Ok(LibraryResponse::Items(ListItems::Tracks(ListPage {
                items: vec![],
                offset: 100,
                total: 362,
                hidden: 0,
                positions: Vec::new(),
            }))),
        },
        ServerMessage::LibraryReply {
            id: 4,
            result: Err("Album 1 was not found".into()),
        },
        // 0011 AC1: a mix page.
        ServerMessage::LibraryReply {
            id: 6,
            result: Ok(LibraryResponse::Page(PageData::Mix {
                mix: MixSummary {
                    id: "016214541b7a008d226272cf9d0964".into(),
                    title: "My Daily Discovery".into(),
                    subtitle: Some("Updates every morning.".into()),
                },
                tracks: ListPage {
                    items: vec![],
                    offset: 0,
                    total: 0,
                    hidden: 0,
                    positions: Vec::new(),
                },
            })),
        },
        // 0021 AC1: lyrics, `at` in milliseconds on the wire.
        ServerMessage::LibraryReply {
            id: 8,
            result: Ok(LibraryResponse::Page(PageData::Lyrics {
                track: 62300893,
                lyrics: Some(Lyrics {
                    synced: vec![LyricLine {
                        at: Duration::from_millis(6_320),
                        text: "We hung the lanterns".into(),
                    }],
                    plain: "We hung the lanterns".into(),
                    right_to_left: false,
                }),
            })),
        },
        ServerMessage::LibraryReply {
            id: 9,
            result: Ok(LibraryResponse::Page(PageData::Lyrics {
                track: 1403577,
                lyrics: None,
            })),
        },
    ];
    let mut decoder = Decoder::<ServerMessage>::new();
    let mut got = decoder.feed(b"{\"LibraryReply\":{\"id\":5,\"result\":{\"Ok\":\"Done\"}}}\n");
    for reply in &replies {
        got.extend(decoder.feed(&encode(reply)));
    }
    let want: Vec<_> = [ServerMessage::LibraryReply {
        id: 5,
        result: Ok(LibraryResponse::Done),
    }]
    .into_iter()
    .chain(replies)
    .map(Ok)
    .collect();
    assert_eq!(got, want, "library replies");

    // 0021 AC1: the wire form of a lyrics line, as written by hand.
    let wire = b"{\"LibraryReply\":{\"id\":10,\"result\":{\"Ok\":{\"Page\":{\"Lyrics\":{\"track\":1,\"lyrics\":{\"synced\":[{\"at\":6320,\"text\":\"x\"}],\"plain\":\"x\",\"right_to_left\":false}}}}}}}\n";
    assert_eq!(
        Decoder::<ServerMessage>::new().feed(wire),
        vec![Ok(ServerMessage::LibraryReply {
            id: 10,
            result: Ok(LibraryResponse::Page(PageData::Lyrics {
                track: 1,
                lyrics: Some(Lyrics {
                    synced: vec![LyricLine {
                        at: Duration::from_millis(6_320),
                        text: "x".into(),
                    }],
                    plain: "x".into(),
                    right_to_left: false,
                }),
            })),
        })],
        "a lyrics reply"
    );

    // 0027 AC1: a log reply, `at` in milliseconds on the wire, a level by
    // its name; a level that is none of the five is an error.
    let line = |level: &str| {
        format!(
            "{{\"LogsReply\":{{\"id\":2,\"result\":{{\"Ok\":{{\"lines\":[{{\"seq\":5,\"at\":1762800245123,\"level\":\"{level}\",\"target\":\"tidal_player::auth\",\"text\":\"refresh failed error=x\"}}],\"off\":false}}}}}}}}\n"
        )
    };
    assert_eq!(
        Decoder::<ServerMessage>::new().feed(line("Warn").as_bytes()),
        vec![Ok(ServerMessage::LogsReply {
            id: 2,
            result: Ok(LogBatch {
                lines: vec![LogLine {
                    seq: 5,
                    at: SystemTime::UNIX_EPOCH + Duration::from_millis(1_762_800_245_123),
                    level: LogLevel::Warn,
                    target: "tidal_player::auth".into(),
                    text: "refresh failed error=x".into(),
                }],
                off: false,
            }),
        })],
        "a log reply"
    );
    let bad = Decoder::<ServerMessage>::new().feed(line("Verbose").as_bytes());
    assert!(
        matches!(bad.as_slice(), [Err(e)] if e.to_string().contains("unknown variant `Verbose`")),
        "{bad:?}"
    );
    let reply = ServerMessage::LogsReply {
        id: 3,
        result: Err("the log is not available".into()),
    };
    assert_eq!(
        Decoder::<ServerMessage>::new().feed(&encode(&reply)),
        vec![Ok(reply)]
    );

    // The real limit: a 17 MiB line is refused once it passes 16 MiB,
    // and never held whole.
    let mut decoder = Decoder::<ClientMessage>::new();
    let chunk = vec![b'x'; 1024 * 1024];
    let mut errors = Vec::new();
    for _ in 0..17 {
        errors.extend(decoder.feed(&chunk));
        assert!(decoder.buffered() <= MAX_LINE, "{}", decoder.buffered());
    }
    assert_eq!(errors, vec![Err(DecodeError::TooLong { limit: MAX_LINE })]);
    assert_eq!(decoder.buffered(), 0);
    assert_eq!(
        decoder.feed(&[b"\n".to_vec(), subscribe_line()].concat()),
        vec![Ok(ClientMessage::Subscribe)]
    );
}

/// AC3: the greeting's exact text; `check_greeting` accepts this
/// version, names both versions on a mismatch, refuses anything else.
#[test]
fn ac3_greeting() {
    assert_eq!(
        String::from_utf8(greeting()).unwrap(),
        format!("{{\"tidal_player\":\"{}\"}}\n", env!("CARGO_PKG_VERSION"))
    );
    let socket = Path::new("/run/user/1000/tidal-player/player.sock");
    let mismatch = |theirs: &str| {
        Err(GreetingError::Mismatch {
            theirs: theirs.into(),
            ours: VERSION.into(),
        })
    };
    let not_ours_error = GreetingError::NotOurs(socket.into());
    let not_ours = Err(not_ours_error.clone());
    let ours = format!("{{\"tidal_player\":\"{VERSION}\"}}");
    let rows: Vec<(&str, String, Result<(), GreetingError>)> = vec![
        ("same version", ours.clone(), Ok(())),
        (
            "with spaces",
            format!("{{ \"tidal_player\" : \"{VERSION}\" }}"),
            Ok(()),
        ),
        (
            "older",
            "{\"tidal_player\":\"0.0.9\"}".into(),
            mismatch("0.0.9"),
        ),
        (
            "newer",
            "{\"tidal_player\":\"9.1.0-rc.1\"}".into(),
            mismatch("9.1.0-rc.1"),
        ),
        ("empty", String::new(), not_ours.clone()),
        ("not JSON", "SSH-2.0-OpenSSH_9.6".into(), not_ours.clone()),
        (
            "another key",
            "{\"spotify_player\":\"0.1.0\"}".into(),
            not_ours.clone(),
        ),
        ("a number", "{\"tidal_player\":1}".into(), not_ours.clone()),
        ("a message first", "\"Subscribe\"".into(), not_ours.clone()),
    ];
    for (name, line, want) in rows {
        assert_eq!(check_greeting(line.as_bytes(), socket), want, "{name}");
    }
    assert_eq!(
        mismatch("0.0.9").unwrap_err().to_string(),
        format!(
            "The running player is tidal-player 0.0.9, this is {VERSION}: restart it \
                 (\"tidal-player daemon stop\", or \"systemctl --user restart tidal-player\")"
        )
    );
    assert_eq!(
        not_ours_error.to_string(),
        "Not a tidal-player socket: /run/user/1000/tidal-player/player.sock"
    );
}
