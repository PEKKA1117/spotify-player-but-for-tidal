//! Rendering of the pure UI model (spec 0004 "TUI"): one bordered frame
//! titled `tidal-player`, the playback window (4 rows) at the top and the
//! queue below it. The two never share rows: the queue list always spans
//! the full width of its own rows.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use tidal_player_core::Track;
use tidal_player_core::protocol::{InsertAt, NowPlaying, PlaybackState, QueueEntry, RepeatMode};
use tidal_player_core::ui::{Key, PageKind, State};

mod pages;

/// Rows of the playback window, inside the frame.
const PLAYBACK_ROWS: u16 = 4;
/// Below this many rows inside the frame only the playback window is drawn.
const MIN_ROWS_WITH_QUEUE: u16 = 6;
/// The narrowest the title side of a row gets before the indicators (or
/// the bit-perfect verdict) make way for it.
const MIN_LEFT: usize = 20;
const STATUS: &str = "Session expired — run \"tidal-player login\" in another terminal";

/// The frame's areas, from the terminal's: the same layout [`render`]
/// draws and [`list_height`] measures.
struct Areas {
    /// The session-expired line (0002 AC17).
    status: Option<Rect>,
    playback: Rect,
    /// The page below the playback window; none when the terminal is too
    /// short for it.
    page: Option<Rect>,
}

fn areas(area: Rect, login_required: bool) -> Areas {
    let mut inner = Block::bordered().inner(area);
    // The status line takes the last row inside the border, when there is
    // one.
    let mut status = None;
    if login_required && inner.height > 0 {
        inner.height -= 1;
        status = Some(Rect {
            y: inner.bottom(),
            height: 1,
            ..inner
        });
    }
    let playback = Rect {
        height: inner.height.min(PLAYBACK_ROWS),
        ..inner
    };
    let page = (inner.height >= MIN_ROWS_WITH_QUEUE).then(|| Rect {
        y: inner.y + PLAYBACK_ROWS,
        height: inner.height - PLAYBACK_ROWS,
        ..inner
    });
    Areas {
        status,
        playback,
        page,
    }
}

/// The rows a list window shows in a terminal of `terminal`'s size: what
/// `Action::Resize` carries (spec 0006 "Lists load as you scroll").
pub fn list_height(terminal: Rect, login_required: bool) -> usize {
    areas(terminal, login_required)
        .page
        .map_or(0, pages::list_height)
}

/// When to draw the key-sequence hint (spec 0013 "When the hint shows"):
/// fed the pending keys and the time after every batch of updates, it says
/// to draw once the same pending keys have been pending for the delay. A
/// further key that keeps a sequence pending restarts it; nothing pending
/// resets it.
#[derive(Debug, Clone)]
pub struct HintTimer {
    delay: Duration,
    pending: Vec<Key>,
    since: Option<Instant>,
}

impl HintTimer {
    pub fn new(delay: Duration) -> Self {
        Self {
            delay,
            pending: Vec::new(),
            since: None,
        }
    }

    /// Whether the hint is drawn at `now` with `pending` keys collected.
    pub fn update(&mut self, pending: &[Key], now: Instant) -> bool {
        if pending.is_empty() {
            self.pending.clear();
            self.since = None;
            return false;
        }
        if self.since.is_none() || self.pending != pending {
            self.pending = pending.to_vec();
            self.since = Some(now);
        }
        self.since
            .is_some_and(|since| now.saturating_duration_since(since) >= self.delay)
    }
}

/// Draws `state` into `frame`; `hint`: the key-sequence hint is due
/// ([`HintTimer`]), drawn when the state has one (spec 0013).
pub fn render(state: &State, frame: &mut Frame, hint: bool) {
    let area = frame.area();
    let areas = areas(area, state.login_required);
    frame.render_widget(Block::bordered().title("tidal-player"), area);
    if let Some(status) = areas.status {
        frame.render_widget(Paragraph::new(STATUS), status);
    }

    render_playback(state, frame, areas.playback);

    let prompt_row = if let Some(page) = areas.page {
        if state.page().kind == PageKind::Queue {
            render_queue(state, frame, page);
        } else {
            pages::render_page(state, frame, page);
        }
        Rect { height: 1, ..page }
    } else {
        // No page: the prompt takes the playback window's last row.
        Rect {
            y: areas.playback.bottom().saturating_sub(1),
            height: areas.playback.height.min(1),
            ..areas.playback
        }
    };
    render_prompt(state, frame, prompt_row);
    pages::render_popup(state, frame, areas.page, prompt_row);
    if hint {
        pages::render_hint(state, frame, areas.page);
    }
    pages::render_help(state, frame, areas.page, area);
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
        let marker = if current { "▶ " } else { "  " };
        self.track_row(index, &entry.track, marker, None)
    }

    /// One track's row; `album` replaces the album column's text (a
    /// credits row shows the artist's roles there).
    fn track_row(&self, index: usize, track: &Track, marker: &str, album: Option<&str>) -> String {
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
            let album = album.or(track.album_title()).unwrap_or_default();
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
    let label = match prompt.at {
        InsertAt::End => "Add to queue: ",
        InsertAt::Next => "Play next: ",
    };
    draw_prompt(frame, area, label, &prompt.text);
}

