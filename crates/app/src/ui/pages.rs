//! Rendering of the library's pages and popups (spec 0006 "Rendering"):
//! the title row, the windows (side by side from 60 columns, else the
//! focused one), their rows, and the popups over the page. Drawn from the
//! UI model's public API only; the queue page is drawn by `ui.rs`.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use tidal_player_core::library::{AlbumKind, TopHit};
use tidal_player_core::ui::{
    Load, NEW_PLAYLIST, PLAYLIST_NAME, Page, PageKind, Popup, ROLE_CATEGORIES, Row, Search,
    SearchFocus, State, Window, WindowKind,
};

use super::{Columns, draw_prompt, fit, pad, tail, text_width};

/// From this many columns inside the frame a page draws its windows side
/// by side; below, only the focused one.
const SIDE_BY_SIDE: u16 = 60;
/// A credits row shows the album column as well as the roles from this
/// window width.
const ROLES_WIDE: usize = 100;
/// The roles column of a wide credits row.
const ROLES_COLUMN: usize = 22;
/// The widest popup.
const POPUP_WIDTH: u16 = 50;
/// The `‹Tab›` marker in a window's title.
const TAB: &str = "‹Tab›";
/// The search input's label and the cursor block after its text while it
/// has the focus (spec 0007 "Rendering").
const SEARCH_LABEL: &str = "Search: ";
const SEARCH_CURSOR: &str = "▏";
const TOP_HIT_LABEL: &str = "Top hit: ";

/// What a window shows in place of its rows, or its rows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// Its rows (or its own loading, failure or empty message).
    Rows,
    /// The page is being fetched: `Loading…`.
    Loading,
    /// Nothing: a search page before its first search.
    Blank,
}

/// Rows of a list window for a page area of `page` (the title row and the
/// window's two border rows excluded).
pub(super) fn list_height(page: Rect) -> usize {
    usize::from(page.height).saturating_sub(3)
}

/// What a window's title says about the others.
#[derive(Clone)]
enum Hint {
    /// The only window of its page.
    None,
    /// One of several, alone on the screen: `‹Tab›`.
    Tab,
    /// A pane of tabs (the window indices, in order; the slot's window is
    /// the active one): `A (n) │ B  [ ]`, and `‹Tab›` when `tab` (only the
    /// focused pane is on screen, other panes exist).
    Tabs { tabs: Vec<usize>, tab: bool },
}

struct Slot {
    rect: Rect,
    window: usize,
    hint: Hint,
}

/// Draws the page on top of the history into `area`: the title row, then
/// the windows (or the failure message).
pub(super) fn render_page(state: &State, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let page = state.page();
    let width = usize::from(area.width);
    frame.render_widget(
        Paragraph::new(Line::styled(
            fit(&page.title(), width),
            Style::new().add_modifier(Modifier::BOLD),
        )),
        Rect { height: 1, ..area },
    );
    let body = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    if body.height == 0 {
        return;
    }
    let body = match &page.search {
        Some(search) => search_rows(frame, page, search, body),
        None => body,
    };
    if body.height == 0 {
        return;
    }
    if let Load::Failed(message) = &page.load {
        let lines: Vec<Line> = wrap(message, width)
            .into_iter()
            .map(|l| Line::styled(l, Style::new().fg(Color::Red)))
            .collect();
        frame.render_widget(Paragraph::new(lines), body);
        return;
    }
    let fill = if matches!(page.load, Load::Loading { .. }) {
        Fill::Loading
    } else if page.kind == PageKind::Search(String::new()) {
        Fill::Blank
    } else {
        Fill::Rows
    };
    for slot in slots(page, body) {
        if slot.window >= page.windows.len() {
            continue;
        }
        render_window(
            frame,
            slot.rect,
            slot.window,
            &page.windows,
            page.windows_focused() && slot.window == page.focus,
            &slot.hint,
            fill,
        );
    }
}

