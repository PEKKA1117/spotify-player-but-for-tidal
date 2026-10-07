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

/// Hosts whose links are accepted (compared in lower case).
const HOSTS: [&str; 3] = ["tidal.com", "www.tidal.com", "listen.tidal.com"];

/// Reads an item from a command-line argument or pasted text (AC16): a bare
/// track ID, or a track, album or playlist link (`https://` on a Tidal host,
/// or `tidal://`), with an optional `/browse` prefix, trailing slash, query
/// string or fragment. Anything else, including a link that also names a
/// track inside an album, is refused with the input quoted as given.
pub fn parse_item(input: &str) -> Result<Item, ItemError> {
    read_item(input.trim()).ok_or_else(|| ItemError(input.to_owned()))
}

fn read_item(text: &str) -> Option<Item> {
    if is_number(text) {
        return text.parse().ok().map(|id| Item::Track(TrackId(id)));
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
    let [kind, id] = segments[..] else {
        return None;
    };
    match kind.to_ascii_lowercase().as_str() {
        "track" if is_number(id) => id.parse().ok().map(|id| Item::Track(TrackId(id))),
        "album" if is_number(id) => id.parse().ok().map(Item::Album),
        "playlist" if is_uuid(id) => Some(Item::Playlist(id.to_ascii_lowercase())),
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
mod tests {
    use super::*;

    const UUID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    fn track(id: u64) -> Result<Item, ()> {
        Ok(Item::Track(TrackId(id)))
    }

    fn album(id: u64) -> Result<Item, ()> {
        Ok(Item::Album(id))
    }

    fn playlist(uuid: &str) -> Result<Item, ()> {
        Ok(Item::Playlist(uuid.to_owned()))
    }

    #[test]
    fn ac16_parse_item() {
        let pl = |suffix: &str| format!("https://tidal.com/browse/playlist/{UUID}{suffix}");
        let table: Vec<(String, Result<Item, ()>)> = vec![
            // Tracks.
            ("77640617".into(), track(77640617)),
            ("  77640617\n".into(), track(77640617)),
            (
                "https://tidal.com/browse/track/77640617".into(),
                track(77640617),
            ),
            ("https://tidal.com/track/77640617".into(), track(77640617)),
            (
                "https://listen.tidal.com/track/77640617".into(),
                track(77640617),
            ),
            (
                "https://listen.tidal.com/browse/track/77640617".into(),
                track(77640617),
            ),
            ("tidal://track/77640617".into(), track(77640617)),
            ("tidal://browse/track/77640617".into(), track(77640617)),
            (
                "https://tidal.com/browse/track/77640617?u".into(),
                track(77640617),
            ),
            // The share menu's `/u` suffix (0004 Bugs).
            (
                "https://tidal.com/track/145060431/u".into(),
                track(145060431),
            ),
            (
                "https://tidal.com/album/145060429/u".into(),
                album(145060429),
            ),
            (pl("/u"), playlist(UUID)),
            (
                "https://tidal.com/track/145060431/u/".into(),
                track(145060431),
            ),
            (
                "https://tidal.com/track/145060431/u?x=1".into(),
                track(145060431),
            ),
            (
                "https://tidal.com/browse/track/145060431/u".into(),
                track(145060431),
            ),
            ("https://tidal.com/track/145060431/x".into(), Err(())),
            ("https://tidal.com/track/145060431/u/u".into(), Err(())),
            (
                "https://tidal.com/track/77640617/?u=1&x=2".into(),
                track(77640617),
            ),
            (
                "https://tidal.com/browse/track/77640617/".into(),
                track(77640617),
            ),
            (
                "https://tidal.com/browse/track/77640617#frag".into(),
                track(77640617),
            ),
            (
                "https://www.tidal.com/browse/track/77640617".into(),
                track(77640617),
            ),
            (
                "HTTPS://TIDAL.COM/browse/track/77640617".into(),
                track(77640617),
            ),
            (
                "https://Listen.Tidal.Com/track/77640617".into(),
                track(77640617),
            ),
            ("http://tidal.com/track/77640617".into(), track(77640617)),
            // Albums.
            ("https://tidal.com/browse/album/123".into(), album(123)),
            ("https://tidal.com/browse/album/123/".into(), album(123)),
            ("https://tidal.com/album/123?u".into(), album(123)),
            ("tidal://album/123".into(), album(123)),
            // Playlists.
            (pl(""), playlist(UUID)),
            (pl("/"), playlist(UUID)),
            (pl("?u"), playlist(UUID)),
            (
                "tidal://playlist/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into(),
                playlist(UUID),
            ),
            (
                "https://tidal.com/playlist/0A1B2C3D-4E5F-4A6B-8C7D-9E0F1A2B3C4D".into(),
                playlist(UUID),
            ),
            // Refused: other kinds, hosts and junk.
            ("https://tidal.com/browse/artist/123".into(), Err(())),
            (
                "https://tidal.com/browse/mix/0016bd04834072e9da423a4f4d911d".into(),
                Err(()),
            ),
            ("https://tidal.com/browse/video/123".into(), Err(())),
            (
                "https://tidal.com/browse/album/123/track/456".into(),
                Err(()),
            ),
            ("https://example.com/track/77640617".into(), Err(())),
            ("https://tidal.com.evil.test/track/77640617".into(), Err(())),
            ("https://evil.test/tidal.com/track/77640617".into(), Err(())),
            ("https://user@tidal.com/track/77640617".into(), Err(())),
            ("ftp://tidal.com/track/77640617".into(), Err(())),
            ("spotify://track/77640617".into(), Err(())),
            ("https://tidal.com/browse/track/".into(), Err(())),
            ("https://tidal.com/browse/track/abc".into(), Err(())),
            ("https://tidal.com/browse/track/-5".into(), Err(())),
            ("https://tidal.com/browse/track/12/34".into(), Err(())),
            (
                "https://tidal.com/browse/playlist/not-a-uuid".into(),
                Err(()),
            ),
            ("https://tidal.com/browse/playlist/123".into(), Err(())),
            ("https://tidal.com/browse".into(), Err(())),
            ("https://tidal.com/".into(), Err(())),
            ("tidal://".into(), Err(())),
            ("track/77640617".into(), Err(())),
            ("-5".into(), Err(())),
            ("12.5".into(), Err(())),
            ("99999999999999999999999".into(), Err(())),
            ("".into(), Err(())),
            ("   ".into(), Err(())),
            ("hello".into(), Err(())),
        ];
        for (input, want) in table {
            let got = parse_item(&input);
            match want {
                Ok(item) => assert_eq!(got, Ok(item), "{input:?}"),
                Err(()) => assert_eq!(got, Err(ItemError(input.clone())), "{input:?}"),
            }
        }
    }

    #[test]
    fn item_display_names_the_kind() {
        assert_eq!(Item::Track(TrackId(1)).to_string(), "Track 1");
        assert_eq!(Item::Album(2).to_string(), "Album 2");
        assert_eq!(
            Item::Playlist(UUID.into()).to_string(),
            format!("Playlist {UUID}")
        );
    }
}