/// One row of `label` and `text`, the end of the text kept in view, with
/// the cursor after it.
fn draw_prompt(frame: &mut Frame, area: Rect, label: &str, text: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = usize::from(area.width);
    // One column stays free for the cursor.
    let room = width.saturating_sub(text_width(label) + 1);
    let text = if text_width(text) <= room {
        text.to_owned()
    } else {
        format!("…{}", tail(text, room.saturating_sub(1)))
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
        terminal.draw(|frame| render(state, frame, true)).unwrap();
        buffer_text(&terminal)
    }

    fn row(text: &str, n: usize) -> &str {
        text.split('\n').nth(n).unwrap_or("")
    }

    #[test]
    fn ac6_empty_state_80x24() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render(&State::default(), frame, true))
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
        terminal.draw(|frame| render(&state, frame, true)).unwrap();
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
            terminal.draw(|frame| render(&state, frame, true)).unwrap();
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
                cover: None,
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
            device: "default".into(),
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
        let id = ask(&mut state, &[Key::Char(']')]);
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
        terminal.draw(|frame| render(state, frame, true)).unwrap();
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
                "Top tracks (91) │ All tracks  [ ]",
                "Albums (78) │ Appears on  [ ]",
                "Song 1",
                "Hold On Till",
                "2010",
            ],
        );
        insta::assert_snapshot!("ac17_artist_page", text);
        // A wider terminal has room for `EP` and `Single`.
        let wide = draw(&artist_page(true), 120, 24);
        assert_contains(&wide, &["Hold On Till May", "2010 EP", "2008 Single"]);

        // *All tracks* focused: the hidden count, the roles.
        let text = draw(&all_tracks(false), 80, 24);
        assert_contains(
            &text,
            &[
                "All tracks (548 · 37 hidden)  [ ]",
                "Hell Above",
                "Albums (78)",
            ],
        );
        // The other tab's name when the pane has room for it.
        let wide = draw(&all_tracks(false), 120, 24);
        assert_contains(&wide, &["Top tracks │ All tracks (548 · 37 hidden)  [ ]"]);
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
        // Only the library's own row, not a popup row.
        assert_eq!(
            text.matches("Late night").count(),
            1,
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
        // `Tab` cycles the panes, `[`/`]` the tabs of the focused one.
        press(&mut state, &[Key::Tab]);
        assert_contains(&draw(&state, 50, 20), &["No albums"]);
        press(&mut state, &[Key::Tab]);
        assert_contains(&draw(&state, 50, 20), &["No credits (37 hidden)"]);
        press(&mut state, &[Key::Char('[')]);
        assert_contains(&draw(&state, 50, 20), &["No top tracks"]);
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
        assert_contains(
            &text,
            &["Top tracks (91) │ All tracks  [ ] ‹Tab›", "Song 1"],
        );
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
        // 0007 AC12: the search page in each of its states.
        let search = search_states();
        // 0008 AC14: the keys help, open over each kind of view.
        let help = help_states();
        // 0011 AC11: the mixes and radio pages.
        let mixes = mixes_states();
        // 0013 AC6: a pending sequence's hint over each kind of view.
        let hints = hint_states();
        let sizes = sizes.chain(
            (1..=200)
                .step_by(13)
                .flat_map(|w| (1..=60).step_by(4).map(move |h| (w, h))),
        );
        for (width, height) in sizes {
            for state in states
                .iter()
                .chain(&search)
                .chain(&help)
                .chain(&mixes)
                .chain(&hints)
            {
                draw(state, width, height);
            }
        }
    }

    // --- spec 0011 AC11: the mixes and radio pages --------------------------------

    use tidal_player_core::library::{MixSummary, RadioSeed};

    fn mix_summary(id: &str, title: &str, subtitle: Option<&str>) -> MixSummary {
        MixSummary {
            id: id.into(),
            title: title.into(),
            subtitle: subtitle.map(str::to_owned),
        }
    }

    fn mixes_data() -> PageData {
        PageData::Mixes {
            mixes: list(
                vec![
                    mix_summary(
                        "m1",
                        "My Mix 1",
                        Some("Pierce The Veil, Sleeping With Sirens"),
                    ),
                    mix_summary("m2", "My Mix 2", Some("Bring Me The Horizon and more")),
                    mix_summary("m3", "Discovery Mix", None),
                ],
                3,
                0,
            ),
        }
    }

    /// `g m`, answered.
    fn mixes_page() -> State {
        let mut state = state_of(playing());
        let id = ask(&mut state, &[Key::Char('g'), Key::Char('m')]);
        answer(&mut state, id, LibraryResponse::Page(mixes_data()));
        state
    }

    /// The first mix, opened and answered.
    fn mix_page(tracks: Vec<Track>) -> State {
        let mut state = mixes_page();
        let n = tracks.len() as u32;
        let id = ask(&mut state, &[Key::Enter]);
        answer(
            &mut state,
            id,
            LibraryResponse::Page(PageData::Mix {
                mix: mix_summary(
                    "m1",
                    "My Mix 1",
                    Some("Pierce The Veil, Sleeping With Sirens"),
                ),
                tracks: list(tracks, n, 0),
            }),
        );
        state
    }

    /// `r` on the first favorite track, answered.
    fn track_radio_page(tracks: Vec<Track>) -> State {
        let mut state = favorites();
        let n = tracks.len() as u32;
        let seed = browse_tracks(1).remove(0);
        let id = ask(&mut state, &[Key::Char('r')]);
        answer(
            &mut state,
            id,
            LibraryResponse::Page(PageData::Radio {
                seed: RadioSeed::Track(seed),
                tracks: list(tracks, n, 0),
            }),
        );
        state
    }

    /// `r` on the library's first artist, answered.
    fn artist_radio_page(tracks: Vec<Track>) -> State {
        let mut state = library();
        let n = tracks.len() as u32;
        let id = ask(&mut state, &[Key::Tab, Key::Tab, Key::Char('r')]);
        answer(
            &mut state,
            id,
            LibraryResponse::Page(PageData::Radio {
                seed: RadioSeed::Artist(artist_ref(7, "Pierce The Veil")),
                tracks: list(tracks, n, 0),
            }),
        );
        state
    }

    fn mixes_states() -> Vec<State> {
        let mut loading = state_of(playing());
        press(&mut loading, &[Key::Char('g'), Key::Char('m')]);
        let mut failed = state_of(playing());
        let id = ask(&mut failed, &[Key::Char('g'), Key::Char('m')]);
        fail(&mut failed, id, "Could not reach Tidal: timed out");
        let mut empty = state_of(playing());
        let id = ask(&mut empty, &[Key::Char('g'), Key::Char('m')]);
        answer(
            &mut empty,
            id,
            LibraryResponse::Page(PageData::Mixes {
                mixes: list(vec![], 0, 0),
            }),
        );
        vec![
            mixes_page(),
            loading,
            failed,
            empty,
            mix_page(browse_tracks(4)),
            mix_page(vec![]),
            track_radio_page(browse_tracks(3)),
            track_radio_page(vec![]),
            artist_radio_page(browse_tracks(3)),
            artist_radio_page(vec![]),
            track_actions_popup(),
        ]
    }

    /// The actions popup over a favorite track.
    fn track_actions_popup() -> State {
        let mut state = favorites();
        press(&mut state, &[Key::Ctrl(' ')]);
        state
    }

    #[test]
    fn ac11_mixes_80x24() {
        // Mixes loaded: the title with the count, each row its title then
        // its subtitle (none for the last), the first row highlighted.
        let state = mixes_page();
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &[
                "Mixes · 3 mixes",
                "My Mix 1  Pierce The Veil, Sleeping With Sirens",
                "My Mix 2  Bring Me The Horizon and more",
                "Discovery Mix",
            ],
        );
        assert!(
            cell_at(&state, 80, 24, "Pierce The Veil, Sleeping")
                .modifier
                .contains(Mod::REVERSED),
            "the cursor row is not highlighted"
        );
        insta::assert_snapshot!("ac11_mixes", text);

        // A mix: its title and track count, the subtitle as a second,
        // dim title row, then the tracks.
        let state = mix_page(browse_tracks(4));
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &[
                "My Mix 1 · 4 tracks",
                "Pierce The Veil, Sleeping With Sirens",
                "Song 1",
                "Song 4",
            ],
        );
        let title = row_of(&text, "My Mix 1 · 4 tracks");
        assert_eq!(
            row_of(&text, "Pierce The Veil, Sleeping With Sirens"),
            title + 1,
            "the subtitle is not the second title row:\n{text}"
        );
        assert!(
            cell_at(&state, 80, 24, "Pierce The Veil, Sleeping")
                .modifier
                .contains(Mod::DIM),
            "the subtitle is not dim"
        );
        insta::assert_snapshot!("ac11_mix", text);
        // Under 8 rows tall the subtitle row goes.
        let text = draw(&state, 80, 7);
        assert!(
            !text.contains("Sleeping With Sirens"),
            "subtitle on a short page:\n{text}"
        );

        // Track radio.
        let text = draw(&track_radio_page(browse_tracks(3)), 80, 24);
        assert_contains(&text, &["Song 1 Radio · Pierce The Veil", "Song 3"]);
        insta::assert_snapshot!("ac11_track_radio", text);

        // Artist radio.
        let text = draw(&artist_radio_page(browse_tracks(3)), 80, 24);
        assert_contains(&text, &["Pierce The Veil Radio", "Song 2"]);
        insta::assert_snapshot!("ac11_artist_radio", text);

        // Empty and failed.
        let states = mixes_states();
        let text = draw(&states[3], 80, 24);
        assert_contains(&text, &["Mixes", "No mixes yet"]);
        insta::assert_snapshot!("ac11_mixes_empty", text);
        let text = draw(&states[7], 80, 24);
        assert_contains(&text, &["No radio for this track"]);
        insta::assert_snapshot!("ac11_track_radio_empty", text);
        let text = draw(&states[2], 80, 24);
        assert_contains(
            &text,
            &["Could not load the mixes: Could not reach Tidal: timed out"],
        );
        insta::assert_snapshot!("ac11_mixes_failed", text);

        // The actions popup on a track offers the radio.
        let text = draw(&states[10], 80, 24);
        assert_contains(&text, &["Go to radio"]);
        insta::assert_snapshot!("ac11_track_actions_radio", text);
    }

    #[test]
    fn ac11_mixes_narrow_50x20() {
        let text = draw(&mix_page(browse_tracks(4)), 50, 20);
        assert_contains(
            &text,
            &[
                "My Mix 1 · 4 tracks",
                "Pierce The Veil, Sleeping With Sirens",
                "Song 1",
            ],
        );
        insta::assert_snapshot!("ac11_mix_narrow", text);
        let text = draw(&mixes_page(), 50, 20);
        assert_contains(
            &text,
            &["Mixes · 3 mixes", "My Mix 1  Pierce The Veil, Sleeping"],
        );
    }

    // --- spec 0007 AC12: the search page ------------------------------------------

    use tidal_player_core::library::TopHit;

    const QUERY: &str = "pierce the veil";

    fn typed(text: &str) -> Vec<Key> {
        text.chars().map(Key::Char).collect()
    }

    /// `g s` on the playing state: an empty search page, its input focused.
    fn search_empty() -> State {
        let mut state = state_of(playing());
        let effects = press(&mut state, &[Key::Char('g'), Key::Char('s')]);
        assert!(effects.is_empty(), "{effects:?}");
        state
    }

    /// `pierce the veil` typed and sent: the ID of the search.
    fn search_sent(state: &mut State) -> u64 {
        press(state, &typed(QUERY));
        ask(state, &[Key::Enter])
    }

    fn search_tracks() -> Vec<Track> {
        let ptv = "Pierce The Veil";
        vec![
            track(11, "Hell Above", ptv, "Collide With The Sky", Some(212)),
            track(12, "King For A Day", ptv, "Collide With The Sky", Some(232)),
            track(
                13,
                "Bulls In The Bronx",
                ptv,
                "Collide With The Sky",
                Some(290),
            ),
        ]
    }

    fn search_data(top_hit: Option<TopHit>) -> PageData {
        PageData::Search {
            top_hit: top_hit.map(Box::new),
            tracks: list(search_tracks(), 300, 0),
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
                ],
                41,
                0,
            ),
            artists: list(
                vec![
                    artist_ref(7, "Pierce The Veil"),
                    artist_ref(8, "Sleeping With Sirens"),
                ],
                6,
                0,
            ),
            playlists: list(
                vec![playlist("s1", "This Is Pierce The Veil", 50, false)],
                300,
                0,
            ),
        }
    }

    fn search_nothing() -> PageData {
        PageData::Search {
            top_hit: None,
            tracks: list(vec![], 0, 0),
            albums: list(vec![], 0, 0),
            artists: list(vec![], 0, 0),
            playlists: list(vec![], 0, 0),
        }
    }

    /// A search for `pierce the veil` answered with `data`.
    fn searched(data: PageData) -> State {
        let mut state = search_empty();
        let id = search_sent(&mut state);
        answer(&mut state, id, LibraryResponse::Page(data));
        state
    }

    fn search_artist_hit() -> State {
        searched(search_data(Some(TopHit::Artist(artist_ref(
            7,
            "Pierce The Veil",
        )))))
    }

    fn search_loading() -> State {
        let mut state = search_empty();
        search_sent(&mut state);
        state
    }

    fn search_failed() -> State {
        let mut state = search_empty();
        let id = search_sent(&mut state);
        fail(&mut state, id, "Could not reach Tidal: timed out");
        state
    }

    /// Every state of the search page, for the no-panic sweep.
    fn search_states() -> Vec<State> {
        let mut windows = search_artist_hit();
        press(&mut windows, &[Key::Tab, Key::Tab]);
        let mut long = search_empty();
        press(&mut long, &typed(&"米津玄師 ".repeat(40)));
        vec![
            search_empty(),
            search_loading(),
            search_artist_hit(),
            searched(search_data(None)),
            searched(search_nothing()),
            search_failed(),
            windows,
            long,
        ]
    }

    /// The row of `text` that holds `part`.
    fn row_of(text: &str, part: &str) -> usize {
        text.split('\n')
            .position(|r| r.contains(part))
            .unwrap_or_else(|| panic!("{part:?} missing:\n{text}"))
    }

    /// The cell at the start of `part` on its row.
    fn cell_at(state: &State, width: u16, height: u16, part: &str) -> ratatui::buffer::Cell {
        let text = draw(state, width, height);
        let y = row_of(&text, part);
        let line = row(&text, y);
        let x = line[..line.find(part).unwrap()].chars().count();
        let terminal = draw_terminal(state, width, height);
        terminal.backend().buffer()[(x as u16, y as u16)].clone()
    }

    #[test]
    fn ac12_search_80x24() {
        // The empty page: the input under the title, focused, with its
        // cursor; the windows show nothing.
        let state = search_empty();
        let text = draw(&state, 80, 24);
        let title = row_of(&text, "│Search ");
        assert_eq!(row_of(&text, "Search: ▏"), title + 1, "{text}");
        assert!(!text.contains("Search ·"), "{text}");
        assert!(!text.contains("Loading…"), "{text}");
        assert!(!text.contains("Top hit"), "{text}");
        assert!(!text.contains("No tracks found"), "{text}");
        assert!(
            !cell_at(&state, 80, 24, "Search: ")
                .modifier
                .contains(Mod::DIM),
            "focused input dimmed"
        );
        insta::assert_snapshot!("ac12_search_empty", text);

        // Loading: the query in the title, each window `Loading…`.
        let text = draw(&search_loading(), 80, 24);
        assert_contains(
            &text,
            &["Search · \"pierce the veil\"", "Search: pierce the veil▏"],
        );
        assert_eq!(text.matches("Loading…").count(), 4, "{text}");
        insta::assert_snapshot!("ac12_search_loading", text);

        // Results with a top hit: the top-hit row, highlighted, under the
        // input; the four windows with totals in a 2 × 2 grid.
        let state = search_artist_hit();
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &[
                "Search · \"pierce the veil\"",
                "Search: pierce the veil",
                "Top hit: Pierce The Veil · artist",
                "Tracks (300)",
                "Albums (41)",
                "Artists (6)",
                "Playlists (300)",
                "Hell Above",
                // Half the width: the third title is cut.
                "Bulls In Th…",
                "Collide With The…",
                "Misadventures",
                "Sleeping With Sirens",
                "This Is Pierce The Veil",
            ],
        );
        assert!(!text.contains('▏'), "the input has no focus:\n{text}");
        let input = row_of(&text, "Search: ");
        assert_eq!(row_of(&text, "Top hit: "), input + 1, "{text}");
        let top = row_of(&text, "Tracks (300)");
        assert_eq!(row_of(&text, "Albums (41)"), top, "{text}");
        let bottom = row_of(&text, "Artists (6)");
        assert_eq!(row_of(&text, "Playlists (300)"), bottom, "{text}");
        assert!(bottom > row_of(&text, "Bulls In Th…"), "{text}");
        assert!(!text.contains("‹Tab›"), "{text}");
        assert!(
            cell_at(&state, 80, 24, "Pierce The Veil · artist")
                .modifier
                .contains(Mod::REVERSED),
            "the focused top hit is not highlighted"
        );
        assert!(
            cell_at(&state, 80, 24, "Search: ")
                .modifier
                .contains(Mod::DIM),
            "the unfocused input is not dim"
        );
        insta::assert_snapshot!("ac12_search_top_hit", text);

        // A top hit of each other kind.
        for (hit, row) in [
            (
                TopHit::Track(search_tracks()[0].clone()),
                "Top hit: Hell Above · track",
            ),
            (
                TopHit::Album(album(
                    1,
                    "Collide With The Sky",
                    "Pierce The Veil",
                    2012,
                    AlbumKind::Album,
                )),
                "Top hit: Collide With The Sky · album",
            ),
            (
                TopHit::Playlist(playlist("s1", "This Is Pierce The Veil", 50, false)),
                "Top hit: This Is Pierce The Veil · playlist",
            ),
        ] {
            assert_contains(&draw(&searched(search_data(Some(hit))), 80, 24), &[row]);
        }

        // Results without a top hit: no top-hit row, *Tracks* focused.
        let text = draw(&searched(search_data(None)), 80, 24);
        assert_contains(
            &text,
            &["Search: pierce the veil", "Tracks (300)", "Hell Above"],
        );
        assert!(!text.contains("Top hit"), "{text}");
        assert_eq!(
            row_of(&text, "Tracks (300)"),
            row_of(&text, "Search: ") + 1,
            "{text}"
        );
        insta::assert_snapshot!("ac12_search_no_top_hit", text);

        // Nothing found: each window says so; the input keeps the focus.
        let text = draw(&searched(search_nothing()), 80, 24);
        assert_contains(
            &text,
            &[
                "Search: pierce the veil▏",
                "Tracks (0)",
                "No tracks found",
                "No albums found",
                "No artists found",
                "No playlists found",
            ],
        );
        insta::assert_snapshot!("ac12_search_nothing_found", text);

        // A failed search: its message in the page, the input still there.
        let text = draw(&search_failed(), 80, 24);
        assert_contains(
            &text,
            &[
                "Search: pierce the veil▏",
                "Could not search: Could not reach Tidal: timed out",
            ],
        );
        assert!(!text.contains("Loading…"), "{text}");
        insta::assert_snapshot!("ac12_search_failed", text);
    }

    /// 50×20: the input, the top hit and the focused window alone, its
    /// title followed by `‹Tab›`.
    #[test]
    fn ac12_search_narrow_50x20() {
        let mut state = search_artist_hit();
        let text = draw(&state, 50, 20);
        assert_contains(
            &text,
            &[
                "Search: pierce the veil",
                "Top hit: Pierce The Veil · artist",
                "Tracks (300) ‹Tab›",
                "Hell Above",
            ],
        );
        for other in ["Albums (41)", "Artists (6)", "Playlists (300)"] {
            assert!(!text.contains(other), "{other} drawn:\n{text}");
        }
        insta::assert_snapshot!("ac12_search_narrow", text);
        // `Tab`: top hit → Tracks → Albums.
        press(&mut state, &[Key::Tab, Key::Tab]);
        let text = draw(&state, 50, 20);
        assert_contains(&text, &["Albums (41) ‹Tab›", "Collide With The Sky"]);
        assert!(!text.contains("Tracks (300)"), "{text}");
        // The empty page: the input and one empty window.
        let text = draw(&search_empty(), 50, 20);
        assert_contains(&text, &["Search: ▏", "Tracks ‹Tab›"]);
    }
    // --- spec 0008 AC14: the keys help, the library layout ---------------------------

    /// `?` pressed on `state`, then `keys`.
    fn with_help(mut state: State, keys: &[Key]) -> State {
        press(&mut state, &[Key::Char('?')]);
        press(&mut state, keys);
        assert!(state.help.is_some(), "the help did not open");
        state
    }

    /// The library with its *Albums* window focused.
    fn library_albums() -> State {
        let mut state = library();
        press(&mut state, &[Key::Tab]);
        state
    }

    /// The library with `n` moved to `g n`, `q` unbound and an
    /// `[[actions]]` binding.
    fn custom_keymap() -> State {
        use tidal_player_core::ui::keymap::{
            ActionEntry, CommandEntry, KeymapEntry, KeymapFile, Target, build,
        };
        let entry = |key_sequence: &str, name: &str| KeymapEntry {
            command: CommandEntry::name(name),
            key_sequence: key_sequence.into(),
        };
        let file = KeymapFile {
            keymaps: vec![
                entry("n", "None"),
                entry("g n", "NextTrack"),
                entry("q", "None"),
            ],
            actions: vec![ActionEntry {
                action: "GoToAlbum".into(),
                key_sequence: "g B".into(),
                target: Target::PlayingTrack,
            }],
        };
        let mut state = library();
        tidal_player_core::ui::apply_keymap(&mut state, build(&file).unwrap());
        state
    }

    fn typed_keys(text: &str) -> Vec<Key> {
        text.chars().map(Key::Char).collect()
    }

    fn filtered(filter: &str) -> State {
        let mut keys = vec![Key::Char('/')];
        keys.extend(typed_keys(filter));
        with_help(library_albums(), &keys)
    }

    fn help_states() -> Vec<State> {
        vec![
            with_help(state_of(playing()), &[]),
            with_help(library_albums(), &[]),
            filtered("fav"),
            filtered("xyz"),
            with_help(actions_popup(), &[]),
            with_help(custom_keymap(), &[]),
            with_help(state_of(playing()), &[Key::Char('j'); 30]),
        ]
    }

    /// The line of `text` that contains `part`.
    fn line_with<'a>(text: &'a str, part: &str) -> &'a str {
        text.split('\n')
            .find(|l| l.contains(part))
            .unwrap_or_else(|| panic!("{part:?} missing:\n{text}"))
    }

    #[test]
    fn ac14_help_80x24() {
        // On the queue page: the window's section first, then the rest.
        let state = with_help(state_of(playing()), &[]);
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &[
                "┌Keys",
                "Queue",
                "play the entry",
                "Lists",
                "j  down  C-n",
                "move down",
                "/ filter · enter run · esc close",
            ],
        );
        let queue = row_of(&text, " Queue");
        let lists = row_of(&text, " Lists");
        assert!(queue < lists, "sections out of order:\n{text}");
        // The highlighted row: the first binding row.
        let y = row_of(&text, "play the entry");
        let x = line_with(&text, "play the entry")
            .split("enter")
            .next()
            .unwrap()
            .chars()
            .count();
        let terminal = draw_terminal(&state, 80, 24);
        let cell = &terminal.backend().buffer()[(x as u16, y as u16)];
        assert!(cell.modifier.contains(Mod::REVERSED), "{cell:?}\n{text}");
        // 80 % of the page area (78 × 18): 62 × 14, centred.
        let top = row_of(&text, "┌Keys");
        let left = line_with(&text, "┌Keys").chars().position(|c| c == '┌');
        assert_eq!((left, top), (Some(9), 7), "{text}");
        assert_eq!(
            line_with(&text, "┌Keys")
                .chars()
                .filter(|c| *c == '─')
                .count()
                + 2
                + 4,
            62,
            "{text}"
        );
        insta::assert_snapshot!("ac14_help_queue", text);

        // The library, *Albums* focused.
        let text = draw(&with_help(library_albums(), &[]), 80, 24);
        assert_contains(
            &text,
            &["┌Keys", "Library · Albums", "open the album", "Z  C-z"],
        );
        insta::assert_snapshot!("ac14_help_library_albums", text);

        // Filtered by `fav`: only the matching rows, the filter shown.
        let text = draw(&filtered("fav"), 80, 24);
        assert_contains(&text, &["┌Keys", "/fav", "favorite tracks", "g y"]);
        assert!(!text.contains("play / pause"), "{text}");
        insta::assert_snapshot!("ac14_help_filtered", text);

        // Nothing matches.
        let text = draw(&filtered("xyz"), 80, 24);
        assert_contains(&text, &["┌Keys", "No keys match \"xyz\""]);
        insta::assert_snapshot!("ac14_help_no_match", text);

        // Opened from the actions popup: the popup's section first.
        let state = with_help(actions_popup(), &[]);
        assert!(state.popup.is_some());
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &[
                "┌Keys",
                "Popup · Actions",
                "run the selected entry",
                "Lists",
                " App",
            ],
        );
        assert!(!text.contains("play / pause"), "{text}");
        insta::assert_snapshot!("ac14_help_actions_popup", text);

        // A custom keymap: the moved key, the removed one gone, `Actions`.
        let text = draw(&with_help(custom_keymap(), &[]), 80, 24);
        // The window, Lists and Pages sections come first: scroll to the
        // end to see the rest.
        let state = with_help(custom_keymap(), &[Key::Char('G')]);
        let end = draw(&state, 80, 24);
        assert_contains(&end, &["Actions", "g B", "go to the album (playing track)"]);
        assert_contains(&end, &["C-c", "quit"]);
        assert!(line_with(&end, "quit").contains("C-c"), "{end}");
        assert!(!line_with(&end, "quit").contains("q  C-c"), "{end}");
        insta::assert_snapshot!("ac14_help_custom_keymap", text);
        insta::assert_snapshot!("ac14_help_custom_keymap_end", end);
        let mut keys = vec![Key::Char('/')];
        keys.extend(typed_keys("next"));
        let middle = draw(&with_help(custom_keymap(), &keys), 80, 24);
        assert!(line_with(&middle, "next track").contains("g n"), "{middle}");
        assert!(
            !line_with(&middle, "next track").contains("n  g n"),
            "{middle}"
        );
    }

    /// 40×12: the help takes the whole terminal (at least 40 × 10); the key
    /// column stays whole and the help text is cut with `…`.
    #[test]
    fn ac14_help_40x12() {
        let text = draw(&with_help(library(), &[]), 40, 12);
        assert_contains(
            &text,
            &["┌Keys", "Library · Playlists", "open the playlist"],
        );
        let row = line_with(&text, "Z  C-z");
        assert!(row.contains("add to the end of"), "{text}");
        assert!(row.contains('…'), "not cut: {row:?}\n{text}");
        assert!(!text.contains("add to the end of the queue"), "{text}");
        insta::assert_snapshot!("ac14_help_40x12", text);
    }

    /// The library at `playlist_percent = 30, album_percent = 50`: 23, 39
    /// and 16 of the 78 columns.
    #[test]
    fn ac14_library_percentages() {
        let mut state = library();
        state.library_layout = tidal_player_core::ui::LibraryLayout {
            playlist_percent: 30,
            album_percent: 50,
        };
        let text = draw(&state, 80, 24);
        assert_contains(&text, &["Playlists (22)", "Albums (14)", "Artists (196)"]);
        let titles = line_with(&text, "Playlists (22)");
        let column = |part: &str| titles[..titles.find(part).unwrap()].chars().count();
        assert_eq!(
            (column("Playlists"), column("Albums"), column("Artists")),
            (2, 25, 64),
            "{text}"
        );
        insta::assert_snapshot!("ac14_library_30_50", text);
        // The defaults keep 0006's 40 / 40 / 20.
        let text = draw(&library(), 80, 24);
        let titles = line_with(&text, "Playlists (22)");
        let column = |part: &str| titles[..titles.find(part).unwrap()].chars().count();
        assert_eq!(
            (column("Playlists"), column("Albums"), column("Artists")),
            (2, 33, 64),
            "{text}"
        );
    }

    // --- spec 0009 AC14 ----------------------------------------------------------

    /// The player's state at start, as the standalone TUI and the daemon
    /// make it from what `playback.json` gave.
    fn started_from(loaded: crate::persist::Loaded) -> State {
        let settings = crate::play::PlayerSettings::default();
        let player =
            crate::player_runtime::starting_state(settings.player.clone(), 7, &settings, loaded);
        state_of(player.snapshot())
    }

    /// AC14: a restored state: stopped (`■`) on "Hell Above" at 1:23 of
    /// 4:56 with its bar, the queue with the current entry marked.
    #[test]
    fn ac14_restored_80x24() {
        use tidal_player_core::PlayerState;
        use tidal_player_core::player::{PlayerConfig, PlayerInput, update as player_update};
        use tidal_player_core::protocol::Command;

        let ptv = "Pierce The Veil";
        let album = "Collide With The Sky";
        let mut st = PlayerState::new(PlayerConfig::default(), 3);
        for command in [
            Command::LoadQueue {
                tracks: vec![
                    track(1001, "May These Noises Startle You", ptv, album, Some(241)),
                    track(1002, "Hell Above", ptv, album, Some(296)),
                    track(1003, "A Match Into Water", ptv, album, Some(262)),
                ],
                start: 1,
            },
            Command::CycleRepeat,
            Command::SetVolume(70),
            Command::SeekTo(Duration::from_secs(83)),
        ] {
            player_update(&mut st, PlayerInput::Command(command));
        }
        let state = started_from(crate::persist::Loaded {
            saved: Some(st.saved()),
            message: None,
        });
        let text = draw(&state, 80, 24);
        assert_contains(
            row(&text, 1),
            &["■ Hell Above · Pierce The Veil", "repeat: queue", "70%"],
        );
        assert_contains(row(&text, 4), &["━", "─", "1:23 / 4:56"]);
        assert_contains(&text, &["Queue (3)", "▶ 2", "A Match Into Water"]);
        insta::assert_snapshot!(text);
    }

    /// AC14: a corrupt `playback.json`: an empty start, the message on the
    /// message row.
    #[test]
    fn ac14_restore_message_80x24() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("playback.json"), "{\"version\": 1").unwrap();
        let loaded = crate::persist::load(&crate::persist::RealFs, dir.path());
        let text = draw(&started_from(loaded), 80, 24);
        assert_contains(row(&text, 1), &["Nothing playing"]);
        assert_contains(
            row(&text, 3),
            &["Could not restore the playback state (kept as playback.json.bad)"],
        );
        insta::assert_snapshot!(text);
    }

    // --- spec 0013: key-sequence hints -------------------------------------------------

    use tidal_player_core::ui::{HintEntry, Hints};

    /// AC5: the timer draws once the same pending keys have been pending
    /// for the delay; a further key restarts it, nothing pending resets it.
    #[test]
    fn ac5_hint_timer() {
        let g = Key::Char('g');
        let s = Key::Char('s');
        let l = Key::Char('l');
        type Step = (Vec<Key>, u64, bool);
        let rows: Vec<(&str, u64, Vec<Step>)> = vec![
            (
                "delay",
                1000,
                vec![
                    (vec![g], 0, false),
                    (vec![g], 999, false),
                    (vec![g], 1000, true),
                    (vec![g], 5000, true),
                ],
            ),
            ("delay 0", 0, vec![(vec![g], 0, true), (vec![g], 10, true)]),
            (
                "nothing pending",
                0,
                vec![(vec![], 0, false), (vec![], 5000, false)],
            ),
            (
                "a further key restarts",
                1000,
                vec![
                    (vec![s], 0, false),
                    (vec![s], 800, false),
                    (vec![s, l], 900, false),
                    (vec![s, l], 1899, false),
                    (vec![s, l], 1900, true),
                ],
            ),
            (
                "completed, cancelled or mismatched resets",
                1000,
                vec![
                    (vec![g], 0, false),
                    (vec![g], 1000, true),
                    (vec![], 1100, false),
                    (vec![g], 1200, false),
                    (vec![g], 2199, false),
                    (vec![g], 2200, true),
                ],
            ),
        ];
        let start = Instant::now();
        for (name, delay, steps) in rows {
            let mut timer = HintTimer::new(Duration::from_millis(delay));
            for (i, (pending, at, want)) in steps.into_iter().enumerate() {
                let now = start + Duration::from_millis(at);
                assert_eq!(
                    timer.update(&pending, now),
                    want,
                    "{name}: step {i} ({pending:?} at {at} ms)"
                );
            }
        }
    }

    fn hint(key: &str, text: &str) -> HintEntry {
        HintEntry {
            key: key.into(),
            text: text.into(),
            dim: false,
            more: 0,
        }
    }

    /// The six default `g` entries, in the library's order.
    fn g_entries() -> Vec<HintEntry> {
        vec![
            hint("a", "actions on the selected row"),
            hint("g", "move to the top"),
            hint("l", "the library"),
            hint("y", "favorite tracks"),
            hint("s", "the search page (on one: its input)"),
            hint("m", "your mixes"),
        ]
    }

    /// AC6: the box's place, columns, rows, title and cut cells for a page
    /// area (`… +N more` when the entries do not fit); no box under 3 × 12.
    #[test]
    fn ac6_key_hints_layout() {
        let g = |entries: Vec<HintEntry>| Hints {
            prefix: "g".into(),
            entries,
        };
        let cell = |text: &str| (text.to_owned(), false);
        let page = |width: u16, height: u16| Rect {
            x: 1,
            y: 5,
            width,
            height,
        };
        type Want = Option<(Rect, usize, Vec<Vec<(String, bool)>>)>;
        let rows: Vec<(&str, Hints, Rect, Want)> = vec![
            (
                "80 × 24: two columns of 32",
                g(g_entries()),
                page(78, 18),
                Some((
                    Rect::new(1, 18, 78, 5),
                    32,
                    vec![
                        vec![
                            cell("a  actions on the selected row"),
                            cell("y  favorite tracks"),
                        ],
                        vec![
                            cell("g  move to the top"),
                            cell("s  the search page (on one: its…"),
                        ],
                        vec![cell("l  the library"), cell("m  your mixes")],
                    ],
                )),
            ),
            (
                "120 × 30: three columns",
                g(g_entries()),
                page(118, 24),
                Some((
                    Rect::new(1, 25, 118, 4),
                    32,
                    vec![
                        vec![
                            cell("a  actions on the selected row"),
                            cell("l  the library"),
                            cell("s  the search page (on one: its…"),
                        ],
                        vec![
                            cell("g  move to the top"),
                            cell("y  favorite tracks"),
                            cell("m  your mixes"),
                        ],
                    ],
                )),
            ),
            (
                "40 × 12: one column, … +4 more",
                g(g_entries()),
                page(38, 6),
                Some((
                    Rect::new(1, 6, 38, 5),
                    32,
                    vec![
                        vec![cell("a  actions on the selected row")],
                        vec![cell("g  move to the top")],
                        vec![cell("… +4 more")],
                    ],
                )),
            ),
            (
                "one row: … +5 more",
                g(g_entries()),
                page(78, 4),
                Some((
                    Rect::new(1, 6, 78, 3),
                    32,
                    vec![vec![
                        cell("a  actions on the selected row"),
                        cell("… +5 more"),
                    ]],
                )),
            ),
            (
                "12 columns: cut to the inner width",
                g(g_entries()[..1].to_vec()),
                page(12, 4),
                Some((Rect::new(1, 6, 12, 3), 10, vec![vec![cell("a  action…")]])),
            ),
            (
                "short entries: as wide as the widest",
                g(vec![hint("a", "x"), hint("b", "y"), hint("C-c", "z")]),
                page(78, 18),
                Some((
                    Rect::new(1, 20, 78, 3),
                    6,
                    vec![vec![cell("a  x"), cell("b  y"), cell("C-c  z")]],
                )),
            ),
            (
                "nested prefix and dim",
                Hints {
                    prefix: "s l".into(),
                    entries: vec![
                        hint("q", "the queue page"),
                        HintEntry {
                            key: "l".into(),
                            text: String::new(),
                            dim: true,
                            more: 2,
                        },
                    ],
                },
                page(78, 18),
                Some((
                    Rect::new(1, 20, 78, 3),
                    17,
                    vec![vec![cell("q  the queue page"), ("l  +2".into(), true)]],
                )),
            ),
            ("11 columns: none", g(g_entries()), page(11, 18), None),
            ("2 rows: none", g(g_entries()), page(78, 2), None),
            (
                "3 rows: no room for a row",
                g(g_entries()),
                page(78, 3),
                None,
            ),
        ];
        for (name, hints, area, want) in rows {
            let got = pages::hint_layout(&hints, area);
            match want {
                None => assert_eq!(got, None, "{name}"),
                Some((rect, width, cells)) => {
                    let got = got.unwrap_or_else(|| panic!("{name}: no box"));
                    assert_eq!(got.rect, rect, "{name}: rect");
                    assert_eq!(got.width, width, "{name}: column width");
                    assert_eq!(got.rows, cells, "{name}: cells");
                    let title = format!("{} …", hints.prefix);
                    assert_eq!(got.title, title, "{name}: title");
                }
            }
        }
    }

    /// `state` with `keys` pressed (a sequence left pending).
    fn pending(mut state: State, keys: &[Key]) -> State {
        press(&mut state, keys);
        assert!(!state.pending.is_empty(), "nothing pending");
        state
    }

    /// The library with `s q`, `s l a` and `s l b` bound.
    fn nested_keymap() -> State {
        use tidal_player_core::ui::keymap::{CommandEntry, KeymapEntry, KeymapFile, build};
        let entry = |key_sequence: &str, name: &str| KeymapEntry {
            command: CommandEntry::name(name),
            key_sequence: key_sequence.into(),
        };
        let file = KeymapFile {
            keymaps: vec![
                entry("s q", "Queue"),
                entry("s l a", "LikedTrackPage"),
                entry("s l b", "NextTrack"),
            ],
            actions: vec![],
        };
        let mut state = library();
        tidal_player_core::ui::apply_keymap(&mut state, build(&file).unwrap());
        state
    }

    /// The library loaded with nothing in it: no row is selected.
    fn library_empty() -> State {
        let mut state = state_of(playing());
        let id = ask(&mut state, &[Key::Char('g'), Key::Char('l')]);
        answer(&mut state, id, LibraryResponse::Page(empty_library()));
        state
    }

    fn hint_states() -> Vec<State> {
        let g = [Key::Char('g')];
        vec![
            pending(library(), &g),
            pending(state_of(playing()), &g),
            pending(actions_popup(), &g),
            pending(nested_keymap(), &[Key::Char('s')]),
            pending(library_empty(), &g),
        ]
    }

    /// AC6: the hint docked at the bottom of the page area, titled `g …`,
    /// its entries in columns, dim entries dim; snapshots per view and size.
    #[test]
    fn ac6_key_hints() {
        let g = [Key::Char('g')];

        // The library, a row selected: two columns at 80 × 24.
        let state = pending(library(), &g);
        let text = draw(&state, 80, 24);
        assert_contains(
            &text,
            &[
                "┌g …",
                "a  actions on the selected row     y  favorite tracks",
                "g  move to the top                 s  the search page (on one: its…",
                "l  the library                     m  your mixes",
            ],
        );
        // Docked: the box's bottom border is the page area's last row.
        assert_eq!(row_of(&text, "┌g …"), 18, "{text}");
        assert!(row(&text, 22).starts_with("│└"), "{text}");
        // Not drawn when the timer says no.
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(&state, frame, false)).unwrap();
        assert!(!buffer_text(&terminal).contains("┌g …"));
        insta::assert_snapshot!("ac6_key_hints_library", text);

        // The queue page: the entry's text.
        let text = draw(&pending(state_of(playing()), &g), 80, 24);
        assert_contains(&text, &["┌g …", "a  actions on the entry"]);
        insta::assert_snapshot!("ac6_key_hints_queue", text);

        // Over the actions popup: only `g`.
        let text = draw(&pending(actions_popup(), &g), 80, 24);
        assert_contains(&text, &["┌g …", "g  move to the top"]);
        assert!(!text.contains("the library"), "{text}");
        insta::assert_snapshot!("ac6_key_hints_actions_popup", text);

        // 40 × 12: one column, cut, `… +N more`.
        let text = draw(&pending(library(), &g), 40, 12);
        assert_contains(
            &text,
            &["┌g …", "a  actions on the selected row", "… +4 more"],
        );
        insta::assert_snapshot!("ac6_key_hints_40x12", text);

        // 120 × 30: three columns.
        let text = draw(&pending(library(), &g), 120, 30);
        assert_contains(
            &text,
            &["┌g …", "a  actions on the selected row     l  the library"],
        );
        insta::assert_snapshot!("ac6_key_hints_120x30", text);

        // A nested prefix.
        let text = draw(&pending(nested_keymap(), &[Key::Char('s')]), 80, 24);
        assert_contains(&text, &["┌s …", "q  the queue page", "l  +2"]);
        insta::assert_snapshot!("ac6_key_hints_nested", text);

        // Dim: no row selected, so `a` does nothing; `l` does.
        let state = pending(library_empty(), &g);
        let text = draw(&state, 80, 24);
        let terminal = draw_terminal(&state, 80, 24);
        let at = |part: &str| {
            let y = row_of(&text, part);
            let x = line_with(&text, part)
                .split(part)
                .next()
                .unwrap()
                .chars()
                .count();
            terminal.backend().buffer()[(x as u16, y as u16)].clone()
        };
        assert!(
            at("actions on the selected row")
                .modifier
                .contains(Mod::DIM)
        );
        assert!(!at("the library").modifier.contains(Mod::DIM));

        // No page area (under 8 rows): no hint.
        let text = draw(&pending(library(), &g), 80, 7);
        assert!(!text.contains("g …"), "{text}");
    }
}