/// Draws a search page's input row and, when there is one, its top-hit
/// row at the top of `body` (spec 0007 "Rendering"); returns the rows
/// left for the windows.
fn search_rows(frame: &mut Frame, page: &Page, search: &Search, body: Rect) -> Rect {
    let width = usize::from(body.width);
    let focused = search.focus == SearchFocus::Input;
    // The end of the query stays in view, one column kept for the cursor.
    let room = width.saturating_sub(text_width(SEARCH_LABEL) + 1);
    let query = if text_width(&search.input) <= room {
        search.input.clone()
    } else {
        format!("…{}", tail(&search.input, room.saturating_sub(1)))
    };
    let cursor = if focused { SEARCH_CURSOR } else { "" };
    let style = if focused {
        Style::new().add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            fit(&format!("{SEARCH_LABEL}{query}{cursor}"), width),
            style,
        )),
        Rect { height: 1, ..body },
    );
    let mut rest = Rect {
        y: body.y + 1,
        height: body.height - 1,
        ..body
    };
    if let Some(hit) = &search.top_hit
        && rest.height > 0
        && page.load == Load::Idle
    {
        let mut style = Style::new();
        if matches!(hit, TopHit::Track(t) if !t.streamable) {
            style = style.add_modifier(Modifier::DIM);
        }
        if search.focus == SearchFocus::TopHit {
            style = style.add_modifier(Modifier::REVERSED);
        }
        frame.render_widget(
            Paragraph::new(Line::styled(fit(&top_hit_row(hit), width), style)),
            Rect { height: 1, ..rest },
        );
        rest.y += 1;
        rest.height -= 1;
    }
    rest
}

/// `Top hit: Pierce The Veil · artist`.
fn top_hit_row(hit: &TopHit) -> String {
    let (name, kind) = match hit {
        TopHit::Track(t) => (t.title.as_str(), "track"),
        TopHit::Album(a) => (a.title.as_str(), "album"),
        TopHit::Artist(a) => (a.name.as_str(), "artist"),
        TopHit::Playlist(p) => (p.title.as_str(), "playlist"),
    };
    format!("{TOP_HIT_LABEL}{name} · {kind}")
}

/// Where each drawn window goes (spec 0006 "Rendering").
fn slots(page: &Page, body: Rect) -> Vec<Slot> {
    if page.windows.is_empty() {
        return Vec::new();
    }
    let focus = page.focus.min(page.windows.len() - 1);
    if page.windows.len() == 1 {
        return vec![Slot {
            rect: body,
            window: 0,
            hint: Hint::None,
        }];
    }
    if body.width < SIDE_BY_SIDE {
        return vec![Slot {
            rect: body,
            window: focus,
            hint: pane_hint(page, page.focused_pane(), true),
        }];
    }
    let split = |at: u16| {
        let left = Rect { width: at, ..body };
        let right = Rect {
            x: body.x + at,
            width: body.width - at,
            ..body
        };
        (left, right)
    };
    match page.kind {
        PageKind::Library => {
            let w = body.width * 40 / 100;
            let (playlists, rest) = split(w);
            let albums = Rect { width: w, ..rest };
            let artists = Rect {
                x: rest.x + w,
                width: rest.width - w,
                ..rest
            };
            [playlists, albums, artists]
                .into_iter()
                .enumerate()
                .map(|(window, rect)| Slot {
                    rect,
                    window,
                    hint: Hint::None,
                })
                .collect()
        }
        PageKind::Search(_) => {
            // Tracks | Albums over Artists | Playlists, each half the
            // width and half the height.
            let left = body.width / 2;
            let top = body.height - body.height / 2;
            let column = |x: u16, width: u16| {
                [
                    Rect {
                        x,
                        width,
                        height: top,
                        ..body
                    },
                    Rect {
                        x,
                        y: body.y + top,
                        width,
                        height: body.height - top,
                    },
                ]
            };
            let [tracks, artists] = column(body.x, left);
            let [albums, playlists] = column(body.x + left, body.width - left);
            [tracks, albums, artists, playlists]
                .into_iter()
                .enumerate()
                .map(|(window, rect)| Slot {
                    rect,
                    window,
                    hint: Hint::None,
                })
                .collect()
        }
        PageKind::Artist(_) => {
            let (left, right) = split(body.width * 60 / 100);
            // Each pane shows its active tab.
            [left, right]
                .into_iter()
                .enumerate()
                .filter_map(|(pane, rect)| {
                    Some(Slot {
                        rect,
                        window: *page.panes.get(pane)?.get(*page.tabs.get(pane)?)?,
                        hint: pane_hint(page, pane, false),
                    })
                })
                .collect()
        }
        _ => vec![Slot {
            rect: body,
            window: focus,
            hint: Hint::Tab,
        }],
    }
}

