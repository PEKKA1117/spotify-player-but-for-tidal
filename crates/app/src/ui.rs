//! Rendering of the pure UI model.

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};
use tidal_player_core::ui::State;

/// Draws `state` into `frame`: a bordered block titled with the app name.
pub fn render(state: &State, frame: &mut Frame) {
    let block = Block::bordered().title("tidal-player");
    let inner = block.inner(frame.area());
    frame.render_widget(block, frame.area());

    // The status line takes the last row inside the border, when there is one (AC17).
    if state.login_required && inner.height > 0 {
        let status_area = Rect {
            y: inner.bottom() - 1,
            height: 1,
            ..inner
        };
        let status_text = "Session expired — run \"tidal-player login\" in another terminal";
        frame.render_widget(Paragraph::new(status_text), status_area);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use tidal_player_core::protocol::{
        Event, InsertAt, NowPlaying, PlaybackState, PlayerSnapshot, QueueEntry, RepeatMode,
    };
    use tidal_player_core::ui::{Action, Prompt, update};
    use tidal_player_core::{AudioQuality, EntryId, Track, TrackId};

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .chunks(usize::from(buffer.area.width).max(1))
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn draw(state: &State, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(state, frame)).unwrap();
        buffer_text(&terminal)
    }

    fn row(text: &str, n: usize) -> &str {
        text.split('\n').nth(n).unwrap_or("")
    }

    #[test]
    fn ac6_empty_state_80x24() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render(&State::default(), frame))
            .unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("tidal-player"), "app name missing:\n{text}");
        insta::assert_snapshot!(text);
    }

    #[test]
    fn ac14_login_required_80x24() {
        let state = State {
            login_required: true,
            ..State::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(&state, frame)).unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("tidal-player login"),
            "status text missing:\n{text}"
        );
        insta::assert_snapshot!(text);
    }

    /// AC17: the status line never panics on a tiny terminal and is only
    /// drawn when there is an inner row for it.
    #[test]
    fn ac17_login_status_tiny_terminals() {
        let state = State {
            login_required: true,
            ..State::default()
        };
        for (width, height) in [(80, 0), (80, 1), (80, 2), (80, 3), (1, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(&state, frame)).unwrap();
            let text = buffer_text(&terminal);
            let rows: Vec<&str> = text.split('\n').collect();
            if height < 3 {
                assert!(
                    !text.contains("Session expired"),
                    "{width}x{height}: status drawn without an inner row:\n{text}"
                );
            }
            if (width, height) == (80, 3) {
                assert!(
                    rows[1].contains("Session expired"),
                    "{width}x{height}: status not on the inner row:\n{text}"
                );
            }
        }
    }

    // --- spec 0004 AC23 -----------------------------------------------------------

    fn track(id: u64, title: &str, artist: &str, album: &str, secs: Option<u64>) -> Track {
        Track {
            id: TrackId(id),
            title: title.into(),
            artists: vec![artist.into()],
            album: Some(album.into()),
            duration: secs.map(Duration::from_secs),
            streamable: true,
        }
    }

    /// An album queue with three autoplay suggestions at its end.
    fn album_queue() -> Vec<QueueEntry> {
        let ptv = "Pierce The Veil";
        let album = "Collide With The Sky";
        let tracks = [
            ("May These Noises Startle You In Your Sleep Tonight", 241),
            ("Hell Above", 212),
            ("A Match Into Water", 262),
            ("King For A Day", 232),
            ("Bulls In The Bronx", 290),
            ("Props & Mayhem", 213),
            ("Tangled In The Great Escape", 304),
            ("Stained Glass Eyes And Colorful Tears", 233),
            ("I'm Low On Gas And You Need A Jacket", 223),
        ];
        let mut queue: Vec<QueueEntry> = tracks
            .iter()
            .enumerate()
            .map(|(i, (title, secs))| QueueEntry {
                id: EntryId(i as u64 + 1),
                track: track(1000 + i as u64, title, ptv, album, Some(*secs)),
                suggested: false,
            })
            .collect();
        let suggested = [
            (
                "If I'm James Dean, You're Audrey Hepburn",
                "Sleeping With Sirens",
                "With Ears To See And Eyes To Hear",
                219,
            ),
            (
                "Can You Feel My Heart",
                "Bring Me The Horizon",
                "Sempiternal",
                228,
            ),
            (
                "All I Want",
                "A Day To Remember",
                "What Separates Me From You",
                201,
            ),
        ];
        queue.extend(
            suggested
                .iter()
                .enumerate()
                .map(|(i, (title, artist, album, secs))| QueueEntry {
                    id: EntryId(20 + i as u64),
                    track: track(2000 + i as u64, title, artist, album, Some(*secs)),
                    suggested: true,
                }),
        );
        queue
    }

    fn now_playing(reason: Option<&str>) -> NowPlaying {
        NowPlaying {
            quality: AudioQuality::Lossless,
            source: "FLAC 16-bit 44.1 kHz stereo".into(),
            output: "hw:1,0 (exclusive) S32_LE 44.1 kHz 2 ch".into(),
            bit_perfect: reason.is_none(),
            bit_perfect_reason: reason.map(Into::into),
        }
    }

    /// Playing "Hell Above" at 1:23 with shuffle, repeat `queue` and 80 %.
    fn playing() -> PlayerSnapshot {
        PlayerSnapshot {
            queue: album_queue(),
            current: Some(EntryId(2)),
            state: PlaybackState::Playing,
            position: Duration::from_secs(83),
            shuffle: true,
            repeat: RepeatMode::Queue,
            autoplay: false,
            volume: 80,
            muted: false,
            now_playing: Some(now_playing(Some("volume below 100%"))),
            message: None,
        }
    }

    fn state_of(snapshot: PlayerSnapshot) -> State {
        let mut state = State::default();
        update(&mut state, Action::Player(Event::Player(snapshot)));
        state
    }

    fn assert_contains(text: &str, parts: &[&str]) {
        for part in parts {
            assert!(text.contains(part), "{part:?} missing:\n{text}");
        }
    }

    #[test]
    fn ac23_playback_window_nothing_playing() {
        let snapshot = PlayerSnapshot {
            queue: vec![],
            current: None,
            state: PlaybackState::Stopped,
            position: Duration::ZERO,
            now_playing: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume: 100,
            ..playing()
        };
        let text = draw(&state_of(snapshot), 80, 24);
        assert_contains(row(&text, 1), &["Nothing playing", "100%"]);
        assert_contains(&text, &["Queue (0)"]);
        assert!(!text.contains('━'), "a bar with nothing playing:\n{text}");
        insta::assert_snapshot!(text);
    }

    #[test]
    fn ac23_playback_window_playing() {
        let text = draw(&state_of(playing()), 80, 24);
        assert_contains(
            row(&text, 1),
            &[
                "▶ Hell Above · Pierce The Veil",
                "shuffle",
                "repeat: queue",
                "80%",
            ],
        );
        assert!(!row(&text, 1).contains("autoplay"), "{text}");
        assert_contains(row(&text, 2), &["Collide With The Sky"]);
        assert_contains(
            row(&text, 3),
            &[
                "LOSSLESS FLAC 16-bit 44.1 kHz",
                "hw:1,0",
                "not bit-perfect: volume below 100%",
            ],
        );
        assert_contains(row(&text, 4), &["━", "─", "1:23 / 3:32"]);
        assert_contains(&text, &["Queue (12)", "▶ 2", "Suggested", "All I Want"]);
        insta::assert_snapshot!(text);
    }

    #[test]
    fn ac23_playback_window_paused() {
        let snapshot = PlayerSnapshot {
            state: PlaybackState::Paused,
            shuffle: false,
            repeat: RepeatMode::Track,
            autoplay: true,
            muted: true,
            now_playing: Some(now_playing(Some("muted"))),
            ..playing()
        };
        let text = draw(&state_of(snapshot), 80, 24);
        assert_contains(
            row(&text, 1),
            &["⏸ Hell Above", "repeat: track", "autoplay", "muted"],
        );
        assert!(!row(&text, 1).contains("shuffle"), "{text}");
        assert!(!row(&text, 1).contains("80%"), "volume shown when muted");
        assert_contains(row(&text, 3), &["not bit-perfect: muted"]);
        insta::assert_snapshot!(text);
    }

    #[test]
    fn ac23_playback_window_loading() {
        let snapshot = PlayerSnapshot {
            state: PlaybackState::Loading,
            position: Duration::ZERO,
            now_playing: None,
            repeat: RepeatMode::Off,
            shuffle: false,
            volume: 100,
            ..playing()
        };
        let text = draw(&state_of(snapshot), 80, 24);
        assert_contains(row(&text, 1), &["… Hell Above", "100%"]);
        assert_contains(row(&text, 4), &["0:00 / 3:32"]);
        insta::assert_snapshot!(text);
    }

    #[test]
    fn ac23_playback_window_failure() {
        let snapshot = PlayerSnapshot {
            state: PlaybackState::Stopped,
            message: Some("Output hw:1,0 was lost".into()),
            ..playing()
        };
        let text = draw(&state_of(snapshot), 80, 24);
        assert_contains(row(&text, 1), &["■ Hell Above"]);
        assert_contains(row(&text, 3), &["Output hw:1,0 was lost"]);
        assert!(
            !row(&text, 3).contains("FLAC"),
            "details beside the message"
        );
        insta::assert_snapshot!(text);
    }

    /// Tidalt bug 6: an unknown duration draws no bar, not a full one.
    #[test]
    fn ac23_playback_window_unknown_duration() {
        let mut snapshot = playing();
        snapshot.queue[1].track.duration = None;
        let text = draw(&state_of(snapshot), 80, 24);
        let progress = row(&text, 4);
        assert_contains(progress, &["1:23 / ?:??"]);
        assert!(
            !progress.contains('━') && !progress.contains('─'),
            "a bar for an unknown duration:\n{text}"
        );
        insta::assert_snapshot!(text);
    }

    /// A queue longer than the window keeps the current entry in view.
    #[test]
    fn ac23_queue_scrolls() {
        let queue: Vec<QueueEntry> = (1..=40)
            .map(|i| QueueEntry {
                id: EntryId(i),
                track: track(i, &format!("Track {i}"), "Artist", "Album", Some(180 + i)),
                suggested: false,
            })
            .collect();
        let mut state = state_of(PlayerSnapshot {
            queue: queue.clone(),
            current: Some(EntryId(1)),
            ..playing()
        });
        update(
            &mut state,
            Action::Player(Event::Player(PlayerSnapshot {
                queue,
                current: Some(EntryId(30)),
                ..playing()
            })),
        );
        let text = draw(&state, 80, 24);
        assert_contains(&text, &["Queue (40)", "▶ 30  Track 30"]);
        assert!(
            !text.contains("Track 1 "),
            "the top stayed in view:\n{text}"
        );
        insta::assert_snapshot!(text);
    }

    /// 40×12: the album column goes first, then the artist; truncated
    /// with `…`.
    #[test]
    fn ac23_truncation_40x12() {
        let text = draw(&state_of(playing()), 40, 12);
        assert_contains(&text, &["▶ Hell Above", "…", "Pierce"]);
        // The queue rows: no album column.
        let rows: Vec<&str> = text.split('\n').skip(6).collect();
        assert!(
            rows.iter().all(|r| !r.contains("Collide")),
            "album column in the queue:\n{text}"
        );
        assert!(
            rows.iter().any(|r| r.contains("Pierce")),
            "artist column dropped too early:\n{text}"
        );
        insta::assert_snapshot!(text);
    }

    /// No panic at any size, 0×0 included, in every state; the playback
    /// window's first row shows the state symbol whenever it has room.
    #[test]
    fn ac23_no_panic_any_size() {
        let mut prompt = state_of(playing());
        prompt.prompt = Some(Prompt {
            at: InsertAt::End,
            text: "https://tidal.com/browse/album/10".into(),
        });
        let mut login = state_of(playing());
        login.login_required = true;
        let states = [
            State::default(),
            state_of(playing()),
            prompt,
            login,
            state_of(PlayerSnapshot {
                message: Some("Output hw:1,0 was lost".into()),
                ..playing()
            }),
        ];
        let sizes = (0..=120)
            .step_by(7)
            .flat_map(|w| (0..=40).step_by(3).map(move |h| (w, h)))
            .chain([
                (0, 0),
                (1, 1),
                (2, 2),
                (3, 3),
                (40, 12),
                (80, 24),
                (120, 40),
            ]);
        for (width, height) in sizes {
            for (i, state) in states.iter().enumerate() {
                let text = draw(state, width, height);
                if i == 1 && width >= 20 && height >= 3 {
                    assert!(
                        row(&text, 1).contains('▶'),
                        "{width}x{height}: no state symbol:\n{text}"
                    );
                }
            }
        }
    }

    /// AC28: the open prompt is drawn over the queue's top row.
    #[test]
    fn ac28_prompt_rendered() {
        let mut state = state_of(playing());
        state.prompt = Some(Prompt {
            at: InsertAt::Next,
            text: "https://tidal.com/browse/album/10".into(),
        });
        let text = draw(&state, 80, 24);
        assert_contains(
            row(&text, 5),
            &["Play next: https://tidal.com/browse/album/10"],
        );
        insta::assert_snapshot!(text);
    }
}
