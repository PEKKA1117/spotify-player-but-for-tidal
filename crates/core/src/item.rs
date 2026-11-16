//! Items the queue is filled from: a track ID or a Tidal link (spec 0004
//! "Filling the queue").

use serde::{Deserialize, Serialize};

use crate::track::TrackId;

/// What a command-line argument or a pasted link names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    Track(TrackId),
    Album(u64),
    /// A playlist UUID, as it appears in the link.
    Playlist(String),
}

/// Not a Tidal track, album or playlist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Not a Tidal track, album or playlist: {0}")]
pub struct ItemError(pub String);

impl std::fmt::Display for Item {
    /// `Track 1`, `Album 1`, `Playlist <uuid>`: how messages name an item.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Track(id) => write!(f, "Track {id}"),
            Self::Album(id) => write!(f, "Album {id}"),
            Self::Playlist(uuid) => write!(f, "Playlist {uuid}"),
        }
    }
}

/// What a Tidal link names (spec 0030): an item the queue takes, or an
/// artist or a mix, which only open a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Item(Item),
    Artist(u64),
    /// A mix ID, as written.
    Mix(String),
}

impl Link {
    /// The link *Copy link* copies: `https://tidal.com/browse/<kind>/<id>`.
    pub fn url(&self) -> String {
        let (kind, id) = match self {
            Self::Item(Item::Track(id)) => ("track", id.to_string()),
            Self::Item(Item::Album(id)) => ("album", id.to_string()),
            Self::Item(Item::Playlist(uuid)) => ("playlist", uuid.clone()),
            Self::Artist(id) => ("artist", id.to_string()),
            Self::Mix(id) => ("mix", id.clone()),
        };
        format!("https://tidal.com/browse/{kind}/{id}")
    }
}

/// Reads a link (or a bare track ID) with [`parse_item`]'s rules, artist
/// and mix links too (spec 0030 AC1).
pub fn parse_link(input: &str) -> Option<Link> {
    read_link(input.trim())
}

/// Hosts whose links are accepted (compared in lower case).
const HOSTS: [&str; 3] = ["tidal.com", "www.tidal.com", "listen.tidal.com"];

/// Reads an item from a command-line argument or pasted text (AC16): a bare
/// track ID, or a track, album or playlist link (`https://` on a Tidal host,
/// or `tidal://`), with an optional `/browse` prefix, trailing slash, query
/// string or fragment. Anything else, including a link that also names a
/// track inside an album, is refused with the input quoted as given.
pub fn parse_item(input: &str) -> Result<Item, ItemError> {
    match read_link(input.trim()) {
        Some(Link::Item(item)) => Ok(item),
        _ => Err(ItemError(input.to_owned())),
    }
}

fn read_link(text: &str) -> Option<Link> {
    if is_number(text) {
        return text
            .parse()
            .ok()
            .map(|id| Link::Item(Item::Track(TrackId(id))));
    }
    let (scheme, rest) = text.split_once("://")?;
    let rest = rest.split(['?', '#']).next()?;
    let path = if scheme.eq_ignore_ascii_case("tidal") {
        rest
    } else if scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http") {
        let (host, path) = rest.split_once('/')?;
        if !HOSTS.contains(&host.to_ascii_lowercase().as_str()) {
            return None;
        }
        path
    } else {
        return None;
    };
    let mut segments: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    if segments
        .first()
        .is_some_and(|s| s.eq_ignore_ascii_case("browse"))
    {
        segments.remove(0);
    }
    // The share menu adds `/u` (`https://tidal.com/track/1/u`, 0004 Bugs).
    if segments.len() == 3 && segments[2] == "u" {
        segments.pop();
    }
    let [kind, id] = segments[..] else {
        return None;
    };
    let item = |item| Some(Link::Item(item));
    match kind.to_ascii_lowercase().as_str() {
        "track" if is_number(id) => item(Item::Track(TrackId(id.parse().ok()?))),
        "album" if is_number(id) => item(Item::Album(id.parse().ok()?)),
        "playlist" if is_uuid(id) => item(Item::Playlist(id.to_ascii_lowercase())),
        "artist" if is_number(id) => id.parse().ok().map(Link::Artist),
        // Spec 0030: a mix ID is letters and digits, kept as written.
        "mix" if !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()) => {
            Some(Link::Mix(id.to_owned()))
        }
        _ => None,
    }
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `8-4-4-4-12` hexadecimal digits.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests;