/// The hint of `pane`'s window: its tabs when it has several, else `‹Tab›`
/// when alone on the screen.
fn pane_hint(page: &Page, pane: usize, alone: bool) -> Hint {
    let tabs = page.panes[pane].clone();
    if tabs.len() > 1 {
        Hint::Tabs {
            tabs,
            tab: alone && page.panes.len() > 1,
        }
    } else if alone {
        Hint::Tab
    } else {
        Hint::None
    }
}

fn render_window(
    frame: &mut Frame,
    area: Rect,
    active: usize,
    windows: &[Window],
    focused: bool,
    hint: &Hint,
    fill: Fill,
) {
    let window = &windows[active];
    let room = usize::from(area.width).saturating_sub(2);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let base = if focused { bold } else { dim };
    let title = match hint {
        Hint::None => Line::styled(fit(&window.title(), room), base),
        Hint::Tab => Line::styled(fit(&format!("{} {TAB}", window.title()), room), base),
        Hint::Tabs { tabs, tab } => {
            let suffix = if *tab {
                format!("  [ ] {TAB}")
            } else {
                "  [ ]".to_owned()
            };
            // The active tab is highlighted; the others are named only.
            let mut spans = Vec::new();
            let mut text = String::new();
            for (n, &at) in tabs.iter().enumerate() {
                if n > 0 {
                    spans.push(Span::styled(" │ ", base));
                    text += " │ ";
                }
                let name = if at == active {
                    let title = window.title();
                    spans.push(Span::styled(
                        title.clone(),
                        base.add_modifier(Modifier::REVERSED),
                    ));
                    title
                } else {
                    let name = windows
                        .get(at)
                        .map_or(String::new(), |w| w.kind.name().to_owned());
                    spans.push(Span::styled(name.clone(), base));
                    name
                };
                text += &name;
            }
            spans.push(Span::styled(suffix.clone(), base));
            text += &suffix;
            if text_width(&text) <= room {
                Line::from(spans)
            } else {
                // The other tabs' names are the first thing a narrow title
                // gives up.
                Line::styled(fit(&format!("{}{suffix}", window.title()), room), base)
            }
        }
    };
    let block = Block::bordered()
        .border_style(if focused { Style::new() } else { dim })
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let lines = window_lines(
        window,
        fill,
        focused,
        usize::from(inner.width),
        usize::from(inner.height),
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

fn message_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    wrap(text, width)
        .into_iter()
        .map(|l| Line::styled(l, style))
        .collect()
}

/// The visible rows of `window`, scrolled to keep the cursor in view.
fn window_lines(
    window: &Window,
    fill: Fill,
    focused: bool,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let plain = Style::new();
    let dim = Style::new().add_modifier(Modifier::DIM);
    let red = Style::new().fg(Color::Red);
    if fill == Fill::Blank {
        return Vec::new();
    }
    // Nothing fetched yet (or the page is being fetched again).
    if fill == Fill::Loading || (window.total.is_none() && !matches!(window.load, Load::Failed(_)))
    {
        return message_lines("Loading…", width, dim);
    }
    if window.is_empty() {
        return match &window.load {
            Load::Failed(message) => message_lines(message, width, red),
            Load::Loading { .. } => message_lines("Loading more…", width, dim),
            Load::Idle => message_lines(&window.empty_message(), width, dim),
        };
    }

    let count = window.len();
    let number = window.total.map_or(count, |t| t as usize).max(count);
    let columns = Columns::new(width, number);
    let credits_columns =
        (width >= ROLES_WIDE).then(|| Columns::new(width.saturating_sub(2 + ROLES_COLUMN), number));
    let mut rows: Vec<(String, Style)> = (0..count)
        .filter_map(|i| {
            let row = window.row(i)?;
            let mut style = plain;
            if row.track().is_some_and(|t| !t.streamable) {
                style = style.add_modifier(Modifier::DIM);
            }
            if i == window.cursor {
                style = style.add_modifier(Modifier::REVERSED);
                if !focused {
                    style = style.add_modifier(Modifier::DIM);
                }
            }
            let text = match row {
                Row::Track(track) => columns.track_row(i, track, "  ", None),
                Row::Credit(credit) => {
                    let roles = credit
                        .roles
                        .iter()
                        .map(|r| format!("{r:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    match &credits_columns {
                        Some(wide) => format!(
                            "{}  {}",
                            wide.track_row(i, &credit.track, "  ", None),
                            pad(&fit(&roles, ROLES_COLUMN), ROLES_COLUMN)
                        ),
                        None => columns.track_row(i, &credit.track, "  ", Some(&roles)),
                    }
                }
                Row::Album(album) => album_row(album, width),
                // A search's playlists are not the user's: no heart.
                Row::Playlist(playlist) => {
                    playlist_row(playlist, width, window.kind != WindowKind::SearchPlaylists)
                }
                Row::Artist(artist) => fit(&artist.name, width),
            };
            Some((text, style))
        })
        .collect();
    match &window.load {
        Load::Idle => {}
        Load::Loading { .. } => rows.push(("Loading more…".into(), dim)),
        Load::Failed(message) => rows.push((fit(message, width), red)),
    }
    let offset = scroll(window.cursor, height, rows.len());
    rows.into_iter()
        .skip(offset)
        .take(height)
        .map(|(text, style)| Line::styled(text, style))
        .collect()
}

/// The first row shown: the cursor stays in the middle of the view once
/// the list scrolls.
fn scroll(cursor: usize, height: usize, rows: usize) -> usize {
    cursor
        .saturating_sub(height / 2)
        .min(rows.saturating_sub(height))
}

/// `title  artists  2012 EP`: the year column goes with the artists as
/// the window narrows.
fn album_row(album: &tidal_player_core::library::AlbumSummary, width: usize) -> String {
    let artists = album
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let kind = match album.kind {
        AlbumKind::Album => None,
        AlbumKind::Ep => Some("EP"),
        AlbumKind::Single => Some("Single"),
    };
    let year = album.year.map(|y| y.to_string());
    let right = if width >= 44 {
        [year, kind.map(str::to_owned)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
    } else if width >= 30 {
        year.unwrap_or_default()
    } else {
        String::new()
    };
    let rw = match width {
        44.. => 11,
        30.. => 4,
        _ => 0,
    };
    if width < 20 {
        return fit(&album.title, width);
    }
    let gap = if rw > 0 { 2 } else { 0 };
    let avail = width - rw - gap - 2;
    let title = avail * 3 / 5;
    let by = avail - title;
    let mut row = format!(
        "{}  {}",
        pad(&fit(&album.title, title), title),
        pad(&fit(&artists, by), by)
    );
    if rw > 0 {
        row += &format!("  {right:>rw$}");
    }
    row
}

/// `♥ title   42`: a favorite (followed) playlist has the heart, when
/// `hearts` (not in search results, spec 0007 decision 5).
fn playlist_row(
    playlist: &tidal_player_core::library::PlaylistSummary,
    width: usize,
    hearts: bool,
) -> String {
    let prefix = if hearts && !playlist.own {
        "♥ "
    } else {
        "  "
    };
    let count = playlist.tracks.map(|n| n.to_string()).unwrap_or_default();
    const COUNT: usize = 5;
    if width < 2 + 1 + COUNT + 4 {
        return fit(&format!("{prefix}{}", playlist.title), width);
    }
    let title = width - 2 - 1 - COUNT;
    format!(
        "{prefix}{} {count:>COUNT$}",
        pad(&fit(&playlist.title, title), title)
    )
}

// --- popups ----------------------------------------------------------------------------

/// Draws the open popup, if any, over the page area (centred, at most 50
/// columns wide, clipped to the page). The name prompt takes `prompt_row`
/// like the open prompt does.
pub(super) fn render_popup(state: &State, frame: &mut Frame, page: Option<Rect>, prompt_row: Rect) {
    let Some(popup) = state.popup.as_ref() else {
        return;
    };
    if let Popup::NewPlaylist { name, .. } = popup {
        draw_prompt(frame, prompt_row, PLAYLIST_NAME, name);
        return;
    }
    let Some(page) = page else {
        return;
    };
    match popup {
        Popup::Actions {
            title,
            actions,
            cursor,
        } => {
            let labels: Vec<String> = actions.iter().map(|a| a.label()).collect();
            menu(frame, page, title, &labels, *cursor, None);
        }
        Popup::AddToPlaylist {
            playlists, cursor, ..
        } => {
            let mut labels = vec![NEW_PLAYLIST.to_owned()];
            labels.extend(popup.own_playlists().iter().map(|p| p.title.clone()));
            let status = match &playlists.load {
                Load::Idle => None,
                Load::Loading { .. } => Some(("Loading…".to_owned(), Modifier::DIM)),
                Load::Failed(message) => Some((message.clone(), Modifier::empty())),
            };
            menu(frame, page, "Add to playlist", &labels, *cursor, status);
        }
        Popup::Confirm { question, .. } => {
            let inner_width = question_width(question, page);
            let lines = wrap(question, inner_width);
            let want_w = lines.iter().map(|l| text_width(l)).max().unwrap_or(0) + 4;
            let rect = centred(page, want_w, lines.len() + 2);
            let block = Block::bordered();
            let inner = block.inner(rect);
            frame.render_widget(Clear, rect);
            frame.render_widget(block, rect);
            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .map(|l| Line::raw(format!(" {l}")))
                        .collect::<Vec<_>>(),
                ),
                inner,
            );
        }
        Popup::Roles { checked, cursor } => {
            let labels: Vec<String> = ROLE_CATEGORIES
                .iter()
                .zip(checked)
                .map(|(category, on)| format!("[{}] {category:?}", if *on { 'x' } else { ' ' }))
                .collect();
            menu(frame, page, "Roles", &labels, *cursor, None);
        }
        Popup::NewPlaylist { .. } => {}
    }
}

/// The columns a `y/n` question may wrap to inside its popup.
fn question_width(question: &str, page: Rect) -> usize {
    let room = usize::from(POPUP_WIDTH.min(page.width)).saturating_sub(4);
    text_width(question).min(room).max(1)
}

/// A bordered list popup titled `title` with `labels` as rows and one
/// optional status row below them.
fn menu(
    frame: &mut Frame,
    page: Rect,
    title: &str,
    labels: &[String],
    cursor: usize,
    status: Option<(String, Modifier)>,
) {
    let widest = labels
        .iter()
        .map(|l| text_width(l))
        .chain(std::iter::once(text_width(title)))
        .max()
        .unwrap_or(0);
    let rows = labels.len() + usize::from(status.is_some());
    let rect = centred(page, (widest + 4).max(24), rows + 2);
    let block = Block::bordered().title(Line::styled(
        fit(title, usize::from(rect.width).saturating_sub(2)),
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let width = usize::from(inner.width);
    let height = usize::from(inner.height);
    let mut lines: Vec<(String, Style)> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == cursor {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            };
            (pad(&fit(&format!(" {label}"), width), width), style)
        })
        .collect();
    if let Some((text, modifier)) = status {
        lines.push((
            fit(&format!(" {text}"), width),
            Style::new().add_modifier(modifier),
        ));
    }
    let offset = cursor
        .saturating_sub(height - 1)
        .min(lines.len().saturating_sub(height));
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(offset)
                .take(height)
                .map(|(text, style)| Line::styled(text, style))
                .collect::<Vec<_>>(),
        ),
        inner,
    );
}

