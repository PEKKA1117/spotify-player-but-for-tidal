//! Spec 0023: the commands against a fake player.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::Duration;

use clap::Parser;
use tidal_player_core::library::{
    LibraryRequest, LibraryResponse, ListItems, ListPage, ListRef, PageData, PageRequest,
    PlaylistSummary,
};
use tidal_player_core::protocol::{ClientMessage, ServerMessage};
use tidal_player_core::{Track, TrackId};

use super::*;
use crate::ipc::client::RecvError;

const MINE: &str = "11111111-1111-4111-8111-111111111111";
const OTHER: &str = "22222222-2222-4222-8222-222222222222";
const THEIRS: &str = "33333333-3333-4333-8333-333333333333";
const NEW: &str = "99999999-9999-4999-8999-999999999999";

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: PlaylistCommand,
}

fn parse(args: &[&str]) -> Result<PlaylistCommand, clap::Error> {
    let line = std::iter::once("playlist").chain(args.iter().copied());
    Args::try_parse_from(line).map(|a| a.command)
}

fn planned(args: &[&str]) -> Plan {
    plan(&parse(args).expect("parses")).expect("a plan")
}

fn track(id: u64) -> Track {
    Track {
        id: TrackId(id),
        title: format!("T{id}"),
        version: None,
        artists: Vec::new(),
        album: None,
        duration: None,
        streamable: true,
    }
}

fn summary(uuid: &str, title: &str, own: bool) -> PlaylistSummary {
    PlaylistSummary {
        uuid: uuid.into(),
        title: title.into(),
        tracks: Some(0),
        duration: None,
        own,
        description: None,
        public: false,
    }
}

/// One playlist of the fake library: its tracks, and its total with videos.
struct Stored {
    summary: PlaylistSummary,
    ids: Vec<u64>,
    total: u32,
}

/// A fake player over a fake library, recording what it is asked.
#[derive(Default)]
struct Player {
    playlists: HashMap<String, Stored>,
    order: Vec<String>,
    sent: Vec<LibraryRequest>,
    replies: VecDeque<ServerMessage>,
    /// The nth write (0-based) fails with this message.
    fail_write: Option<(usize, String)>,
    writes: usize,
}

impl Player {
    fn with(self, summary: PlaylistSummary, ids: &[u64]) -> Self {
        let total = ids.len() as u32;
        self.with_total(summary, ids, total)
    }

    fn with_total(mut self, summary: PlaylistSummary, ids: &[u64], total: u32) -> Self {
        self.order.push(summary.uuid.clone());
        self.playlists.insert(
            summary.uuid.clone(),
            Stored {
                summary,
                ids: ids.to_vec(),
                total,
            },
        );
        self
    }

    fn page(&self, uuid: &str, offset: u32, limit: u32) -> ListPage<Track> {
        let stored = &self.playlists[uuid];
        let items = stored
            .ids
            .iter()
            .skip(offset as usize)
            .take(limit as usize)
            .copied()
            .map(track)
            .collect();
        ListPage {
            items,
            offset,
            total: stored.total,
            hidden: 0,
            positions: Vec::new(),
        }
    }

    fn answer(&mut self, request: &LibraryRequest) -> Result<LibraryResponse, String> {
        use LibraryRequest as R;
        match request {
            R::Page(PageRequest::Playlist(uuid)) => {
                let Some(stored) = self.playlists.get(uuid) else {
                    return Err(format!("Playlist {uuid} was not found"));
                };
                Ok(LibraryResponse::Page(PageData::Playlist {
                    playlist: stored.summary.clone(),
                    etag: Some("e".into()),
                    tracks: self.page(uuid, 0, 100),
                }))
            }
            R::More {
                list: ListRef::PlaylistTracks(uuid),
                offset,
                limit,
            } => Ok(LibraryResponse::Items(ListItems::Tracks(
                self.page(uuid, *offset, *limit),
            ))),
            R::More {
                list: ListRef::Playlists,
                offset,
                limit,
            } => {
                let items: Vec<PlaylistSummary> = self
                    .order
                    .iter()
                    .skip(*offset as usize)
                    .take(*limit as usize)
                    .map(|u| self.playlists[u].summary.clone())
                    .collect();
                Ok(LibraryResponse::Items(ListItems::Playlists(ListPage {
                    items,
                    offset: *offset,
                    total: self.order.len() as u32,
                    hidden: 0,
                    positions: Vec::new(),
                })))
            }
            _ => {
                let write = self.writes;
                self.writes += 1;
                if let Some((n, message)) = &self.fail_write
                    && *n == write
                {
                    return Err(message.clone());
                }
                Ok(match request {
                    R::CreatePlaylist { title, .. } => {
                        LibraryResponse::Created(summary(NEW, title, true))
                    }
                    _ => LibraryResponse::Done,
                })
            }
        }
    }

