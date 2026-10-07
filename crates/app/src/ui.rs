//! Rendering of the pure UI model (spec 0004 "TUI"): one bordered frame
//! titled `tidal-player`, the playback window (4 rows) at the top and the
//! queue below it. The two never share rows: the queue list always spans
//! the full width of its own rows.

use std::time::Duration;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use tidal_player_core::protocol::{InsertAt, NowPlaying, PlaybackState, QueueEntry, RepeatMode};
use tidal_player_core::ui::State;

/// Rows of the playback window, inside the frame.
const PLAYBACK_ROWS: u16 = 4;
/// Below this many rows inside the frame only the playback window is drawn.
const MIN_ROWS_WITH_QUEUE: u16 = 6;
/// The narrowest the title side of a row gets before the indicators (or
/// the bit-perfect verdict) make way for it.
const MIN_LEFT: usize = 20;
const STATUS: &str = "Session expired — run \"tidal-player login\" in another terminal";

/// Draws `state` into `frame`.
pub fn render(state: &State, frame: &mut Frame) {
    let area = frame.area();
    let block = Block::bordered().title("tidal-player");
    let mut inner = block.inner(area);
    frame.render_widget(block, area);

    // The status line takes the last row inside the border, when there is
    // one (0002 AC17).
    if state.login_required && inner.height > 0 {
        inner.height -= 1;
        let status = Rect {
            y: inner.bottom(),
            height: 1,
            ..inner
        };
        frame.render_widget(Paragraph::new(STATUS), status);
    }

    let playback = Rect {
        height: inner.height.min(PLAYBACK_ROWS),
        ..inner
    };
    render_playback(state, frame, playback);

    let prompt_row = if inner.height >= MIN_ROWS_WITH_QUEUE {
        let queue = Rect {
            y: inner.y + PLAYBACK_ROWS,
            height: inner.height - PLAYBACK_ROWS,
            ..inner
        };
        render_queue(state, frame, queue);
        Rect { height: 1, ..queue }
    } else {
        // No queue: the prompt takes the playback window's last row.
        Rect {
            y: playback.bottom().saturating_sub(1),
            height: playback.height.min(1),
            ..playback
        }
    };
    render_prompt(state, frame, prompt_row);
}

// --- the playback window -----------------------------------------------------------

