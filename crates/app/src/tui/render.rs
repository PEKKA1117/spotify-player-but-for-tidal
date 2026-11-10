//! Rendering of the pure UI model (spec 0004 "TUI"): one bordered frame
//! titled `tidal-player`, the playback window (4 rows, taller with the
//! album cover of spec 0016) at the top and the queue below it. The two
//! never share rows: the queue list always spans the full width of its own
//! rows. Everything is drawn in the active theme's styles (spec 0017), with
//! the focus, playing and dim cues on top.

use std::time::{Duration, Instant};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Rect, Size},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use tidal_player_core::Track;
use tidal_player_core::protocol::{InsertAt, NowPlaying, PlaybackState, QueueEntry, RepeatMode};
use tidal_player_core::ui::filter::{self, no_match};
use tidal_player_core::ui::select;
use tidal_player_core::ui::theme::Component;
use tidal_player_core::ui::{Key, PageKind, State, Theme};

use theme::of;

mod pages;
pub mod theme;

/// Rows of the playback window, inside the frame.
const PLAYBACK_ROWS: u16 = 4;
/// Below this many rows inside the frame only the playback window is drawn.
const MIN_ROWS_WITH_QUEUE: u16 = 6;
/// The fewest rows the page keeps below the playback window.
const MIN_PAGE_ROWS: u16 = MIN_ROWS_WITH_QUEUE - PLAYBACK_ROWS;
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
    /// The album cover's area while it is active (spec 0016).
    cover: Option<Rect>,
    /// The playback window's lines: right of the cover, or all of it.
    text: Rect,
    /// The page below the playback window; none when the terminal is too
    /// short for it.
    page: Option<Rect>,
}

fn areas(area: Rect, login_required: bool, cover: Option<CoverGeometry>) -> Areas {
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
    let layout = cover_layout(inner, cover);
    Areas {
        status,
        playback: layout.playback,
        cover: layout.cover,
        text: layout.text,
        page: layout.page,
    }
}

/// The rows a list window shows in a terminal of `terminal`'s size: what
/// `Action::Resize` carries (spec 0006 "Lists load as you scroll"); with
/// `cover`, the taller playback window while it is active (spec 0016).
pub fn list_height(terminal: Rect, login_required: bool, cover: Option<CoverGeometry>) -> usize {
    areas(terminal, login_required, cover)
        .page
        .map_or(0, pages::list_height)
}

// --- the album cover (spec 0016) ---------------------------------------------------

/// What sizes the cover: `cover_img_rows` and the terminal's cell size in
/// pixels (width, height), when known. Given only while the cover can be
/// active (`cover_img` on and a protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverGeometry {
    pub rows: u16,
    pub cell: Option<(u16, u16)>,
}

/// An image ready to draw, encoded for its area: the seam between the
/// frame and `ratatui-image`.
pub trait CoverImage {
    /// Draws into `area` of `buf`.
    fn draw(&self, area: Rect, buf: &mut Buffer);
}

/// The cover for one frame: its geometry, and the ready image with the
/// area size it was encoded for.
#[derive(Clone, Copy)]
pub struct CoverFrame<'a> {
    pub geometry: CoverGeometry,
    pub image: Option<(Size, &'a dyn CoverImage)>,
}

/// The playback window's parts and the page, inside the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverLayout {
    pub playback: Rect,
    /// The cover area; none when the cover is not active.
    pub cover: Option<Rect>,
    /// Where the playback window's lines go.
    pub text: Rect,
    pub page: Option<Rect>,
}

