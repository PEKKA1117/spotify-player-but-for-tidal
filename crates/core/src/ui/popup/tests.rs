use super::*;

fn items(n: u32, target: impl Fn(u32) -> RemovedTarget) -> Vec<Removed> {
    (1..=n)
        .map(|i| Removed {
            title: format!("T{i}"),
            target: target(i),
        })
        .collect()
}

/// Spec 0019 AC5: the question for one and several items, for the
/// favorites of each kind, a playlist and the queue.
#[test]
fn ac5_removal_question() {
    let favorites = |kind| RemoveFrom::Favorites(kind);
    let favorite = |i: u32| RemovedTarget::Favorite(i.to_string());
    let playlist = || RemoveFrom::Playlist {
        uuid: "p1".into(),
        title: "Mine".into(),
        etag: Some("e1".into()),
    };
    let cases = [
        (
            favorites(FavoriteKind::Track),
            1,
            "Remove T1 from Collected Tracks? (y/n)",
        ),
        (
            favorites(FavoriteKind::Album),
            1,
            "Remove T1 from your collection? (y/n)",
        ),
        (
            favorites(FavoriteKind::Artist),
            1,
            "Remove T1 from your collection? (y/n)",
        ),
        (
            favorites(FavoriteKind::Playlist),
            1,
            "Remove T1 from your collection? (y/n)",
        ),
        (
            favorites(FavoriteKind::Track),
            3,
            "Remove 3 tracks from Collected Tracks? (y/n)",
        ),
        (
            favorites(FavoriteKind::Album),
            3,
            "Remove 3 albums from your collection? (y/n)",
        ),
        (
            favorites(FavoriteKind::Artist),
            3,
            "Remove 3 artists from your collection? (y/n)",
        ),
        (
            favorites(FavoriteKind::Playlist),
            3,
            "Remove 3 playlists from your collection? (y/n)",
        ),
        (playlist(), 1, "Remove T1 from Mine? (y/n)"),
        (playlist(), 3, "Remove 3 tracks from Mine? (y/n)"),
        (RemoveFrom::Queue, 1, "Remove T1 from the queue? (y/n)"),
        (
            RemoveFrom::Queue,
            3,
            "Remove 3 tracks from the queue? (y/n)",
        ),
        // Spec 0020: deleting own playlists (`d s` in the library).
        (RemoveFrom::OwnPlaylists, 1, "Delete T1? (y/n)"),
        (RemoveFrom::OwnPlaylists, 3, "Delete 3 playlists? (y/n)"),
    ];
    for (from, n, want) in cases {
        let target = match &from {
            RemoveFrom::Favorites(_) => favorite,
            RemoveFrom::Playlist { .. } => RemovedTarget::Position,
            RemoveFrom::Queue => |i| RemovedTarget::Entry(EntryId(u64::from(i))),
            RemoveFrom::OwnPlaylists => |i: u32| RemovedTarget::Playlist(format!("p{i}")),
        };
        let removal = Removal {
            items: items(n, target),
            from,
        };
        assert_eq!(question(&removal), want, "{:?} x{n}", removal.from);
    }
}
/// 0028 AC8: a track's favorite texts say *Collected Tracks*, an
/// album's, artist's and playlist's the collection; none says
/// *favorite*.
#[test]
fn ac8_collection_texts() {
    use FavoriteKind::{Album, Artist, Playlist, Track};
    let mut texts: Vec<String> = Vec::new();
    // The popup's labels per kind.
    let kinds = [
        (
            Track,
            "Add to Collected Tracks",
            "Remove from Collected Tracks",
        ),
        (Album, "Add to collection", "Remove from collection"),
        (Artist, "Add to collection", "Remove from collection"),
        (Playlist, "Add to collection", "Remove from collection"),
    ];
    for (kind, add, remove) in kinds {
        assert_eq!(MenuAction::AddFavorite(kind, "1".into()).label(), add);
        assert_eq!(MenuAction::RemoveFavorite(kind, "1".into()).label(), remove);
        texts.extend([add.to_owned(), remove.to_owned()]);
    }
    // The questions for one and three items.
    let questions = [
        (Track, 1, "Remove T1 from Collected Tracks? (y/n)"),
        (Album, 1, "Remove T1 from your collection? (y/n)"),
        (Artist, 1, "Remove T1 from your collection? (y/n)"),
        (Playlist, 1, "Remove T1 from your collection? (y/n)"),
        (Track, 3, "Remove 3 tracks from Collected Tracks? (y/n)"),
        (Album, 3, "Remove 3 albums from your collection? (y/n)"),
        (Artist, 3, "Remove 3 artists from your collection? (y/n)"),
        (
            Playlist,
            3,
            "Remove 3 playlists from your collection? (y/n)",
        ),
    ];
    for (kind, n, want) in questions {
        let removal = Removal {
            from: RemoveFrom::Favorites(kind),
            items: items(n, |i| RemovedTarget::Favorite(i.to_string())),
        };
        assert_eq!(question(&removal), want, "{kind:?} x{n}");
        texts.push(question(&removal));
    }
    // The selection popup: tracks only, or any other selection.
    let rows = vec![(Track, Held::Row(0), "1".to_owned())];
    let selection = [
        (
            SelectionAction::AddFavorites {
                tracks_only: true,
                rows: rows.clone(),
            },
            "Add to Collected Tracks",
        ),
        (
            SelectionAction::AddFavorites {
                tracks_only: false,
                rows,
            },
            "Add to collection",
        ),
        (
            SelectionAction::RemoveFavorites { tracks_only: true },
            "Remove from Collected Tracks",
        ),
        (
            SelectionAction::RemoveFavorites { tracks_only: false },
            "Remove from collection",
        ),
    ];
    for (action, want) in selection {
        assert_eq!(action.label(), want);
        texts.push(action.label().to_owned());
    }
    // The selection list's questions, under a heading per list.
    let part = |from, n| SelectionPart {
        removal: Removal {
            from,
            items: items(n, |i| RemovedTarget::Favorite(i.to_string())),
        },
        held: (0..n as usize).map(Held::Row).collect(),
    };
    let origin = Origin::Window { page: 1, window: 0 };
    let own = SelectionPart {
        removal: Removal {
            from: RemoveFrom::OwnPlaylists,
            items: items(2, |i| RemovedTarget::Playlist(format!("p{i}"))),
        },
        held: vec![Held::Row(0), Held::Row(1)],
    };
    for (kind, heading) in [
        (Track, "Remove from Collected Tracks:"),
        (Album, "Remove from your collection:"),
        (Playlist, "Remove from your collection:"),
    ] {
        let removal = SelectionRemoval {
            origin,
            parts: vec![own.clone(), part(RemoveFrom::Favorites(kind), 1)],
        };
        let lines = target_lines(&Confirmed::RemoveSelection(removal));
        let headings: Vec<&str> = lines
            .iter()
            .filter(|l| l.heading)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(headings, ["Delete:", heading], "{kind:?}");
        texts.extend(
            lines
                .into_iter()
                .map(|l| l.text)
                .filter(|t| t.ends_with(':')),
        );
    }
    // Own and followed playlists together.
    let both = SelectionRemoval {
        origin,
        parts: vec![own, part(RemoveFrom::Favorites(Playlist), 1)],
    };
    let asked = selection_question(&both);
    assert_eq!(
        asked,
        "Delete 2 playlists and remove 1 from your collection? (y/n)"
    );
    texts.push(asked);
    for text in texts {
        assert!(!text.to_lowercase().contains("favorite"), "{text}");
    }
}