fn render_playback(state: &State, frame: &mut Frame, area: Rect) {
    let width = usize::from(area.width);
    let entry = state.current();
    let mut lines = vec![Line::styled(
        header(state, entry, width),
        Style::new().add_modifier(Modifier::BOLD),
    )];
    if let Some(entry) = entry {
        let album = entry.track.album_title().unwrap_or_default();
        lines.push(Line::raw(fit(&format!("  {album}"), width)));
        lines.push(match state.message() {
            Some(message) => Line::styled(
                fit(&format!("  {message}"), width),
                Style::new().fg(Color::Red),
            ),
            None => Line::raw(
                state
                    .player
                    .as_ref()
                    .and_then(|p| p.now_playing.as_ref())
                    .map(|np| details(np, width))
                    .unwrap_or_default(),
            ),
        });
        lines.push(Line::raw(progress(
            state.position,
            entry.track.duration,
            width,
        )));
    } else if let Some(message) = state.message() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            fit(&format!("  {message}"), width),
            Style::new().fg(Color::Red),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// `▶ Title · Artists` on the left, the indicators flush right; the
/// indicators shrink to the volume, then go, before the title gets
/// narrower than [`MIN_LEFT`].
fn header(state: &State, entry: Option<&QueueEntry>, width: usize) -> String {
    let left = match (entry, state.player.as_ref()) {
        (Some(entry), Some(player)) => {
            let symbol = match player.state {
                PlaybackState::Playing => "▶",
                PlaybackState::Paused => "⏸",
                PlaybackState::Loading | PlaybackState::Buffering => "…",
                PlaybackState::Stopped => "■",
            };
            let artists = entry.track.artist_names();
            if artists.is_empty() {
                format!("{symbol} {}", entry.track.title)
            } else {
                format!("{symbol} {} · {artists}", entry.track.title)
            }
        }
        _ => "Nothing playing".to_owned(),
    };
    let Some(player) = state.player.as_ref() else {
        return fit(&left, width);
    };
    let volume = if player.muted {
        "muted".to_owned()
    } else {
        format!("{}%", player.volume)
    };
    let mut all = Vec::new();
    if player.shuffle {
        all.push("shuffle".to_owned());
    }
    match player.repeat {
        RepeatMode::Off => {}
        RepeatMode::Queue => all.push("repeat: queue".to_owned()),
        RepeatMode::Track => all.push("repeat: track".to_owned()),
    }
    if player.autoplay {
        all.push("autoplay".to_owned());
    }
    all.push(volume.clone());
    [all.join("  "), volume]
        .iter()
        .find_map(|right| split_row(&left, right, width))
        .unwrap_or_else(|| fit(&left, width))
}

/// `left` and `right` on one row of `width`, `right` flush right; `None`
/// when `right` would leave `left` fewer than [`MIN_LEFT`] columns it needs.
fn split_row(left: &str, right: &str, width: usize) -> Option<String> {
    let (lw, rw) = (text_width(left), text_width(right));
    if lw + 1 + rw <= width {
        return Some(format!("{left}{}{right}", " ".repeat(width - lw - rw)));
    }
    if width >= rw + 1 + MIN_LEFT {
        let left = fit(left, width - rw - 1);
        let pad = width - text_width(&left) - rw;
        return Some(format!("{left}{}{right}", " ".repeat(pad)));
    }
    None
}

/// `LOSSLESS FLAC 16-bit 44.1 kHz → hw:1,0 · not bit-perfect: <reason>`:
/// the output by its device (0003's Output line has the rest), and the
/// usual stereo left unsaid, then ` · device released` while the engine
/// released the output (spec 0005). A short row keeps the verdict: the
/// format is cut first, then left out.
fn details(np: &NowPlaying, width: usize) -> String {
    let source = np.source.strip_suffix(" stereo").unwrap_or(&np.source);
    let device = np.output.split_whitespace().next().unwrap_or_default();
    let left = format!("{} {source} → {device}", np.quality);
    let verdict = match (np.bit_perfect, np.bit_perfect_reason.as_deref()) {
        (true, _) => "bit-perfect".to_owned(),
        (false, Some(reason)) => format!("not bit-perfect: {reason}"),
        (false, None) => "not bit-perfect".to_owned(),
    };
    let verdict = if np.released {
        format!("{verdict} · device released")
    } else {
        verdict
    };
    let full = format!("  {left} · {verdict}");
    let vw = text_width(&verdict);
    if text_width(&full) <= width {
        full
    } else if width >= 2 + vw + 3 + 10 {
        format!("  {} · {verdict}", fit(&left, width - 2 - 3 - vw))
    } else {
        fit(&format!("  {verdict}"), width)
    }
}

/// `━━━━━────  1:23 / 3:32`; no bar when the duration is unknown
/// (`1:23 / ?:??`, tidalt bug 6).
fn progress(position: Duration, duration: Option<Duration>, width: usize) -> String {
    let time = format!(
        "{} / {}",
        clock(position),
        duration.map_or_else(|| "?:??".to_owned(), clock)
    );
    let tw = text_width(&time);
    match duration {
        Some(duration) if width > 2 + 2 + tw => {
            let bar = width - 2 - 2 - tw;
            let done = if duration.is_zero() {
                0
            } else {
                let ratio = position.as_secs_f64() / duration.as_secs_f64();
                ((bar as f64 * ratio).round() as usize).min(bar)
            };
            format!("  {}{}  {time}", "━".repeat(done), "─".repeat(bar - done))
        }
        _ if width >= tw => format!("{}{time}", " ".repeat(width - tw)),
        _ => fit(&time, width),
    }
}

/// `m:ss`, or `h:mm:ss` from an hour.
pub(crate) fn clock(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// --- the queue -------------------------------------------------------------------------

/// One row of the queue list.
enum Row<'a> {
    /// Before a run of autoplay suggestions.
    Suggested,
    Entry(usize, &'a QueueEntry),
}

fn render_queue(state: &State, frame: &mut Frame, area: Rect) {
    let queue = state.queue();
    let block = Block::bordered().title(format!("Queue ({})", queue.len()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut rows = Vec::with_capacity(queue.len() + 1);
    for (i, entry) in queue.iter().enumerate() {
        if entry.suggested && (i == 0 || !queue[i - 1].suggested) {
            rows.push(Row::Suggested);
        }
        rows.push(Row::Entry(i, entry));
    }

    // Keep the anchor (the current entry when it changed, the cursor when
    // it moved) in the middle of the view once the list scrolls.
    let height = usize::from(inner.height);
    let current = state.player.as_ref().and_then(|p| p.current);
    let anchor = [state.anchor, state.cursor, current]
        .into_iter()
        .flatten()
        .find_map(|id| {
            rows.iter()
                .position(|r| matches!(r, Row::Entry(_, e) if e.id == id))
        })
        .unwrap_or(0);
    let offset = anchor
        .saturating_sub(height / 2)
        .min(rows.len().saturating_sub(height));

    let width = usize::from(inner.width);
    let columns = Columns::new(width, queue.len());
    let lines: Vec<Line> = rows
        .iter()
        .skip(offset)
        .take(height)
        .map(|row| match row {
            Row::Suggested => Line::styled(
                fit(&divider(width), width),
                Style::new().add_modifier(Modifier::DIM),
            ),
            Row::Entry(i, entry) => {
                let mut style = Style::new();
                if entry.suggested {
                    style = style.add_modifier(Modifier::DIM);
                }
                if Some(entry.id) == current {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if Some(entry.id) == state.cursor {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                Line::styled(columns.row(*i, entry, Some(entry.id) == current), style)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// `  ── Suggested ─────…`, across `width`.
fn divider(width: usize) -> String {
    let label = "  ── Suggested ";
    format!(
        "{label}{}",
        "─".repeat(width.saturating_sub(text_width(label)))
    )
}

/// The queue's column widths: `▶ ` + number, title, artist, album,
/// duration. Narrow rows drop the album column first, then the artist.
struct Columns {
    width: usize,
    number: usize,
    title: usize,
    artist: Option<usize>,
    album: Option<usize>,
}

/// The duration column (`12:34`).
const DURATION: usize = 5;

impl Columns {
    fn new(width: usize, entries: usize) -> Self {
        let number = entries.max(1).to_string().len();
        let text = width.saturating_sub(2 + number + 2 + 2 + DURATION);
        let (title, artist, album) = if text >= 48 {
            let avail = text - 4;
            let title = avail * 2 / 5;
            let artist = avail * 3 / 10;
            (title, Some(artist), Some(avail - title - artist))
        } else if text >= 20 {
            let avail = text - 2;
            let title = avail * 3 / 5;
            (title, Some(avail - title), None)
        } else {
            (text, None, None)
        };
        Self {
            width,
            number,
            title,
            artist,
            album,
        }
    }

    fn row(&self, index: usize, entry: &QueueEntry, current: bool) -> String {
        let track = &entry.track;
        let marker = if current { "▶ " } else { "  " };
        let mut row = format!(
            "{marker}{:<n$}  {}",
            index + 1,
            pad(&fit(&track.title, self.title), self.title),
            n = self.number
        );
        if let Some(w) = self.artist {
            row += &format!("  {}", pad(&fit(&track.artist_names(), w), w));
        }
        if let Some(w) = self.album {
            let album = track.album_title().unwrap_or_default();
            row += &format!("  {}", pad(&fit(album, w), w));
        }
        let duration = track.duration.map(clock).unwrap_or_default();
        row += &format!("  {duration:>DURATION$}");
        pad(&fit(&row, self.width), self.width)
    }
}

// --- the open prompt -------------------------------------------------------------------

/// The open prompt, over `area` (one row): `Add to queue: ` or
/// `Play next: ` and the text, its end kept in view, with the cursor.
fn render_prompt(state: &State, frame: &mut Frame, area: Rect) {
    let Some(prompt) = state.prompt.as_ref() else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
    let label = match prompt.at {
        InsertAt::End => "Add to queue: ",
        InsertAt::Next => "Play next: ",
    };
    let width = usize::from(area.width);
    // One column stays free for the cursor.
    let room = width.saturating_sub(text_width(label) + 1);
    let text = if text_width(&prompt.text) <= room {
        prompt.text.clone()
    } else {
        format!("…{}", tail(&prompt.text, room.saturating_sub(1)))
    };
    let line = fit(&format!("{label}{text}"), width);
    let x = area.x + u16::try_from(text_width(&line)).unwrap_or(area.width);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            line,
            Style::new().add_modifier(Modifier::BOLD),
        )])),
        area,
    );
    frame.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
}

// --- text ------------------------------------------------------------------------------

/// Display columns of `text`.
fn text_width(text: &str) -> usize {
    Span::raw(text).width()
}

fn char_width(c: char) -> usize {
    text_width(c.encode_utf8(&mut [0; 4]))
}

/// `text` cut to `width` columns, ending in `…` when cut.
fn fit(text: &str, width: usize) -> String {
    if text_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = char_width(c);
        if used + w > width - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    let mut out = out.trim_end().to_owned();
    out.push('…');
    out
}

/// The last `width` columns of `text`.
fn tail(text: &str, width: usize) -> String {
    let mut out: Vec<char> = Vec::new();
    let mut used = 0;
    for c in text.chars().rev() {
        let w = char_width(c);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.into_iter().rev().collect()
}

/// `text` padded with spaces to `width` columns.
fn pad(text: &str, width: usize) -> String {
    let w = text_width(text);
    format!("{text}{}", " ".repeat(width.saturating_sub(w)))
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
    use tidal_player_core::{AlbumRef, ArtistRef, AudioQuality, EntryId, Track, TrackId};

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
            version: None,
            artists: vec![ArtistRef {
                id: id + 1000,
                name: artist.into(),
            }],
            album: Some(AlbumRef {
                id: id + 2000,
                title: album.into(),
            }),
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
            released: false,
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

    /// 0005 AC22: paused with the device released, the third row ends
    /// with ` · device released`.
    #[test]
    fn ac22_released_row() {
        let snapshot = PlayerSnapshot {
            state: PlaybackState::Paused,
            volume: 100,
            now_playing: Some(NowPlaying {
                released: true,
                ..now_playing(None)
            }),
            ..playing()
        };
        let text = draw(&state_of(snapshot.clone()), 80, 24);
        let third = row(&text, 3).trim_end_matches('│').trim_end();
        assert!(
            third.ends_with("· bit-perfect · device released"),
            "third row: {third:?}\n{text}"
        );
        insta::assert_snapshot!(text);

        // Not released: no note.
        let snapshot = PlayerSnapshot {
            now_playing: Some(now_playing(None)),
            ..snapshot
        };
        let text = draw(&state_of(snapshot), 80, 24);
        assert!(!row(&text, 3).contains("released"), "{text}");
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

    // --- spec 0006 AC17: pages ----------------------------------------------------

    use ratatui::style::Modifier as Mod;
    use tidal_player_core::library::{
        AlbumKind, AlbumSummary, CreditedTrack, LibraryResponse, ListItems, ListPage, PageData,
        PlaylistSummary, RoleCategory,
    };
    use tidal_player_core::ui::{Effect, Key};

    fn press(state: &mut State, keys: &[Key]) -> Vec<Effect> {
        keys.iter()
            .flat_map(|key| update(state, Action::Key(*key)))
            .collect()
    }

    /// The ID of the one library request the keys made.
    fn ask(state: &mut State, keys: &[Key]) -> u64 {
        let effects = press(state, keys);
        effects
            .iter()
            .find_map(|e| match e {
                Effect::Library { id, .. } => Some(*id),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no library request from {keys:?}: {effects:?}"))
    }

    fn answer(state: &mut State, id: u64, response: LibraryResponse) {
        update(
            state,
            Action::LibraryReply {
                id,
                result: Ok(response),
            },
        );
    }

    fn fail(state: &mut State, id: u64, message: &str) {
        update(
            state,
            Action::LibraryReply {
                id,
                result: Err(message.into()),
            },
        );
    }

    fn list<T>(items: Vec<T>, total: u32, hidden: u32) -> ListPage<T> {
        ListPage {
            items,
            offset: 0,
            total,
            hidden,
        }
    }

    fn playlist(uuid: &str, title: &str, tracks: u32, own: bool) -> PlaylistSummary {
        PlaylistSummary {
            uuid: uuid.into(),
            title: title.into(),
            tracks: Some(tracks),
            duration: Some(Duration::from_secs(u64::from(tracks) * 200)),
            own,
        }
    }

    fn album(id: u64, title: &str, artist: &str, year: u16, kind: AlbumKind) -> AlbumSummary {
        AlbumSummary {
            id,
            title: title.into(),
            artists: vec![ArtistRef {
                id: 7,
                name: artist.into(),
            }],
            year: Some(year),
            kind,
            tracks: Some(9),
            duration: Some(Duration::from_secs(2400)),
        }
    }

    fn artist_ref(id: u64, name: &str) -> ArtistRef {
        ArtistRef {
            id,
            name: name.into(),
        }
    }

    fn library_data() -> PageData {
        PageData::Library {
            playlists: list(
                vec![
                    playlist("p1", "Running", 42, true),
                    playlist("p2", "Late night", 17, false),
                    playlist("p3", "Gym mix", 8, true),
                ],
                22,
                0,
            ),
            albums: list(
                vec![
                    album(
                        1,
                        "Collide With The Sky",
                        "Pierce The Veil",
                        2012,
                        AlbumKind::Album,
                    ),
                    album(
                        2,
                        "Misadventures",
                        "Pierce The Veil",
                        2016,
                        AlbumKind::Album,
                    ),
                    album(
                        3,
                        "Hold On Till May",
                        "Pierce The Veil",
                        2010,
                        AlbumKind::Ep,
                    ),
                ],
                14,
                0,
            ),
            artists: list(
                vec![
                    artist_ref(7, "Pierce The Veil"),
                    artist_ref(8, "Sleeping With Sirens"),
                    artist_ref(9, "Bring Me The Horizon"),
                ],
                196,
                0,
            ),
        }
    }

    fn empty_library() -> PageData {
        PageData::Library {
            playlists: list(vec![], 0, 0),
            albums: list(vec![], 0, 0),
            artists: list(vec![], 0, 0),
        }
    }

    /// The library page, loaded.
    fn library() -> State {
        let mut state = state_of(playing());
        let id = ask(&mut state, &[Key::Char('g'), Key::Char('l')]);
        answer(&mut state, id, LibraryResponse::Page(library_data()));
        state
    }

    fn browse_tracks(n: u64) -> Vec<Track> {
        (1..=n)
            .map(|i| {
                track(
                    100 + i,
                    &format!("Song {i}"),
                    "Pierce The Veil",
                    "Album",
                    Some(180 + i),
                )
            })
            .collect()
    }

    fn favorites() -> State {
        let mut state = state_of(playing());
        let id = ask(&mut state, &[Key::Char('g'), Key::Char('y')]);
        let mut tracks = browse_tracks(2);
        tracks.push(Track {
            streamable: false,
            ..track(
                500,
                "Not Streamable Here",
                "Pierce The Veil",
                "Misadventures",
                Some(201),
            )
        });
        answer(
            &mut state,
            id,
            LibraryResponse::Page(PageData::FavoriteTracks {
                tracks: list(tracks, 362, 0),
            }),
        );
        state
    }

    /// An album page, opened from the library's Albums window.
    fn album_page(tracks: Vec<Track>) -> State {
        let mut state = library();
        let n = tracks.len() as u32;
        let id = ask(&mut state, &[Key::Tab, Key::Enter]);
        answer(
            &mut state,
            id,
            LibraryResponse::Page(PageData::Album {
                album: album(
                    1,
                    "Collide With The Sky",
                    "Pierce The Veil",
                    2012,
                    AlbumKind::Album,
                ),
                tracks: list(tracks, n, 0),
            }),
        );
        state
    }

    fn playlist_page(tracks: Vec<Track>) -> State {
        let mut state = library();
        let n = tracks.len() as u32;
        let id = ask(&mut state, &[Key::Enter]);
        answer(
            &mut state,
            id,
            LibraryResponse::Page(PageData::Playlist {
                playlist: playlist("p1", "Running", n, true),
                etag: Some("\"1\"".into()),
                tracks: list(tracks, n, 0),
            }),
        );
        state
    }

    /// An artist page, opened from the library's Artists window.
    fn artist_page(full: bool) -> State {
        let mut state = library();
        let id = ask(&mut state, &[Key::Tab, Key::Tab, Key::Enter]);
        let data = if full {
            PageData::Artist {
                artist: artist_ref(7, "Pierce The Veil"),
                top_tracks: list(browse_tracks(4), 91, 0),
                albums: list(
                    vec![
                        album(
                            1,
                            "Collide With The Sky",
                            "Pierce The Veil",
                            2012,
                            AlbumKind::Album,
                        ),
                        album(
                            3,
                            "Hold On Till May",
                            "Pierce The Veil",
                            2010,
                            AlbumKind::Ep,
                        ),
                        album(
                            4,
                            "Pierce The Veil",
                            "Pierce The Veil",
                            2008,
                            AlbumKind::Single,
                        ),
                    ],
                    78,
                    0,
                ),
                appears_on: list(
                    vec![album(
                        9,
                        "Warped Tour 2013",
                        "Various Artists",
                        2013,
                        AlbumKind::Album,
                    )],
                    30,
                    0,
                ),
            }
        } else {
            PageData::Artist {
                artist: artist_ref(7, "Pierce The Veil"),
                top_tracks: list(vec![], 0, 0),
                albums: list(vec![], 0, 0),
                appears_on: list(vec![], 0, 0),
            }
        };
        answer(&mut state, id, LibraryResponse::Page(data));
        state
    }

    /// The artist page with *All tracks* focused and its first page in.
    fn all_tracks(empty: bool) -> State {
        let mut state = artist_page(!empty);
        let id = ask(&mut state, &[Key::Tab, Key::Tab, Key::Tab]);
        let page = if empty {
            list(vec![], 0, 37)
        } else {
            list(
                vec![
                    CreditedTrack {
                        track: track(300, "Hell Above", "Pierce The Veil", "Collide", Some(212)),
                        roles: vec![RoleCategory::Performer, RoleCategory::Songwriter],
                    },
                    CreditedTrack {
                        track: track(
                            301,
                            "Caraphernelia",
                            "Pierce The Veil",
                            "Selfish",
                            Some(241),
                        ),
                        roles: vec![RoleCategory::Producer],
                    },
                ],
                548,
                37,
            )
        };
        answer(
            &mut state,
            id,
            LibraryResponse::Items(ListItems::Credits(page)),
        );
        state
    }

    fn own_playlists_page() -> LibraryResponse {
        LibraryResponse::Items(ListItems::Playlists(list(
            vec![
                playlist("p1", "Running", 42, true),
                playlist("p3", "Gym mix", 8, true),
            ],
            2,
            0,
        )))
    }

    /// The actions popup over the library's Albums window.
    fn actions_popup() -> State {
        let mut state = library();
        press(&mut state, &[Key::Tab, Key::Ctrl(' ')]);
        state
    }

    /// *Add to playlist…* from an album's actions, its playlists in.
    fn add_to_playlist_popup() -> State {
        let mut state = actions_popup();
        // Open, Go to artist: …, Add to queue, Play next, Add to favorites,
        // Remove from favorites, Add to playlist…
        let keys = [Key::Char('j'); 6];
        press(&mut state, &keys);
        let id = ask(&mut state, &[Key::Enter]);
        answer(&mut state, id, own_playlists_page());
        state
    }

    fn confirm_popup() -> State {
        let mut state = library();
        // Open, Add to queue, Play next, Delete playlist.
        press(
            &mut state,
            &[
                Key::Ctrl(' '),
                Key::Char('j'),
                Key::Char('j'),
                Key::Char('j'),
            ],
        );
        press(&mut state, &[Key::Enter]);
        state
    }

    fn name_popup() -> State {
        let mut state = add_to_playlist_popup();
        press(&mut state, &[Key::Enter]);
        for c in "Road trip".chars() {
            press(&mut state, &[Key::Char(c)]);
        }
        state
    }

    fn roles_popup() -> State {
        let mut state = all_tracks(false);
        press(&mut state, &[Key::Char('f')]);
        state
    }

    fn draw_terminal(state: &State, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(state, frame)).unwrap();
        terminal
    }

    #[test]
    fn ac17_pages_80x24() {
        // Library loaded: three windows with counts, focus, `♥`.
        let text = draw(&library(), 80, 24);
        assert_contains(
            &text,
            &[
                "Library",
                "Playlists (22)",
                "Albums (14)",
                "Artists (196)",
                "Running",
                "♥ Late night",
                "Collide With The Sky",
                "Pierce The Veil",
            ],
        );
        assert!(
            !text.contains("Queue (12)"),
            "the queue under the library:\n{text}"
        );
        insta::assert_snapshot!("ac17_library_loaded", text);

        // Library loading.
        let mut state = state_of(playing());
        press(&mut state, &[Key::Char('g'), Key::Char('l')]);
        let text = draw(&state, 80, 24);
        assert_contains(&text, &["Library", "Loading…"]);
        assert!(!text.contains("Queue (12)"), "{text}");
        insta::assert_snapshot!("ac17_library_loading", text);

        // Favorite tracks: a non-streamable row dimmed, `Loading more…` last.
        let mut state = favorites();
        press(&mut state, &[Key::Char('j')]);
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &[
                "Favorite tracks · 362 tracks",
                "Song 1",
                "Not Streamable Here",
                "Loading more…",
            ],
        );
        let y = text
            .split('\n')
            .position(|r| r.contains("Not Streamable Here"))
            .unwrap();
        let line = text.split('\n').nth(y).unwrap();
        let x = line[..line.find("Not Streamable Here").unwrap()]
            .chars()
            .count();
        let terminal = draw_terminal(&state, 80, 24);
        let cell = &terminal.backend().buffer()[(x as u16, y as u16)];
        assert!(cell.modifier.contains(Mod::DIM), "not dimmed: {cell:?}");
        let song = text.split('\n').position(|r| r.contains("Song 1")).unwrap();
        let cell = &terminal.backend().buffer()[(x as u16, song as u16)];
        assert!(!cell.modifier.contains(Mod::DIM), "streamable row dimmed");
        let last = text
            .split('\n')
            .rev()
            .find(|r| r.contains("Song") || r.contains("Loading more…"))
            .unwrap();
        assert!(last.contains("Loading more…"), "last row: {last:?}\n{text}");
        insta::assert_snapshot!("ac17_favorite_tracks", text);

        // An album page: the title row.
        let text = draw(&album_page(browse_tracks(9)), 80, 24);
        assert_contains(
            &text,
            &[
                "Collide With The Sky · Pierce The Veil · 2012 · 9 tracks · 40:00",
                "Song 1",
            ],
        );
        insta::assert_snapshot!("ac17_album_page", text);

        // A playlist page.
        let text = draw(&playlist_page(browse_tracks(3)), 80, 24);
        assert_contains(&text, &["Running · 3 tracks · 10:00", "Song 3"]);
        insta::assert_snapshot!("ac17_playlist_page", text);

        // The artist page: both halves, `‹Tab›` titles.
        let text = draw(&artist_page(true), 80, 24);
        assert_contains(
            &text,
            &[
                "Pierce The Veil",
                "Top tracks (91) ‹Tab› All tracks",
                "Albums (78) ‹Tab› Appears on",
                "Song 1",
                "Hold On Till May",
                "EP",
                "Single",
            ],
        );
        insta::assert_snapshot!("ac17_artist_page", text);

        // *All tracks* focused: the hidden count, the roles.
        let text = draw(&all_tracks(false), 80, 24);
        assert_contains(
            &text,
            &[
                "All tracks (548 · 37 hidden) ‹Tab› Top tracks",
                "Hell Above",
                "Albums (78)",
            ],
        );
        insta::assert_snapshot!("ac17_artist_all_tracks", text);

        // The role filter popup.
        let text = draw(&roles_popup(), 80, 24);
        assert_contains(
            &text,
            &[
                "[x] Performer",
                "[x] Songwriter",
                "[x] Producer",
                "[x] Engineer",
            ],
        );
        insta::assert_snapshot!("ac17_roles_popup", text);

        // A failed page.
        let mut state = state_of(playing());
        let id = ask(&mut state, &[Key::Char('g'), Key::Char('l')]);
        fail(&mut state, id, "Could not reach Tidal: timed out");
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &["Could not load the library: Could not reach Tidal: timed out"],
        );
        assert!(!text.contains("Loading…"), "{text}");
        insta::assert_snapshot!("ac17_failed_page", text);

        // Empty favorites.
        let mut state = state_of(playing());
        let id = ask(&mut state, &[Key::Char('g'), Key::Char('y')]);
        answer(
            &mut state,
            id,
            LibraryResponse::Page(PageData::FavoriteTracks {
                tracks: list(vec![], 0, 0),
            }),
        );
        let text = draw(&state, 80, 24);
        assert_contains(&text, &["No favorite tracks yet"]);
        insta::assert_snapshot!("ac17_empty_favorite_tracks", text);

        // The actions popup (an album), *Add to playlist…*, a `y/n`
        // question, the name prompt.
        let text = draw(&actions_popup(), 80, 24);
        assert_contains(
            &text,
            &[
                "Collide With The Sky",
                "Open",
                "Go to artist: Pierce The Veil",
                "Add to queue",
                "Play next",
                "Add to playlist…",
            ],
        );
        insta::assert_snapshot!("ac17_actions_popup", text);

        let text = draw(&add_to_playlist_popup(), 80, 24);
        assert_contains(&text, &["New playlist…", "Running", "Gym mix"]);
        assert!(
            !text.contains("Late night"),
            "a followed playlist offered:\n{text}"
        );
        insta::assert_snapshot!("ac17_add_to_playlist", text);

        let text = draw(&confirm_popup(), 80, 24);
        assert_contains(&text, &["Delete Running? (y/n)"]);
        insta::assert_snapshot!("ac17_confirm", text);

        let text = draw(&name_popup(), 80, 24);
        assert_contains(&text, &["Playlist name: Road trip"]);
    }

    /// Each empty-list message, one window at a time (50×20).
    #[test]
    fn ac17_empty_messages() {
        let mut state = state_of(playing());
        let id = ask(&mut state, &[Key::Char('g'), Key::Char('l')]);
        answer(&mut state, id, LibraryResponse::Page(empty_library()));
        for message in [
            "No playlists yet",
            "No favorite albums yet",
            "No favorite artists yet",
        ] {
            let text = draw(&state, 50, 20);
            assert_contains(&text, &[message]);
            press(&mut state, &[Key::Tab]);
        }

        let text = draw(&album_page(vec![]), 50, 20);
        assert_contains(&text, &["This album has no tracks"]);
        let text = draw(&playlist_page(vec![]), 50, 20);
        assert_contains(&text, &["This playlist has no tracks"]);

        let mut state = all_tracks(true);
        let text = draw(&state, 50, 20);
        assert_contains(&text, &["No credits (37 hidden)"]);
        // Focus wraps: Top tracks, Albums, Appears on.
        press(&mut state, &[Key::Tab]);
        assert_contains(&draw(&state, 50, 20), &["No top tracks"]);
        press(&mut state, &[Key::Tab]);
        assert_contains(&draw(&state, 50, 20), &["No albums"]);
        press(&mut state, &[Key::Tab]);
        assert_contains(&draw(&state, 50, 20), &["No albums"]);
    }

    /// 50×20: only the focused window, its title followed by `‹Tab›`.
    #[test]
    fn ac17_narrow_50x20() {
        let mut state = library();
        let text = draw(&state, 50, 20);
        assert_contains(&text, &["Playlists (22) ‹Tab›", "Running"]);
        assert!(
            !text.contains("Albums (14)") && !text.contains("Artists (196)"),
            "other windows drawn:\n{text}"
        );
        insta::assert_snapshot!("ac17_narrow_library", text);
        press(&mut state, &[Key::Tab]);
        let text = draw(&state, 50, 20);
        assert_contains(&text, &["Albums (14) ‹Tab›", "Collide With The Sky"]);
        assert!(!text.contains("Playlists (22)"), "{text}");

        let text = draw(&artist_page(true), 50, 20);
        assert_contains(&text, &["Top tracks (91) ‹Tab›", "Song 1"]);
        assert!(!text.contains("Hold On Till May"), "{text}");

        // A page with one window has no `‹Tab›`.
        let text = draw(&favorites(), 50, 20);
        assert!(!text.contains("‹Tab›"), "{text}");
    }

    /// No panic from 0×0 to 120×40 on every page kind and popup.
    #[test]
    fn ac17_no_panic_any_size() {
        let mut login = library();
        login.login_required = true;
        let mut loading = state_of(playing());
        press(&mut loading, &[Key::Char('g'), Key::Char('l')]);
        let mut failed = state_of(playing());
        let id = ask(&mut failed, &[Key::Char('g'), Key::Char('l')]);
        fail(&mut failed, id, "Could not reach Tidal: timed out");
        let mut more = favorites();
        press(&mut more, &[Key::Char('j')]);
        let states = [
            state_of(playing()),
            library(),
            loading,
            failed,
            more,
            album_page(browse_tracks(9)),
            album_page(vec![]),
            playlist_page(browse_tracks(3)),
            artist_page(true),
            artist_page(false),
            all_tracks(false),
            all_tracks(true),
            actions_popup(),
            add_to_playlist_popup(),
            confirm_popup(),
            name_popup(),
            roles_popup(),
            login,
        ];
        let sizes = (0..=120)
            .step_by(7)
            .flat_map(|w| (0..=40).step_by(3).map(move |h| (w, h)))
            .chain([
                (0, 0),
                (1, 1),
                (2, 2),
                (3, 3),
                (50, 20),
                (59, 24),
                (60, 24),
                (62, 24),
                (80, 24),
                (120, 40),
            ]);
        for (width, height) in sizes {
            for state in &states {
                draw(state, width, height);
            }
        }
    }
}