/// A `width` × `height` rectangle centred in `area`, at most 50 columns
/// wide and clipped to `area`.
fn centred(area: Rect, width: usize, height: usize) -> Rect {
    let width = u16::try_from(width)
        .unwrap_or(u16::MAX)
        .min(POPUP_WIDTH)
        .min(area.width);
    let height = u16::try_from(height).unwrap_or(u16::MAX).min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

// --- text ------------------------------------------------------------------------------

/// `text` broken into lines of at most `width` columns, at spaces where it
/// can (a longer word is cut).
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut word = word.to_owned();
        loop {
            let gap = usize::from(!current.is_empty());
            if text_width(&current) + gap + text_width(&word) <= width {
                if gap == 1 {
                    current.push(' ');
                }
                current.push_str(&word);
                break;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                continue;
            }
            // A word wider than the line: cut it.
            let mut head = String::new();
            let mut used = 0;
            let mut rest = String::new();
            for c in word.chars() {
                let w = text_width(c.encode_utf8(&mut [0; 4]));
                if rest.is_empty() && used + w <= width {
                    head.push(c);
                    used += w;
                } else {
                    rest.push(c);
                }
            }
            if head.is_empty() {
                // A single character wider than the line.
                break;
            }
            lines.push(head);
            word = rest;
            if word.is_empty() {
                break;
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
