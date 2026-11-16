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

const MIX: &str = "0016bd04834072e9da423a4f4d911d";

#[test]
fn ac1_link_round_trip() {
    let table = [
        (
            Link::Item(Item::Track(TrackId(77640617))),
            "https://tidal.com/browse/track/77640617",
        ),
        (
            Link::Item(Item::Album(123)),
            "https://tidal.com/browse/album/123",
        ),
        (Link::Artist(7804), "https://tidal.com/browse/artist/7804"),
        (
            Link::Item(Item::Playlist(UUID.into())),
            "https://tidal.com/browse/playlist/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        ),
        (
            Link::Mix(MIX.into()),
            "https://tidal.com/browse/mix/0016bd04834072e9da423a4f4d911d",
        ),
    ];
    for (link, url) in table {
        assert_eq!(link.url(), url, "{link:?}");
        assert_eq!(parse_link(url), Some(link.clone()), "{url}");
    }
}

#[test]
fn ac1_artist_and_mix_links() {
    let artist = Some(Link::Artist(7804));
    let mix = Some(Link::Mix(MIX.into()));
    let table: Vec<(&str, Option<Link>)> = vec![
        ("https://tidal.com/artist/7804", artist.clone()),
        ("https://listen.tidal.com/artist/7804", artist.clone()),
        ("https://www.tidal.com/browse/artist/7804/", artist.clone()),
        ("https://tidal.com/browse/artist/7804/u", artist.clone()),
        ("https://tidal.com/browse/artist/7804?u#x", artist.clone()),
        ("tidal://artist/7804", artist.clone()),
        ("  https://TIDAL.com/Artist/7804\n", artist.clone()),
        (
            "https://tidal.com/browse/mix/0016bd04834072e9da423a4f4d911d",
            mix.clone(),
        ),
        (
            "https://listen.tidal.com/mix/0016bd04834072e9da423a4f4d911d/",
            mix.clone(),
        ),
        ("tidal://mix/0016bd04834072e9da423a4f4d911d?u", mix.clone()),
        (
            "https://tidal.com/mix/0016bd04834072e9da423a4f4d911d/u",
            mix.clone(),
        ),
        (
            "https://tidal.com/mix/00ABcd",
            Some(Link::Mix("00ABcd".into())),
        ),
        // A track ID and the queue's links are links too.
        ("77640617", Some(Link::Item(Item::Track(TrackId(77640617))))),
        (
            "https://tidal.com/album/123/u",
            Some(Link::Item(Item::Album(123))),
        ),
        // Refused.
        ("https://tidal.com/browse/artist/abc", None),
        ("https://tidal.com/browse/artist/", None),
        ("https://tidal.com/browse/mix/", None),
        ("https://tidal.com/browse/mix/a-b", None),
        ("https://tidal.com/browse/mix/a b", None),
        ("https://example.com/artist/7804", None),
        ("https://tidal.com/browse/video/123", None),
        ("Listen: https://tidal.com/artist/7804", None),
        ("", None),
        ("hello", None),
    ];
    for (input, want) in table {
        assert_eq!(parse_link(input), want, "{input:?}");
    }
}

#[test]
fn ac1_parse_item_refuses_artist_and_mix() {
    for input in [
        "https://tidal.com/browse/artist/7804",
        "https://tidal.com/browse/mix/0016bd04834072e9da423a4f4d911d",
    ] {
        assert_eq!(parse_item(input), Err(ItemError(input.into())), "{input}");
        assert!(parse_link(input).is_some(), "{input}");
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
