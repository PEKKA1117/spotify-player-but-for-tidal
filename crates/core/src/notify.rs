//! Desktop notifications on track change (spec 0029), their pure part: the
//! format and its placeholders, which started tracks notify, and the
//! `Notify` call's arguments. The process side is the binary's
//! `player::notify` module.

use std::fmt;
use std::path::Path;

use crate::{Track, TrackId};

#[cfg(test)]
mod tests;

/// The `app_name` of every notification.
pub const APP_NAME: &str = "tidal-player";
/// spotify-player's default `notify_format.summary`.
pub const DEFAULT_SUMMARY: &str = "{track} • {artists}";
/// spotify-player's default `notify_format.body`.
pub const DEFAULT_BODY: &str = "{album}";

/// A format that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// An unknown placeholder in the summary, as written (`{title}`).
    Summary(String),
    /// An unknown placeholder in the body, as written.
    Body(String),
    /// The summary is empty.
    EmptySummary,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Summary(name) | Self::Body(name) => write!(
                f,
                "unknown placeholder {name}: expected {{track}}, {{artists}} or {{album}}"
            ),
            Self::EmptySummary => f.write_str("the summary is empty"),
        }
    }
}

/// One piece of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    Track,
    Artists,
    Album,
}

/// A text with placeholders, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    parts: Vec<Part>,
}

impl Template {
    /// Parses `text`; `Err` holds the first unknown placeholder, braces
    /// included.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            literal.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            // A `{` with no `}` before the next `{` (or the end) is text.
            let Some(close) = after
                .find(['{', '}'])
                .filter(|&i| after[i..].starts_with('}'))
            else {
                literal.push('{');
                rest = after;
                continue;
            };
            let part = match &after[..close] {
                "track" => Part::Track,
                "artists" => Part::Artists,
                "album" => Part::Album,
                name => return Err(format!("{{{name}}}")),
            };
            if !literal.is_empty() {
                parts.push(Part::Text(std::mem::take(&mut literal)));
            }
            parts.push(part);
            rest = &after[close + 1..];
        }
        literal.push_str(rest);
        if !literal.is_empty() {
            parts.push(Part::Text(literal));
        }
        Ok(Self { parts })
    }

    fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// The text with the placeholders filled from `track`.
    pub fn render(&self, track: &Track) -> String {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => text.clone(),
                Part::Track => track.display_title(),
                Part::Artists => track.artist_names(),
                Part::Album => track.album_title().unwrap_or_default().to_owned(),
            })
            .collect()
    }
}

/// `notify_format`: the summary and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyFormat {
    summary: Template,
    body: Template,
}

impl NotifyFormat {
    /// Checks both texts: no unknown placeholder, a summary that is not
    /// empty.
    pub fn new(summary: &str, body: &str) -> Result<Self, FormatError> {
        let summary = Template::parse(summary).map_err(FormatError::Summary)?;
        let body = Template::parse(body).map_err(FormatError::Body)?;
        if summary.is_empty() {
            return Err(FormatError::EmptySummary);
        }
        Ok(Self { summary, body })
    }

    /// The summary and the body for `track`.
    pub fn render(&self, track: &Track) -> (String, String) {
        (self.summary.render(track), self.body.render(track))
    }
}

impl Default for NotifyFormat {
    fn default() -> Self {
        Self::new(DEFAULT_SUMMARY, DEFAULT_BODY).expect("the default format is valid")
    }
}

/// The four settings (spec 0029 "The settings").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifySettings {
    /// `enable_notify`.
    pub enabled: bool,
    /// `notify_format`.
    pub format: NotifyFormat,
    /// `notify_timeout_in_secs`; `0`: the server's own time.
    pub timeout_secs: u16,
    /// `notify_transient`.
    pub transient: bool,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            format: NotifyFormat::default(),
            timeout_secs: 0,
            transient: false,
        }
    }
}

/// Which started tracks notify: not one with the ID of the track that
/// started before it (spec 0029 "Which tracks notify").
#[derive(Debug, Clone, Default)]
pub struct NotifyTracker {
    last: Option<TrackId>,
}

impl NotifyTracker {
    /// `track` started: whether it notifies.
    pub fn started(&mut self, track: &Track) -> bool {
        self.last.replace(track.id) != Some(track.id)
    }
}

/// `&`, `<` and `>` as markup entities.
pub fn escape_markup(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// Whether a server with `capabilities` reads the body as markup.
pub fn supports_markup(capabilities: &[String]) -> bool {
    capabilities.iter().any(|c| c == "body-markup")
}

/// A hint's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    Str(String),
    Bool(bool),
}

/// The arguments of one `org.freedesktop.Notifications.Notify` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub app_name: String,
    pub replaces_id: u32,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    /// In the order sent.
    pub hints: Vec<(String, Hint)>,
    /// Milliseconds; `-1`: the server decides.
    pub expire_timeout: i32,
}

impl Request {
    /// The call for `track`, with the cover at `cover` (an absolute path),
    /// replacing the notification `replaces_id` (`0`: none); `markup`: the
    /// server reads the body as markup.
    pub fn new(
        settings: &NotifySettings,
        track: &Track,
        cover: Option<&Path>,
        replaces_id: u32,
        markup: bool,
    ) -> Self {
        let (summary, body) = settings.format.render(track);
        let body = if markup { escape_markup(&body) } else { body };
        let mut hints = Vec::new();
        if let Some(cover) = cover {
            hints.push((
                "image-path".to_owned(),
                Hint::Str(cover.display().to_string()),
            ));
        }
        if settings.transient {
            hints.push(("transient".to_owned(), Hint::Bool(true)));
        }
        let expire_timeout = match settings.timeout_secs {
            0 => -1,
            secs => i32::from(secs) * 1000,
        };
        Self {
            app_name: APP_NAME.to_owned(),
            replaces_id,
            app_icon: String::new(),
            summary,
            body,
            actions: Vec::new(),
            hints,
            expire_timeout,
        }
    }
}