    /// The writes sent, reads left out.
    fn writes_sent(&self) -> Vec<LibraryRequest> {
        self.sent
            .iter()
            .filter(|r| !matches!(r, LibraryRequest::Page(_) | LibraryRequest::More { .. }))
            .cloned()
            .collect()
    }
}

impl Link for Player {
    fn send(&mut self, message: &ClientMessage) -> io::Result<()> {
        let ClientMessage::Library { id, request } = message else {
            panic!("not a library request: {message:?}");
        };
        self.sent.push(request.clone());
        let result = self.answer(request);
        self.replies
            .push_back(ServerMessage::LibraryReply { id: *id, result });
        Ok(())
    }

    fn recv(&mut self, _: Option<Duration>) -> Result<Option<ServerMessage>, RecvError> {
        Ok(self.replies.pop_front())
    }

    fn close(&mut self) {}
}

/// Runs `args`: the exit code, stdout, stderr. `typed`: the terminal.
fn run_with(player: &mut Player, args: &[&str], typed: Option<&str>) -> (u8, String, String) {
    let plan = planned(args);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut input = typed.map(|t| t.as_bytes());
    let terminal = input.as_mut().map(|t| t as &mut dyn BufRead);
    let code = execute(
        player,
        &plan,
        Duration::from_secs(1),
        &mut out,
        &mut err,
        terminal,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn run_it(player: &mut Player, args: &[&str]) -> (u8, String, String) {
    run_with(player, args, None)
}

fn adds(uuid: &str, batches: &[std::ops::Range<u64>]) -> Vec<LibraryRequest> {
    batches
        .iter()
        .map(|ids| LibraryRequest::AddToPlaylist {
            uuid: uuid.into(),
            tracks: ids.clone().map(TrackId).collect(),
            allow_duplicates: true,
        })
        .collect()
}

/// AC1: every command's plan; a UUID or a playlist link.
#[test]
fn ac1_parse() {
    let link = format!("https://tidal.com/browse/playlist/{MINE}");
    let rows: Vec<(Vec<&str>, Plan)> = vec![
        (vec!["list"], Plan::List { json: false }),
        (vec!["list", "--json"], Plan::List { json: true }),
        (
            vec!["new", " Road ", "-d", "Words", "--public"],
            Plan::New {
                title: "Road".into(),
                description: "Words".into(),
                public: true,
            },
        ),
        (
            vec!["new", "Road"],
            Plan::New {
                title: "Road".into(),
                description: String::new(),
                public: false,
            },
        ),
        (
            vec!["edit", MINE, "--name", " A ", "--description", ""],
            Plan::Edit {
                uuid: MINE.into(),
                title: Some("A".into()),
                description: Some(String::new()),
                public: None,
            },
        ),
        (
            vec!["edit", &link, "--private"],
            Plan::Edit {
                uuid: MINE.into(),
                title: None,
                description: None,
                public: Some(false),
            },
        ),
        (
            vec!["delete", MINE, "--yes"],
            Plan::Delete {
                uuid: MINE.into(),
                yes: true,
            },
        ),
        (
            vec!["fork", &link, "--name", " Copy "],
            Plan::Fork {
                uuid: MINE.into(),
                name: Some("Copy".into()),
            },
        ),
        (
            vec!["import", MINE, OTHER, "--duplicates"],
            Plan::Import {
                from: MINE.into(),
                to: OTHER.into(),
                duplicates: true,
            },
        ),
    ];
    for (args, want) in rows {
        assert_eq!(planned(&args), want, "{args:?}");
    }
}

/// AC1: refused with nothing sent; clap refuses the two flags together.
#[test]
fn ac1_refused() {
    let track = "https://tidal.com/browse/track/1";
    let album = "https://tidal.com/browse/album/1";
    let rows: Vec<(Vec<&str>, UsageError)> = vec![
        (vec!["delete", "1"], UsageError::NotAPlaylist("1".into())),
        (vec!["delete", "x"], UsageError::NotAPlaylist("x".into())),
        (vec!["fork", track], UsageError::NotAPlaylist(track.into())),
        (vec!["fork", album], UsageError::NotAPlaylist(album.into())),
        (vec!["new", "  "], UsageError::BlankName),
        (vec!["edit", MINE, "--name", " "], UsageError::BlankName),
        (vec!["fork", MINE, "--name", " "], UsageError::BlankName),
        (vec!["edit", MINE], UsageError::NothingToChange),
        (vec!["import", MINE, MINE], UsageError::SamePlaylist),
    ];
    for (args, want) in rows {
        assert_eq!(plan(&parse(&args).unwrap()), Err(want), "{args:?}");
    }
    assert!(parse(&["edit", MINE, "--public", "--private"]).is_err());
}

/// AC2: tab-separated, a title's tabs and line breaks as spaces.
#[test]
fn ac2_list_lines() {
    let mut player = Player::default()
        .with(
            PlaylistSummary {
                tracks: Some(39),
                ..summary(MINE, "Late\tnight\nout", true)
            },
            &[],
        )
        .with(summary(THEIRS, "Theirs", false), &[]);
    let (code, out, err) = run_it(&mut player, &["list"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        format!("{MINE}\town\t39\tLate night out\n{THEIRS}\tfollowed\t0\tTheirs\n")
    );
}

/// AC2: every page is read.
#[test]
fn ac2_list_pages() {
    let mut player = Player::default();
    for n in 0..120 {
        player = player.with(summary(&format!("p{n}"), "P", true), &[]);
    }
    let (code, out, _) = run_it(&mut player, &["list"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 120);
    let offsets: Vec<u32> = player
        .sent
        .iter()
        .map(|r| match r {
            LibraryRequest::More { offset, .. } => *offset,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(offsets, vec![0, 50, 100]);
}

/// AC2: one array; `null` when unknown or empty.
#[test]
fn ac2_list_json() {
    let mut player = Player::default().with(
        PlaylistSummary {
            tracks: None,
            description: Some("Words".into()),
            public: true,
            ..summary(MINE, "Late", true)
        },
        &[],
    );
    let (code, out, _) = run_it(&mut player, &["list", "--json"]);
    assert_eq!(code, 0);
    let got: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(
        got,
        serde_json::json!([{
            "uuid": MINE,
            "title": "Late",
            "own": true,
            "public": true,
            "tracks": null,
            "description": "Words",
        }])
    );
    let (_, empty, _) = run_it(&mut Player::default(), &["list", "--json"]);
    assert_eq!(empty.trim(), "[]");
}

/// AC3: the UUID on stdout; `--public` after creating.
#[test]
fn ac3_new() {
    let mut player = Player::default();
    let (code, out, err) = run_it(&mut player, &["new", "Road", "-d", "W", "--public"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("{NEW}\n"));
    assert_eq!(
        player.sent,
        vec![
            LibraryRequest::CreatePlaylist {
                title: "Road".into(),
                description: "W".into(),
            },
            LibraryRequest::SetPlaylistPublic {
                uuid: NEW.into(),
                public: true,
            },
        ]
    );
}

/// AC3: created but not public: the UUID, the reason, exit 1.
#[test]
fn ac3_new_public_fails() {
    let mut player = Player {
        fail_write: Some((1, "Could not reach Tidal".into())),
        ..Player::default()
    };
    let (code, out, err) = run_it(&mut player, &["new", "Road", "--public"]);
    assert_eq!(code, 1);
    assert_eq!(out, format!("{NEW}\n"));
    assert_eq!(err, "Could not reach Tidal\n");
}

/// AC4: only what was given.
#[test]
fn ac4_edit() {
    let edit = |title: Option<&str>, description: Option<&str>| LibraryRequest::EditPlaylist {
        uuid: MINE.into(),
        title: title.map(Into::into),
        description: description.map(Into::into),
    };
    let public = |public| LibraryRequest::SetPlaylistPublic {
        uuid: MINE.into(),
        public,
    };
    let rows: Vec<(Vec<&str>, Vec<LibraryRequest>)> = vec![
        (vec!["--name", "A"], vec![edit(Some("A"), None)]),
        (vec!["--description", ""], vec![edit(None, Some(""))]),
        (vec!["--public"], vec![public(true)]),
        (vec!["--private"], vec![public(false)]),
        (
            vec!["--name", "A", "--description", "B", "--public"],
            vec![edit(Some("A"), Some("B")), public(true)],
        ),
    ];
    for (flags, want) in rows {
        let mut player = Player::default().with(summary(MINE, "Mine", true), &[]);
        let args = [vec!["edit", MINE], flags.clone()].concat();
        let (code, out, err) = run_it(&mut player, &args);
        assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""), "{flags:?}");
        assert_eq!(player.writes_sent(), want, "{flags:?}");
    }
}

/// AC5: a followed playlist is refused before any write.
#[test]
fn ac5_not_your_playlist() {
    let rows: [Vec<&str>; 3] = [
        vec!["edit", THEIRS, "--name", "A"],
        vec!["delete", THEIRS, "--yes"],
        vec!["import", MINE, THEIRS],
    ];
    for args in rows {
        let mut player = Player::default()
            .with(summary(MINE, "Mine", true), &[1])
            .with(summary(THEIRS, "Theirs", false), &[]);
        let (code, _, err) = run_it(&mut player, &args);
        assert_eq!(code, 1, "{args:?}");
        assert_eq!(err, "Theirs is not your playlist\n", "{args:?}");
        assert_eq!(player.writes_sent(), vec![], "{args:?}");
    }
}

/// AC6: asks on a terminal, refuses without one.
#[test]
fn ac6_delete() {
    let delete = vec![LibraryRequest::DeletePlaylist { uuid: MINE.into() }];
    // (flags, the terminal, exit code, deleted, what stderr holds)
    type Row<'a> = (Vec<&'a str>, Option<&'a str>, u8, bool, &'a str);
    let rows: Vec<Row> = vec![
        (vec!["--yes"], None, 0, true, ""),
        (vec![], Some("y\n"), 0, true, "Delete Mine? (y/n) "),
        (vec![], Some("n\n"), 0, false, "Delete Mine? (y/n) "),
        (vec![], Some(""), 0, false, "Delete Mine? (y/n) "),
        (
            vec![],
            None,
            2,
            false,
            "Use --yes to delete without a terminal\n",
        ),
    ];
    for (flags, typed, code, deleted, stderr) in rows {
        let mut player = Player::default().with(summary(MINE, "Mine", true), &[]);
        let args = [vec!["delete", MINE], flags].concat();
        let got = run_with(&mut player, &args, typed);
        assert_eq!((got.0, got.2.as_str()), (code, stderr), "{typed:?}");
        let want = if deleted { delete.clone() } else { vec![] };
        assert_eq!(player.writes_sent(), want, "{typed:?}");
        if typed.is_none() && code == 2 {
            assert_eq!(player.sent, vec![], "nothing sent");
        }
    }
}

/// AC7: the description and the default name, then the tracks.
#[test]
fn ac7_fork() {
    let mut player = Player::default().with(
        PlaylistSummary {
            description: Some("Words".into()),
            ..summary(THEIRS, "Theirs", false)
        },
        &[1, 2],
    );
    let (code, out, err) = run_it(&mut player, &["fork", THEIRS]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("{NEW}\n"));
    let mut want = vec![LibraryRequest::CreatePlaylist {
        title: "Theirs (copy)".into(),
        description: "Words".into(),
    }];
    want.push(LibraryRequest::AddToPlaylist {
        uuid: NEW.into(),
        tracks: vec![TrackId(1), TrackId(2)],
        allow_duplicates: true,
    });
    assert_eq!(player.writes_sent(), want);

    let mut named = Player::default().with(summary(THEIRS, "Theirs", false), &[]);
    run_it(&mut named, &["fork", THEIRS, "--name", "Mine now"]);
    assert_eq!(
        named.writes_sent(),
        vec![LibraryRequest::CreatePlaylist {
            title: "Mine now".into(),
            description: String::new(),
        }],
        "an empty source: created, nothing added"
    );
}

/// AC7: every page read, 100 tracks per request, in order.
#[test]
fn ac7_fork_batches() {
    let ids: Vec<u64> = (1..=250).collect();
    let mut player = Player::default().with(summary(MINE, "Mine", true), &ids);
    let (code, _, _) = run_it(&mut player, &["fork", MINE]);
    assert_eq!(code, 0);
    let got = player.writes_sent();
    assert_eq!(got[1..], adds(NEW, &[1..101, 101..201, 201..251]));
}

/// AC7: an empty source still makes a playlist.
#[test]
fn ac7_fork_empty() {
    let mut player = Player::default().with(summary(MINE, "Mine", true), &[]);
    let (code, out, _) = run_it(&mut player, &["fork", MINE]);
    assert_eq!((code, out), (0, format!("{NEW}\n")));
    assert_eq!(player.writes_sent().len(), 1);
}

/// AC8: what `TO` has and repeats in `FROM` are skipped, in `FROM`'s order.
#[test]
fn ac8_import() {
    let mut player = Player::default()
        .with(summary(THEIRS, "Theirs", false), &[3, 1, 2, 3, 4])
        .with(summary(MINE, "Mine", true), &[1, 9]);
    let (code, out, err) = run_it(&mut player, &["import", THEIRS, MINE]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "Added 3 tracks to Mine, skipped 2\n");
    assert_eq!(
        player.writes_sent(),
        vec![LibraryRequest::AddToPlaylist {
            uuid: MINE.into(),
            tracks: [3, 2, 4].map(TrackId).to_vec(),
            allow_duplicates: true,
        }]
    );

    // Nothing new: nothing sent.
    let mut again = Player::default()
        .with(summary(THEIRS, "Theirs", false), &[1])
        .with(summary(MINE, "Mine", true), &[1]);
    let (code, out, _) = run_it(&mut again, &["import", THEIRS, MINE]);
    assert_eq!(
        (code, out.as_str()),
        (0, "Added 0 tracks to Mine, skipped 1\n")
    );
    assert_eq!(again.writes_sent(), vec![]);
}

/// AC8: `--duplicates` adds them all.
#[test]
fn ac8_import_duplicates() {
    let mut player = Player::default()
        .with(summary(THEIRS, "Theirs", false), &[1, 1])
        .with(summary(MINE, "Mine", true), &[1]);
    let (code, out, _) = run_it(&mut player, &["import", THEIRS, MINE, "--duplicates"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "Added 2 tracks to Mine, skipped 0\n")
    );
    assert_eq!(
        player.writes_sent(),
        vec![LibraryRequest::AddToPlaylist {
            uuid: MINE.into(),
            tracks: [1, 1].map(TrackId).to_vec(),
            allow_duplicates: true,
        }]
    );
}

/// AC9: a total above the tracks read is videos.
#[test]
fn ac9_skipped_videos() {
    for (total, line) in [(3, "Skipped 1 video\n"), (4, "Skipped 2 videos\n")] {
        let mut player = Player::default().with_total(summary(MINE, "Mine", true), &[1, 2], total);
        let (code, _, err) = run_it(&mut player, &["fork", MINE]);
        assert_eq!((code, err.as_str()), (0, line));
    }
}

/// AC9: how far it got, and `fork`'s UUID.
#[test]
fn ac9_partial_failure() {
    let ids: Vec<u64> = (1..=250).collect();
    let failing = |player: Player, n| Player {
        fail_write: Some((n, "Could not reach Tidal".into())),
        ..player
    };
    // The create is write 0, so the second batch is write 2.
    let mut fork = failing(Player::default().with(summary(MINE, "Mine", true), &ids), 2);
    let (code, out, err) = run_it(&mut fork, &["fork", MINE]);
    assert_eq!(code, 1);
    assert_eq!(out, format!("{NEW}\n"));
    assert_eq!(
        err,
        "Added 100 of 250 tracks to Mine (copy): Could not reach Tidal\n"
    );

    let mut import = failing(
        Player::default()
            .with(summary(THEIRS, "Theirs", false), &ids)
            .with(summary(MINE, "Mine", true), &[]),
        1,
    );
    let (code, out, err) = run_it(&mut import, &["import", THEIRS, MINE]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(
        err,
        "Added 100 of 250 tracks to Mine: Could not reach Tidal\n"
    );
}

/// A player that does not answer, and one that does not exist in the list.
#[test]
fn ac10_no_answer_and_not_found() {
    struct Silent;
    impl Link for Silent {
        fn send(&mut self, _: &ClientMessage) -> io::Result<()> {
            Ok(())
        }
        fn recv(&mut self, _: Option<Duration>) -> Result<Option<ServerMessage>, RecvError> {
            Ok(None)
        }
        fn close(&mut self) {}
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let plan = Plan::List { json: false };
    let code = execute(&mut Silent, &plan, Duration::ZERO, &mut out, &mut err, None);
    assert_eq!(code, 1);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "The player did not answer\n"
    );

    let mut player = Player::default();
    let (code, _, err) = run_it(&mut player, &["fork", MINE]);
    assert_eq!(code, 1);
    assert_eq!(err, format!("Playlist {MINE} was not found\n"));
}