/// The layout of `inner` (the frame's inside, the status line taken out)
/// with `cover` (`None`: not active), spec 0016 "Layout".
pub fn cover_layout(inner: Rect, cover: Option<CoverGeometry>) -> CoverLayout {
    if let Some(geometry) = cover {
        let rows = geometry.rows;
        let cols = cover_columns(geometry);
        if inner.height >= rows.saturating_add(MIN_PAGE_ROWS)
            && usize::from(inner.width) >= usize::from(cols) + 1 + MIN_LEFT
        {
            let playback = Rect {
                height: rows,
                ..inner
            };
            return CoverLayout {
                playback,
                cover: Some(Rect {
                    width: cols,
                    ..playback
                }),
                text: Rect {
                    x: inner.x + cols + 1,
                    width: inner.width - cols - 1,
                    ..playback
                },
                page: Some(Rect {
                    y: inner.y + rows,
                    height: inner.height - rows,
                    ..inner
                }),
            };
        }
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
    CoverLayout {
        playback,
        cover: None,
        text: playback,
        page,
    }
}

/// The cover area in a terminal of `terminal`'s size; none when the cover
/// is not active there.
pub fn cover_area(
    terminal: Rect,
    login_required: bool,
    cover: Option<CoverGeometry>,
) -> Option<Rect> {
    areas(terminal, login_required, cover).cover
}

/// The cover's width: square in pixels, `round(rows x cell height / cell
/// width)`; 2 columns per row with no cell size known.
fn cover_columns(geometry: CoverGeometry) -> u16 {
    let rows = u32::from(geometry.rows);
    let cols = match geometry.cell {
        Some((w, h)) if w > 0 && h > 0 => {
            let (w, h) = (u32::from(w), u32::from(h));
            (2 * rows * h + w) / (2 * w)
        }
        _ => 2 * rows,
    };
    u16::try_from(cols.max(1)).unwrap_or(u16::MAX)
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
    render_with_cover(state, frame, hint, None);
}

/// [`render`] with the album cover (spec 0016); `None`: not active.
pub fn render_with_cover(state: &State, frame: &mut Frame, hint: bool, cover: Option<&CoverFrame>) {
    let area = frame.area();
    let areas = areas(area, state.login_required, cover.map(|c| c.geometry));
    let look = state.theme();
    // Every cell starts in the screen's style (spec 0017 "Components").
    frame
        .buffer_mut()
        .set_style(area, theme::style(look.screen));
    frame.render_widget(
        Block::bordered()
            .border_style(of(look, Component::Border))
            .title(Line::styled(
                "tidal-player",
                of(look, Component::BlockTitle),
            )),
        area,
    );
    if let Some(status) = areas.status {
        frame.render_widget(Paragraph::new(STATUS), status);
    }

    render_playback(state, frame, areas.text);

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

    // Last, and only where nothing else was drawn over it: popups and the
    // hint stay in the page; the keys help may not.
    if let (Some(rect), Some((size, image))) = (areas.cover, cover.and_then(|c| c.image))
        && size == rect.as_size()
        && pages::help_rect(state, areas.page, area).is_none_or(|help| !help.intersects(rect))
    {
        image.draw(rect, frame.buffer_mut());
    }
}

// --- the playback window -----------------------------------------------------------

fn render_playback(state: &State, frame: &mut Frame, area: Rect) {
    let width = usize::from(area.width);
    let entry = state.current();
    let look = state.theme();
    let mut lines = vec![header(state, entry, width)];
    if let Some(entry) = entry {
        let album = entry.track.album_title().unwrap_or_default();
        lines.push(Line::styled(
            fit(&format!("  {album}"), width),
            of(look, Component::PlaybackAlbum),
        ));
        lines.push(match state.message() {
            Some(message) => Line::styled(
                fit(&format!("  {message}"), width),
                of(look, Component::Error),
            ),
            None => Line::styled(
                state
                    .player
                    .as_ref()
                    .and_then(|p| p.now_playing.as_ref())
                    .map(|np| details(np, width))
                    .unwrap_or_default(),
                of(look, Component::PlaybackMetadata),
            ),
        });
        let bar = progress(state.position, entry.track.duration, width, look);
        if area.height > PLAYBACK_ROWS {
            // The taller window (spec 0016): the bar on its last row.
            let last = Rect {
                y: area.bottom() - 1,
                height: 1,
                ..area
            };
            frame.render_widget(Paragraph::new(bar), last);
        } else {
            lines.push(bar);
        }
    } else if let Some(message) = state.message() {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            fit(&format!("  {message}"), width),
            of(look, Component::Error),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// `▶ Title · Artists` on the left, the indicators flush right; the
/// indicators shrink to the volume, then go, before the title gets
/// narrower than [`MIN_LEFT`]. The symbol and indicators are in
/// `playback_status`, the title in `playback_track`, ` · ` and the artists
/// in `playback_artists`; the gap between is the screen's.
fn header(state: &State, entry: Option<&QueueEntry>, width: usize) -> Line<'static> {
    let look = state.theme();
    let status = of(look, Component::PlaybackStatus);
    let parts: Vec<(String, Style)> = match (entry, state.player.as_ref()) {
        (Some(entry), Some(player)) => {
            let symbol = match player.state {
                PlaybackState::Playing => "▶",
                PlaybackState::Paused => "⏸",
                PlaybackState::Loading | PlaybackState::Buffering => "…",
                PlaybackState::Stopped => "■",
            };
            let mut parts = vec![
                (format!("{symbol} "), status),
                (
                    entry.track.title.clone(),
                    of(look, Component::PlaybackTrack),
                ),
            ];
            let artists = entry.track.artist_names();
            if !artists.is_empty() {
                parts.push((
                    format!(" · {artists}"),
                    of(look, Component::PlaybackArtists),
                ));
            }
            parts
        }
        _ => vec![(
            "Nothing playing".to_owned(),
            of(look, Component::PlaybackTrack),
        )],
    };
    let left: String = parts.iter().map(|(text, _)| text.as_str()).collect();
    let Some(player) = state.player.as_ref() else {
        return Line::from(fit_spans(&parts, &fit(&left, width)));
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
    let row = [all.join("  "), volume]
        .into_iter()
        .find_map(|right| split_row(&left, &right, width).map(|(left, gap)| (left, gap, right)));
    let Some((shown, gap, right)) = row else {
        return Line::from(fit_spans(&parts, &fit(&left, width)));
    };
    let mut spans = fit_spans(&parts, &shown);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(right, status));
    Line::from(spans)
}

/// `left` and `right` on one row of `width`, `right` flush right: `left`
/// (cut to fit) and the gap before `right`; `None` when `right` would leave
/// `left` fewer than [`MIN_LEFT`] columns it needs.
fn split_row(left: &str, right: &str, width: usize) -> Option<(String, usize)> {
    let (lw, rw) = (text_width(left), text_width(right));
    if lw + 1 + rw <= width {
        return Some((left.to_owned(), width - lw - rw));
    }
    if width >= rw + 1 + MIN_LEFT {
        let left = fit(left, width - rw - 1);
        let pad = width - text_width(&left) - rw;
        return Some((left, pad));
    }
    None
}

/// `parts` (text and style) shown as `shown`: their concatenation, or what
/// [`fit`] cut it to, the `…` in the style of the last part kept.
fn fit_spans(parts: &[(String, Style)], shown: &str) -> Vec<Span<'static>> {
    let whole: String = parts.iter().map(|(text, _)| text.as_str()).collect();
    if shown == whole {
        return parts
            .iter()
            .map(|(text, style)| Span::styled(text.clone(), *style))
            .collect();
    }
    // A cut row: a prefix of `whole`, then `…` (or nothing at width 0).
    let Some(kept) = shown.strip_suffix('…') else {
        return Vec::new();
    };
    let mut left = kept.chars().count();
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (text, style) in parts {
        if left == 0 {
            break;
        }
        let part: String = text.chars().take(left).collect();
        left -= part.chars().count();
        spans.push(Span::styled(part, *style));
    }
    match spans.last_mut() {
        Some(last) => last.content.to_mut().push('…'),
        None => spans.push(Span::styled(
            "…",
            parts.first().map_or_else(Style::new, |(_, style)| *style),
        )),
    }
    spans
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
/// (`1:23 / ?:??`, tidalt bug 6). The played part in
/// `playback_progress_bar`, the rest in `playback_progress_bar_unfilled`,
/// the time in `playback_metadata`.
fn progress(
    position: Duration,
    duration: Option<Duration>,
    width: usize,
    look: &Theme,
) -> Line<'static> {
    let time = format!(
        "{} / {}",
        clock(position),
        duration.map_or_else(|| "?:??".to_owned(), clock)
    );
    let tw = text_width(&time);
    let metadata = of(look, Component::PlaybackMetadata);
    match duration {
        Some(duration) if width > 2 + 2 + tw => {
            let bar = width - 2 - 2 - tw;
            let done = if duration.is_zero() {
                0
            } else {
                let ratio = position.as_secs_f64() / duration.as_secs_f64();
                ((bar as f64 * ratio).round() as usize).min(bar)
            };
            Line::from(vec![
                Span::raw("  "),
                Span::styled("━".repeat(done), of(look, Component::PlaybackProgressBar)),
                Span::styled(
                    "─".repeat(bar - done),
                    of(look, Component::PlaybackProgressBarUnfilled),
                ),
                Span::raw("  "),
                Span::styled(time, metadata),
            ])
        }
        _ if width >= tw => Line::from(vec![
            Span::raw(" ".repeat(width - tw)),
            Span::styled(time, metadata),
        ]),
        _ => Line::styled(fit(&time, width), metadata),
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
    /// The entry's queue position, its place among the entries shown
    /// (`secondary_row` on every other one) and the entry.
    Entry(usize, usize, &'a QueueEntry),
}

fn render_queue(state: &State, frame: &mut Frame, area: Rect) {
    let queue = state.queue();
    let filter = filter::queue_filter(state);
    let width = usize::from(area.width.saturating_sub(2));
    let look = state.theme();
    // Bold while its filter is typed (spec 0012 "Drawing").
    let mut title_style = of(look, Component::BlockTitle);
    if filter.typing {
        title_style = title_style.add_modifier(Modifier::BOLD);
    }
    let block = Block::bordered()
        .border_style(of(look, Component::Border))
        .title(Line::styled(fit(&queue_title(state), width), title_style));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The entries shown (all, or the filter's matches) with their queue
    // positions; `Suggested` over the first suggestion shown.
    let shown = filter::queue_shown(state);
    if shown.is_empty() && filter.active() {
        let line = Line::styled(
            no_match_row(&filter.text, usize::from(inner.width)),
            Style::new().add_modifier(Modifier::DIM),
        );
        frame.render_widget(Paragraph::new(vec![line]), inner);
        return;
    }
    let mut rows = Vec::with_capacity(shown.len() + 1);
    let mut suggested = false;
    for (n, i) in shown.into_iter().enumerate() {
        let entry = &queue[i];
        if entry.suggested && !suggested {
            rows.push(Row::Suggested);
        }
        suggested = entry.suggested;
        rows.push(Row::Entry(i, n, entry));
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
                .position(|r| matches!(r, Row::Entry(_, _, e) if e.id == id))
        })
        .unwrap_or(0);
    let offset = anchor
        .saturating_sub(height / 2)
        .min(rows.len().saturating_sub(height));

    let width = usize::from(inner.width);
    // Spec 0020 "How it looks": the `+ ` marker column once the queue has a
    // selection, dropped when the window is too narrow for it.
    let marked = select::queue_entries(state);
    let marker = !marked.is_empty() && width >= 10;
    let columns = Columns::new(if marker { width - 2 } else { width }, queue.len());
    let lines: Vec<Line> = rows
        .iter()
        .skip(offset)
        .take(height)
        .map(|row| match row {
            Row::Suggested => Line::styled(
                fit(&divider(width), width),
                Style::new().add_modifier(Modifier::DIM),
            ),
            Row::Entry(i, n, entry) => {
                // Spec 0017 "State cues on top of the theme".
                let mut style = if n % 2 == 1 {
                    of(look, Component::SecondaryRow)
                } else {
                    Style::new()
                };
                if Some(entry.id) == current {
                    style = style.patch(of(look, Component::CurrentPlaying));
                }
                if entry.suggested {
                    style = style.add_modifier(Modifier::DIM);
                }
                let is_marked = marked.contains(&entry.id);
                if is_marked {
                    style = style.patch(of(look, Component::SelectedRow));
                }
                if Some(entry.id) == state.cursor {
                    style = style.patch(of(look, Component::Selection));
                }
                let text = columns.row(*i, entry, Some(entry.id) == current);
                let lead = match (marker, is_marked) {
                    (false, _) => "",
                    (true, true) => "+ ",
                    (true, false) => "  ",
                };
                Line::styled(format!("{lead}{text}"), style)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// The queue's title with the selection's parts after the filter's:
/// `Queue (12 · 3 selected, 1 hidden · motion)`.
fn queue_title(state: &State) -> String {
    let parts = select::queue_marks(state).title_parts();
    pages::with_marks(filter::queue_title(state), true, &parts)
}

/// `No rows match "xyz"` across `width`, the filter's text cut with `…`
/// to fit (spec 0012 "Drawing").
fn no_match_row(text: &str, width: usize) -> String {
    let frame = text_width(&no_match(""));
    fit(&no_match(&fit(text, width.saturating_sub(frame))), width)
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
    draw_prompt(frame, area, state.theme(), label, &prompt.text);
}

/// One row of `label` and `text` in `prompt`, the end of the text kept in
/// view, with the cursor after it.
fn draw_prompt(frame: &mut Frame, area: Rect, look: &Theme, label: &str, text: &str) {
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
    clear(frame, area, look);
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            line,
            of(look, Component::Prompt),
        )])),
        area,
    );
    frame.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
}

/// Clears `area` to the screen's style (what is drawn over it starts there,
/// spec 0017).
fn clear(frame: &mut Frame, area: Rect, look: &Theme) {
    frame.render_widget(Clear, area);
    frame
        .buffer_mut()
        .set_style(area, theme::style(look.screen));
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
mod tests;
