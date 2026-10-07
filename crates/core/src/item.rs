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

/// Reads an item from a command-line argument or pasted text (AC16).
pub fn parse_item(input: &str) -> Result<Item, ItemError> {
    match input.trim().parse::<u64>() {
        Ok(id) => Ok(Item::Track(TrackId(id))),
        Err(_) => Err(ItemError(input.to_owned())),
    }
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
